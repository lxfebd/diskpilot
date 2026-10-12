//! 扫描树构建（从 scan_core.rs 拆分，2026-10-12）。
//! build_tree 把 walk 阶段收集的 DirAcc 逐目录汇总（ext_bytes 聚合 / 子目录
//! 递归 / 文件 top-K 截断 / max_dirs 有界截断）重建成 Node 树。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{DirAcc, Node};

pub(crate) fn build_tree(
    dir: &Path,
    accs: &HashMap<PathBuf, DirAcc>,
    subdirs: &HashMap<PathBuf, Vec<String>>,
    keep_files: Option<usize>,
    max_dirs: Option<usize>,
) -> Node {
    let acc = accs.get(dir);
    let size = acc.map(|a| a.size).unwrap_or(0);
    let file_count = acc.map(|a| a.file_count).unwrap_or(0);
    let top_extensions = acc
        .map(|a| {
            let mut v: Vec<_> = a
                .ext_bytes
                .iter()
                .map(|(k, &b)| crate::ExtShare {
                    ext: k.clone(),
                    bytes: b,
                    count: a.ext_count.get(k).copied().unwrap_or(0),
                })
                .collect();
            v.sort_by_key(|e| std::cmp::Reverse(e.bytes));
            v.truncate(8);
            v
        })
        .unwrap_or_default();

    let name = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.to_string_lossy().to_string());

    let mut children: Vec<Node> = Vec::new();
    let mut children_truncated: Option<u64> = None;

    // Subdirectories — names were captured during the walk (post system-trash
    // prune), so we no longer do a second read_dir pass over the disk here.
    // Order within a directory doesn't matter; we sort by size below anyway.
    // 目录广度上限（max_dirs）与文件 top-K 对称：超过上限的层按 size 保留
    // 最大的 max_dirs 个子目录，并标记 children_truncated 供前端 +N 按需
    // 取回（tree_subtree）。这把整盘扫描树的内存钉在有界范围内——几十万
    // 目录的盘不再全部驻留。
    if let Some(names) = subdirs.get(dir) {
        // 先递归构建全部子目录，再按 size 取 top-K。dirs_in_acc 里每个目录
        // 的 size 已在 walk 时汇总；子目录 Node 建出来后按 size 排序截断。
        let mut sub_nodes: Vec<Node> = Vec::with_capacity(names.len());
        for sub in names {
            sub_nodes.push(build_tree(
                &dir.join(sub),
                accs,
                subdirs,
                keep_files,
                max_dirs,
            ));
        }
        sub_nodes.sort_by_key(|c| std::cmp::Reverse(c.size));
        if let Some(limit) = max_dirs {
            if sub_nodes.len() > limit {
                children_truncated = Some(sub_nodes.len() as u64);
                sub_nodes.truncate(limit);
            }
        }
        children.extend(sub_nodes);
    }

    // Files — pull the largest from this dir's acc and emit as leaf nodes.
    // `files` is already a descending top-K list from push_file during the walk.
    if let Some(a) = acc {
        let limit = keep_files.unwrap_or(usize::MAX);
        for (fname, fsize, fmtime) in a.files.iter().take(limit) {
            let fpath = dir.join(fname);
            children.push(Node {
                name: fname.clone(),
                path: fpath.to_string_lossy().to_string(),
                is_dir: false,
                size: *fsize,
                file_count: 1,
                children: Vec::new(),
                scaffold_id: None,
                top_extensions: Vec::new(),
                children_truncated: None,
                mtime: *fmtime,
            });
        }
    }

    children.sort_by_key(|c| std::cmp::Reverse(c.size));

    Node {
        name,
        path: dir.to_string_lossy().to_string(),
        is_dir: true,
        size,
        file_count,
        children,
        scaffold_id: None,
        top_extensions,
        children_truncated,
        mtime: None,
    }
}
