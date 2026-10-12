//! USN 变更在缓存树上的应用域：upsert / 增量传播 / 节点移除。
//! 从 usn_tree.rs 拆分（2026-10-12）。纯树操作，无 IO 调用（删除路径
//! 的 stat 例外）。usn_tree.rs 经 pub(crate) use 重导出保持旧调用路径。

use std::path::Path;

use super::child_index;

/// Navigate (creating missing directories on the way) to `parent_rel`, then
/// upsert the leaf. File size / count deltas propagate up the ancestor chain.
pub(crate) fn apply_upsert(
    tree: &mut crate::Node,
    rel: &Path,
    parent_rel: &Path,
    is_dir: bool,
    size: u64,
    abs_path: &Path,
    keep_files: Option<usize>,
) {
    let leaf_name = rel
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut cur = &mut *tree;
    for seg in parent_rel.components() {
        let name = seg.as_os_str().to_string_lossy().to_string();
        match child_index(cur, &name) {
            Some(i) => {
                // Cached as a file but now holds children (dir promotion).
                if !cur.children[i].is_dir {
                    cur.children[i].is_dir = true;
                    cur.children[i].children = Vec::new();
                }
                cur = &mut cur.children[i];
            }
            None => {
                let path = child_abs_path(cur, &name);
                cur.children.push(crate::Node {
                    name,
                    path,
                    is_dir: true,
                    size: 0,
                    file_count: 0,
                    children: Vec::new(),
                    scaffold_id: None,
                    top_extensions: Vec::new(),
                    children_truncated: None,
                    mtime: None,
                });
                let last = cur.children.len() - 1;
                cur = &mut cur.children[last];
            }
        }
    }

    if is_dir {
        // Directory create event: ensure the node exists (empty dirs carry no
        // aggregate; their size comes from children's delta propagation).
        let path = child_abs_path(cur, &leaf_name);
        if child_index(cur, &leaf_name).is_none() {
            cur.children.push(crate::Node {
                name: leaf_name,
                path,
                is_dir: true,
                size: 0,
                file_count: 0,
                children: Vec::new(),
                scaffold_id: None,
                top_extensions: Vec::new(),
                children_truncated: None,
                mtime: None,
            });
        }
        return;
    }

    // File upsert.
    match child_index(cur, &leaf_name) {
        Some(i) => {
            let old = cur.children[i].size;
            cur.children[i].size = size;
            cur.children[i].path = abs_path.to_string_lossy().to_string();
            propagate_delta(tree, parent_rel, size as i64 - old as i64, 0);
        }
        None => {
            cur.children.push(crate::Node {
                name: leaf_name,
                path: abs_path.to_string_lossy().to_string(),
                is_dir: false,
                size,
                file_count: 1,
                children: Vec::new(),
                scaffold_id: None,
                top_extensions: Vec::new(),
                children_truncated: None,
                mtime: None,
            });
            // Trim to the same "all dirs + top-K files by size" shape the walk
            // uses; the dropped file still counted in the parent aggregate.
            cur.children.sort_by_key(|c| std::cmp::Reverse(c.size));
            if let Some(keep) = keep_files {
                let mut files_kept = 0usize;
                let mut i = cur.children.len();
                while i > 0 && files_kept < keep {
                    i -= 1;
                    if !cur.children[i].is_dir {
                        files_kept += 1;
                    }
                }
                // `i` is the index of the last kept file; drop any *file*
                // children past it (dirs stay).
                let mut j = cur.children.len();
                while j > i {
                    j -= 1;
                    if j >= i && !cur.children[j].is_dir {
                        cur.children.remove(j);
                    }
                }
            }
            propagate_delta(tree, parent_rel, size as i64, 1);
        }
    }
}

pub(crate) fn child_abs_path(node: &crate::Node, name: &str) -> String {
    let base = Path::new(&node.path);
    base.join(name).to_string_lossy().to_string()
}

/// Walk the existing chain from the root to `parent_rel`, adjusting each
/// node's size / file_count by the delta (saturating, never below zero).
pub(crate) fn propagate_delta(
    tree: &mut crate::Node,
    parent_rel: &Path,
    delta_size: i64,
    delta_count: i64,
) {
    fn apply(n: &mut crate::Node, ds: i64, dc: i64) {
        n.size = (n.size as i64).saturating_add(ds).max(0) as u64;
        n.file_count = (n.file_count as i64).saturating_add(dc).max(0) as u64;
    }
    let mut cur = &mut *tree;
    apply(cur, delta_size, delta_count);
    for seg in parent_rel.components() {
        let name = seg.as_os_str().to_string_lossy().to_string();
        match child_index(cur, &name) {
            Some(i) => {
                cur = &mut cur.children[i];
                apply(cur, delta_size, delta_count);
            }
            None => break, // chain not present in cache (new outside-subtree dir)
        }
    }
}

/// Remove a node by relative path. Returns true if a node was removed.
///
/// Two tricky cases must still roll back the parent aggregates:
/// - the file was below the parent's top-K (never materialized as a child) —
///   we still know its size from the FRN table stat, so propagate a pure delta;
/// - the node is a directory — `file_count` is a *recursive* count on the
///   cache tree (DirAcc accumulates children), so rolling back must subtract
///   the subtree's own recursive count, not 0.
pub(crate) fn remove_node(tree: &mut crate::Node, rel: &Path, size_hint: Option<u64>) -> bool {
    let parent_rel = rel.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let leaf_name = rel
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    // Navigate to the parent (no creation: deleting from a dir we know about).
    let mut cur = &mut *tree;
    let mut chain_ok = true;
    for seg in parent_rel.components() {
        let name = seg.as_os_str().to_string_lossy().to_string();
        match child_index(cur, &name) {
            Some(i) => cur = &mut cur.children[i],
            None => {
                chain_ok = false;
                break;
            }
        }
    }
    if !chain_ok {
        return false;
    }
    match child_index(cur, &leaf_name) {
        Some(i) => {
            // Node is materialized — remove it and propagate its real size /
            // recursive file_count up the chain.
            let gone = cur.children.remove(i);
            let ds = -(gone.size as i64);
            let dc = -(gone.file_count as i64);
            propagate_delta(tree, &parent_rel, ds, dc);
            true
        }
        None => {
            // Below top-K (never a child) or already gone. The FRN stat gives
            // us a size hint, so the parent aggregates still get corrected;
            // the count delta is -1 only when the hint claims a plain file.
            let Some(hint) = size_hint else {
                return false; // no size info and nothing to remove → no-op
            };
            let is_dir = std::fs::metadata(rel).map(|m| m.is_dir()).unwrap_or(false);
            let ds = -(hint as i64);
            let dc = if is_dir { 0 } else { -1 };
            propagate_delta(tree, &parent_rel, ds, dc);
            true
        }
    }
}
