//! 单个 MFT 条目 → Node 节点：build_node（含扩展名统计 / 文件 top-K /
//! 目录 max_dirs 截断）。从 mft_tree.rs 拆分（2026-10-12）。

use std::collections::HashMap;
use std::path::Path;

use crate::mft::Entry;
use crate::{is_pruned_system_dir_at, is_pruned_system_file, ExtShare, Node};
pub(crate) fn build_node(
    frn: u64,
    path: &Path,
    entries: &HashMap<u64, Entry>,
    sizes: &HashMap<u64, (u64, u64)>,
    keep_files: Option<usize>,
    max_dirs: Option<usize>,
) -> Node {
    let entry = entries.get(&frn);
    let (size, file_count) = sizes.get(&frn).copied().unwrap_or((0, 0));
    let name = match entry {
        Some(e) => e.name.clone(),
        None => path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string()),
    };
    let is_dir = entry.map(|e| e.is_dir).unwrap_or(true);

    let mut children_nodes: Vec<Node> = Vec::new();
    let mut children_truncated: Option<u64> = None;
    let mut ext_bytes: HashMap<String, u64> = HashMap::new();
    let mut ext_count: HashMap<String, u64> = HashMap::new();

    if let Some(e) = entry {
        // Sort children by size desc for nicer display.
        let mut kids: Vec<(u64, u64)> = e
            .children
            .iter()
            .filter(|&&c| c != frn)
            .map(|&c| (c, sizes.get(&c).map(|(b, _)| *b).unwrap_or(0)))
            .collect();
        kids.sort_by_key(|k| std::cmp::Reverse(k.1));

        // 对齐 walkdir 模式的树形态：目录全部保留（系统目录在下方剪枝），
        // 文件按大小取 top-K（keep_files）。此前这里用 depth 分级 breadth_cap
        // （200/80/25）把文件和目录混在一起截断——深层目录超过 25 个孩子时
        // 大部分子目录根本不在树里，前端无法展开查看。kids 已按大小降序，
        // 文件 top-K = 顺序取前 keep 个。
        let keep = keep_files.unwrap_or(usize::MAX);
        let mut files_kept = 0usize;
        for (cfrn, _) in kids {
            let centry = match entries.get(&cfrn) {
                Some(c) => c,
                None => continue,
            };
            if centry.is_dir
                && is_pruned_system_dir_at(
                    std::ffi::OsStr::new(&e.name),
                    std::ffi::OsStr::new(&centry.name),
                )
            {
                continue;
            }
            if !centry.is_dir && is_pruned_system_file(&centry.name) {
                continue;
            }
            let cpath = path.join(&centry.name);

            // 扩展名统计覆盖全部直系文件（不受 top-K 截断影响；此前被
            // breadth_cap 截过，大目录的 top_extensions 偏小）。
            if !centry.is_dir {
                let ext = std::path::Path::new(&centry.name)
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_else(|| "(none)".into());
                // get_mut 借用更新零复制；每个目录首次遇到某后缀才 clone 入桶
                if let Some(b) = ext_bytes.get_mut(&ext) {
                    *b += centry.own_size;
                } else {
                    ext_bytes.insert(ext.clone(), centry.own_size);
                }
                if let Some(c) = ext_count.get_mut(&ext) {
                    *c += 1;
                } else {
                    ext_count.insert(ext, 1);
                }
                if files_kept >= keep {
                    continue;
                }
                files_kept += 1;
            }

            let cnode = build_node(cfrn, &cpath, entries, sizes, keep_files, max_dirs);
            children_nodes.push(cnode);
        }
    }

    // 目录广度上限（max_dirs）与文件 top-K 对称：超过上限的层按 size 保留
    // 最大的 max_dirs 个子目录，并标记 children_truncated（截断前目录总数，
    // 与 walkdir 侧/深度 cap 同口径——前端徽标 = children_truncated - children）
    // 供 +N 按需取回（tree_subtree）。children_nodes 已按 size 降序（kids
    // 排序保证），retain 保序保留前 limit 个目录节点 = 保留最大的 limit 个
    // 目录；文件节点不受影响。这把整盘扫描树的内存钉在有界范围内。
    if let Some(limit) = max_dirs {
        let dir_count = children_nodes.iter().filter(|c| c.is_dir).count();
        if dir_count > limit {
            let mut dirs_seen = 0usize;
            children_nodes.retain(|c| {
                if c.is_dir {
                    dirs_seen += 1;
                    dirs_seen <= limit
                } else {
                    true
                }
            });
            children_truncated = Some(dir_count as u64);
        }
    }

    let mut top_extensions: Vec<ExtShare> = ext_bytes
        .into_iter()
        .map(|(ext, bytes)| ExtShare {
            ext: ext.clone(),
            bytes,
            count: ext_count.get(&ext).copied().unwrap_or(0),
        })
        .collect();
    top_extensions.sort_by_key(|e| std::cmp::Reverse(e.bytes));
    top_extensions.truncate(8);

    Node {
        name,
        path: path.to_string_lossy().to_string(),
        is_dir,
        size,
        file_count,
        children: children_nodes,
        scaffold_id: None,
        top_extensions,
        children_truncated,
        mtime: None,
    }
}
