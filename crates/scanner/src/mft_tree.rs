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
use crate::Node;

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
    F: FnMut(u64, u64) -> bool, // (records_seen, bytes_seen) → false = 调用方要求中止
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

        if record_num.is_multiple_of(PROGRESS_EVERY) && !on_progress(record_num, bytes_total) {
            return Err(anyhow!(
                "scan:cancelled: mft slow parse at record {record_num}/{total_records}"
            ));
        }
    }
    if !on_progress(total_records, bytes_total) {
        return Err(anyhow!("scan:cancelled: mft slow parse final"));
    }

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

mod build_node;
pub(crate) use build_node::build_node;
