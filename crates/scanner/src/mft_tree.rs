//! MFT 条目 → Node 树：link_children / rollup / build_node_tree /
//! scan_volume_slow / find_frn_for_path / build_node。
//!
//! 从 mft.rs 拆分（2026-10-12）。

use anyhow::{anyhow, Context};
use ntfs::structured_values::{NtfsFileName, NtfsFileNamespace};
use ntfs::{KnownNtfsFileRecordNumber, Ntfs, NtfsAttributeType};
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use crate::mft::{Entry, PROGRESS_EVERY};
use crate::{ExtShare, Node};

pub(crate) fn link_children(entries: &mut HashMap<u64, Entry>) {
    let frns: Vec<u64> = entries.keys().copied().collect();
    for frn in frns {
        let parent = entries.get(&frn).map(|e| e.parent_frn).unwrap_or(0);
        if parent != frn {
            if let Some(p) = entries.get_mut(&parent) {
                p.children.push(frn);
            }
        }
    }
}

/// DFS 汇总每个目录的总大小与文件数（快/慢路径共用，带环保护）。
pub(crate) fn rollup(
    frn: u64,
    entries: &HashMap<u64, Entry>,
    sizes: &mut HashMap<u64, (u64, u64)>,
) -> (u64, u64) {
    if let Some(c) = sizes.get(&frn) {
        return *c;
    }
    // Cycle guard.
    sizes.insert(frn, (0, 0));
    let mut total_bytes: u64 = 0;
    let mut total_files: u64 = 0;
    if let Some(e) = entries.get(&frn) {
        if !e.is_dir {
            total_bytes = e.own_size;
            total_files = 1;
        } else {
            for &c in &e.children {
                if c == frn {
                    continue;
                }
                if let Some(centry) = entries.get(&c) {
                    if centry.is_dir
                        && super::is_pruned_system_dir_at(
                            std::ffi::OsStr::new(&e.name),
                            std::ffi::OsStr::new(&centry.name),
                        )
                    {
                        continue;
                    }
                    if !centry.is_dir && super::is_pruned_system_file(&centry.name) {
                        continue;
                    }
                }
                let (b, n) = rollup(c, entries, sizes);
                total_bytes = total_bytes.saturating_add(b);
                total_files = total_files.saturating_add(n);
            }
        }
    }
    sizes.insert(frn, (total_bytes, total_files));
    (total_bytes, total_files)
}

/// 构建可见 Node 树（快/慢路径共用）。`bytes_total` 仅用于日志/统计。
/// 子路径在 $MFT 里逐组件匹配不到时返回 Err（绝不回退整卷），
/// 上层据此落到 walkdir 兜底。
pub(crate) fn build_node_tree(
    volume_letter: char,
    subroot: Option<&Path>,
    keep_files: Option<usize>,
    max_dirs: Option<usize>,
    entries: &HashMap<u64, Entry>,
    bytes_total: u64,
) -> anyhow::Result<Node> {
    let root_frn = KnownNtfsFileRecordNumber::RootDirectory as u64;
    let volume_root = format!("{}:\\", volume_letter.to_ascii_uppercase());

    let mut sizes: HashMap<u64, (u64, u64)> = HashMap::new();
    rollup(root_frn, entries, &mut sizes);

    let start_frn = if let Some(sub) = subroot {
        match find_frn_for_path(sub, root_frn, entries) {
            Some(frn) => frn,
            None => {
                return Err(anyhow::anyhow!(
                    "subroot not found in $MFT: {}",
                    sub.display()
                ))
            }
        }
    } else {
        root_frn
    };
    let start_path = if let Some(sub) = subroot {
        sub.to_path_buf()
    } else {
        PathBuf::from(&volume_root)
    };

    let node = build_node(
        start_frn,
        &start_path,
        entries,
        &sizes,
        keep_files,
        max_dirs,
    );
    let _ = bytes_total;
    Ok(node)
}

/// 慢路径（原实现）：逐条 `ntfs::file()` + seek。保留作快路径失败的回退。
#[allow(clippy::too_many_arguments)] // 8 个同步参数原子传递，抽 struct 反而割裂调用点
pub(crate) fn scan_volume_slow<R, F>(
    reader: &mut R,
    ntfs: &Ntfs,
    volume_letter: char,
    subroot: Option<&Path>,
    keep_files: Option<usize>,
    max_dirs: Option<usize>,
    record_size: usize,
    mut on_progress: F,
) -> anyhow::Result<Node>
where
    R: Read + Seek,
    F: FnMut(u64, u64), // (records_seen, bytes_seen)
{
    // 定位 $MFT，拿总记录数。
    let mft_file = ntfs.file(reader, KnownNtfsFileRecordNumber::MFT as u64)?;
    let mft_data = mft_file
        .data(reader, "")
        .ok_or_else(|| anyhow!("$MFT has no $DATA attribute"))?
        .context("reading $MFT $DATA")?;
    let mft_data_attribute = mft_data.to_attribute()?;
    let mft_data_value = mft_data_attribute.value(reader)?;
    let total_size = mft_data_value.len();
    let total_records = total_size / record_size as u64;
    let _ = mft_data_value;
    let _ = mft_data_attribute;
    let _ = mft_file;

    // Pass 1: collect all entries by FRN.
    let mut entries: HashMap<u64, Entry> = HashMap::with_capacity(total_records as usize);
    let mut bytes_total: u64 = 0;

    for record_num in 0..total_records {
        let f = match ntfs.file(reader, record_num) {
            Ok(f) => f,
            Err(_) => continue,
        };

        let mut best_name: Option<NtfsFileName> = None;
        let mut best_namespace_rank: u8 = 0;
        let mut size: u64 = 0;
        let is_dir = f.is_directory();

        let mut attrs_iter = f.attributes();
        while let Some(item) = attrs_iter.next(reader) {
            let item = match item {
                Ok(a) => a,
                Err(_) => continue,
            };
            let attr = match item.to_attribute() {
                Ok(a) => a,
                Err(_) => continue,
            };
            match attr.ty() {
                Ok(NtfsAttributeType::FileName) => {
                    if let Ok(name) = attr.structured_value::<_, NtfsFileName>(reader) {
                        let rank = match name.namespace() {
                            NtfsFileNamespace::Posix => 4,
                            NtfsFileNamespace::Win32 => 3,
                            NtfsFileNamespace::Win32AndDos => 2,
                            NtfsFileNamespace::Dos => 1,
                        };
                        if rank > best_namespace_rank {
                            best_namespace_rank = rank;
                            best_name = Some(name);
                        }
                    }
                }
                Ok(NtfsAttributeType::Data) => {
                    if size != 0 {
                        continue;
                    }
                    let unnamed = attr.name().map(|n| n.is_empty()).unwrap_or(true);
                    if !unnamed {
                        continue;
                    }
                    size = attr.value_length();
                }
                _ => {}
            }
        }

        if let Some(fname) = best_name {
            let parent_frn = fname.parent_directory_reference().file_record_number();
            let name = fname.name().to_string_lossy();
            let own_size = if is_dir { 0 } else { size };
            bytes_total = bytes_total.saturating_add(own_size);
            entries.insert(
                record_num,
                Entry {
                    name,
                    parent_frn,
                    own_size,
                    is_dir,
                    children: Vec::new(),
                },
            );
        }

        if record_num.is_multiple_of(PROGRESS_EVERY) {
            on_progress(record_num, bytes_total);
        }
    }
    on_progress(total_records, bytes_total);

    // 公共的 children 链接 + 树构建。
    link_children(&mut entries);
    build_node_tree(
        volume_letter,
        subroot,
        keep_files,
        max_dirs,
        &entries,
        bytes_total,
    )
}

pub(crate) fn find_frn_for_path(
    target: &Path,
    root_frn: u64,
    entries: &HashMap<u64, Entry>,
) -> Option<u64> {
    // Walk component-by-component from root looking for matching child names.
    let mut current = root_frn;
    let comps: Vec<String> = target
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    for c in comps {
        let e = entries.get(&current)?;
        let next = e.children.iter().copied().find(|frn| {
            entries
                .get(frn)
                .map(|x| x.name.eq_ignore_ascii_case(&c))
                .unwrap_or(false)
        })?;
        current = next;
    }
    Some(current)
}

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
                && super::is_pruned_system_dir_at(
                    std::ffi::OsStr::new(&e.name),
                    std::ffi::OsStr::new(&centry.name),
                )
            {
                continue;
            }
            if !centry.is_dir && super::is_pruned_system_file(&centry.name) {
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
