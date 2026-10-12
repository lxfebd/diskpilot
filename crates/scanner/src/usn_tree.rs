//! USN 变更在缓存树（crate::Node）上的合并/应用/删除逻辑。
//!
//! 从 usn.rs 拆分（2026-10-12）：本模块是纯树操作（无 DeviceIoControl），
//! 依赖 crate::usn 的类型（UsnChange 等）与 crate::Node。usn.rs 经
//! `pub use crate::usn_tree::merge_changes;` 重导出保持旧调用路径。

use std::path::Path;

use crate::usn::{
    ResolvedChange, USN_REASON_FILE_DELETE, USN_REASON_RENAME_NEW_NAME, USN_REASON_RENAME_OLD_NAME,
};

pub(crate) fn child_index(node: &crate::Node, name: &str) -> Option<usize> {
    node.children
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(name))
}

/// 沿相对路径在缓存树里找节点的 size（找不到返回 None）。用于重命名旧路径
/// 删除时——路径已消失无法 stat，回滚聚合得靠缓存节点自己的真实 size。
pub(crate) fn node_size_at(tree: &crate::Node, rel: &Path) -> Option<u64> {
    let mut cur = tree;
    for seg in rel.components() {
        let name = seg.as_os_str().to_string_lossy().to_string();
        {
            let i = child_index(cur, &name)?;
            cur = &cur.children[i]
        }
    }
    Some(cur.size)
}

/// 递归重写子树所有节点的绝对 `path`（以节点自身 path 为基准）。目录整体
/// 搬移后缓存里的绝对路径必须跟着新位置走，否则后端内存树与全量扫描不一致。
pub(crate) fn rebase_subtree_paths(node: &mut crate::Node) {
    let base = Path::new(&node.path).to_path_buf();
    for c in &mut node.children {
        c.path = base.join(&c.name).to_string_lossy().to_string();
        rebase_subtree_paths(c);
    }
}

/// RENAME_OLD_NAME 与同批次同 FRN 的 RENAME_NEW_NAME 配对：Windows 对目录
/// 整体改名/移动会成对吐出这两个事件（实践中都在同一批 resolved 里）。配对
/// 成功就把旧路径的整棵子树搬到新路径——否则 remove_node 只摘掉空壳节点，
/// 子树内容不会掉到新路径，增量结果只剩 size=0 的空目录，与全量扫描不一致。
/// 聚合沿祖先链滚动：旧链回滚 -(size, count)、新链补回，共享祖先净零。
/// 返回 true 表示已搬移（调用方跳过删除分支）；false 走原删除语义。
pub(crate) fn try_rename_move(
    tree: &mut crate::Node,
    resolved: &[ResolvedChange],
    root: &Path,
    old_rel: &Path,
    old_parent_rel: &Path,
    frn: u64,
) -> bool {
    // 同批次里找同 FRN 的 RENAME_NEW_NAME（事件顺序不定，全量搜索；排除
    // 自己——本事件的 reason 可能同时带 RENAME_NEW_NAME 位但 path 仍是旧路径）。
    let old_abs = root.join(old_rel);
    let new_abs = match resolved
        .iter()
        .find(|r| r.frn == frn && r.reason & USN_REASON_RENAME_NEW_NAME != 0 && r.path != old_abs)
    {
        Some(r) => r.path.clone(),
        None => return false,
    };
    let Ok(new_rel) = new_abs.strip_prefix(root).map(|p| p.to_path_buf()) else {
        return false;
    };
    let Some(new_leaf) = new_rel.file_name().map(|s| s.to_string_lossy().to_string()) else {
        return false;
    };
    let new_parent_rel = new_rel
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();

    // 旧节点必须物化在树上且是目录（文件被移动时新路径会有独立的
    // FILE_CREATE/RENAME_NEW_NAME 事件走 apply_upsert 重建，无需搬移）。
    let mut cur = &mut *tree;
    for seg in old_parent_rel.components() {
        let name = seg.as_os_str().to_string_lossy().to_string();
        match child_index(cur, &name) {
            Some(i) => cur = &mut cur.children[i],
            None => return false,
        }
    }
    let Some(old_leaf) = old_rel.file_name().map(|s| s.to_string_lossy().to_string()) else {
        return false;
    };
    let old_idx = match child_index(cur, &old_leaf) {
        Some(i) => i,
        None => return false,
    };
    if !cur.children[old_idx].is_dir {
        return false;
    }
    let mut sub = cur.children.remove(old_idx);

    // 子树根改名 + 刷成新绝对路径，再递归重写后代路径。
    sub.name = new_leaf.clone();
    sub.path = new_abs.to_string_lossy().to_string();
    rebase_subtree_paths(&mut sub);

    let ds = -(sub.size as i64);
    let dc = -(sub.file_count as i64);
    // 先沿旧父链回滚，旧分支的 size/file_count 不再重复计入。
    propagate_delta(tree, old_parent_rel, ds, dc);

    // 沿新父链（必要时建中间节点）把子树挂上；新叶子已有占位节点（如同批
    // 较早的 new-name 事件建的空目录）则整体替换。
    let mut insert_stale = None;
    {
        let mut cur = &mut *tree;
        for seg in new_parent_rel.components() {
            let name = seg.as_os_str().to_string_lossy().to_string();
            match child_index(cur, &name) {
                Some(i) => {
                    // 与新路径冲突的“文件”节点做“文件→目录”晋升（同 apply_upsert）。
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
        match child_index(cur, &new_leaf) {
            Some(i) => {
                insert_stale = Some((
                    cur.children[i].size as i64,
                    cur.children[i].file_count as i64,
                ));
                cur.children[i] = sub;
            }
            None => cur.children.push(sub),
        }
    }
    // 被替换掉的占位节点若有聚合，先回滚再补回搬来的子树聚合。
    if let Some((s, c)) = insert_stale {
        if s != 0 || c != 0 {
            propagate_delta(tree, &new_parent_rel, -s, -c);
        }
    }
    propagate_delta(tree, &new_parent_rel, -ds, -dc);
    true
}

/// Apply the resolved changes to a cached tree in journal order. Returns the
/// number of changes applied structurally (add / remove / size update).
pub fn merge_changes(
    tree: &mut crate::Node,
    resolved: &[ResolvedChange],
    root: &Path,
    keep_files: Option<usize>,
) -> usize {
    let mut applied = 0usize;
    for rc in resolved {
        let rel = match rc.path.strip_prefix(root) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue, // outside the scanned subtree
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let parent_rel = rel.parent().map(|p| p.to_path_buf()).unwrap_or_default();

        // RENAME_OLD_NAME：旧路径已不存在（改名/移动走了），同一个 FRN 会成对
        // 出现 RENAME_NEW_NAME（目录整体移动时子目录内的文件不会逐条重放）。
        // 先试配对：找到新路径就把整棵子树搬过去，避免新路径只剩空目录壳；
        // 配对不上（纯删除语义的 rename_old）再走删除分支——删除前 stat 会失败
        // （路径已消失），节点还在树上，直接按缓存节点移除即可。
        let rename_old = rc.reason & USN_REASON_RENAME_OLD_NAME != 0;
        if rename_old {
            if try_rename_move(tree, resolved, root, &rel, &parent_rel, rc.frn) {
                applied += 1;
            } else {
                // 需要删除但路径已不在：用缓存节点的真实 size/count 回滚聚合。
                let hint = node_size_at(tree, &rel);
                if remove_node(tree, &rel, hint) {
                    applied += 1;
                }
            }
            continue;
        }

        let deleted = rc.reason & USN_REASON_FILE_DELETE != 0;
        if deleted {
            // 删除前 stat 一次拿真实大小：节点在缓存里（含 below top-K 未物化的）
            // 都要把父目录聚合回滚掉，否则 size/file_count 永久虚高。
            let size_hint = std::fs::metadata(&rc.path).ok().map(|m| m.len());
            if remove_node(tree, &rel, size_hint) {
                applied += 1;
            }
            continue;
        }
        // Not a delete: the file may still exist (size/data change, create,
        // rename-new-name). Stat it to refresh size; a vanished path is a
        // no-op (the FRN table is a snapshot, the file may be gone by now).
        let md = match std::fs::metadata(&rc.path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        apply_upsert(
            tree,
            &rel,
            &parent_rel,
            md.is_dir(),
            md.len(),
            &rc.path,
            keep_files,
        );
        applied += 1;
    }
    applied
}

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
