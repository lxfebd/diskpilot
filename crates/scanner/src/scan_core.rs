//! 扫描主逻辑：4 个 scan 入口 + build_tree + USN 增量扫描 + 路径采样。
//!
//! 从 lib.rs 拆分（2026-10-12）：本模块是扫描执行层，类型（Node/ScanOptions/
//! ScanStats 等）仍留在 lib.rs；lib.rs 经 `pub use scan_core::*` 重导出保持
//! `diskpilot_scanner::scan_xxx` / `sample_paths` 顶层路径不变。

use anyhow::Context;
use jwalk::WalkDir as JWalk;
use rayon::prelude::*;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::{is_pruned_system_file, DirAcc, Node, ScanOptions, ScanProgress, ScanStats};

pub fn scan<P: AsRef<Path>>(root: P) -> anyhow::Result<Node> {
    scan_with(root, ScanOptions::default(), |_| {})
}

pub fn scan_with<P, F>(root: P, opts: ScanOptions, on_progress: F) -> anyhow::Result<Node>
where
    P: AsRef<Path>,
    F: Fn(&ScanProgress) + Send + Sync,
{
    scan_with_stats(root, opts, on_progress).map(|(n, _)| n)
}

/// Same as `scan_with`, but also returns phase-level timings. Internal API for
/// the desktop app's diagnostics bar — keeps `scan` / `scan_with` unchanged.
pub fn scan_with_stats<P, F>(
    root: P,
    opts: ScanOptions,
    on_progress: F,
) -> anyhow::Result<(Node, ScanStats)>
where
    P: AsRef<Path>,
    F: Fn(&ScanProgress) + Send + Sync,
{
    scan_with_stats_cancellable(root, opts, None, on_progress)
}

/// Like `scan_with_stats`, but aborts (with a "cancelled" error) as soon as
/// `cancel` flips true. `None` disables cancellation. The check runs every
/// 5000 files inside the walk loop and once before the tree build, so a long
/// scan can be interrupted from another thread instead of blocking the caller
/// until the whole volume is walked.
pub fn scan_with_stats_cancellable<P, F>(
    root: P,
    opts: ScanOptions,
    cancel: Option<&AtomicBool>,
    on_progress: F,
) -> anyhow::Result<(Node, ScanStats)>
where
    P: AsRef<Path>,
    F: Fn(&ScanProgress) + Send + Sync,
{
    let root = root.as_ref().to_path_buf();
    let total_t0 = Instant::now();
    let mut stats = ScanStats::default();
    tracing::info!("scan: start root={:?}", root);

    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(anyhow::anyhow!("scan:cancelled: before start"));
    }

    // Try the MFT fast path on Windows when the root is on an NTFS volume.
    #[cfg(windows)]
    {
        if let Some(letter) = drive_letter_of(&root) {
            let subroot = if is_drive_root(&root) {
                None
            } else {
                Some(root.as_path())
            };
            let progress = &on_progress;
            stats.mft_attempted = true;
            let mft_t0 = Instant::now();
            // The `ntfs` crate can panic on non-NTFS volumes (e.g. CI runners,
            // ReFS, removable media) instead of returning an Err. Catch it so
            // we always fall back to walkdir cleanly. AssertUnwindSafe is OK
            // because we don't observe partial state on panic.
            let mft_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::mft::scan_volume(
                    letter,
                    subroot,
                    opts.keep_files_per_dir,
                    opts.max_dirs_per_level,
                    |records, bytes| {
                        progress(&ScanProgress {
                            files_seen: records,
                            bytes_seen: bytes,
                            current_path: format!("MFT record {}", records),
                        });
                    },
                )
            }))
            .unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "MFT scan panicked (likely non-NTFS volume)"
                ))
            });
            // Honour an interrupt request even on the fast MFT path.
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Err(anyhow::anyhow!("scan:cancelled: during MFT pass"));
            }
            match mft_result {
                // 空树守卫：ntfs 0.4 在某些环境（BitLocker / 权限受限 / 非标准
                // NTFS）下会“成功”返回但 entries 全空，直接返回会让前端看到
                // 0 B · 0 个文件的假成功。只有确有数据才走 MFT 结果，否则
                // 记录警告并落入 walkdir 兜底。
                Ok(n) if n.file_count > 0 || n.size > 0 => {
                    stats.mft_ms = mft_t0.elapsed().as_millis() as u64;
                    stats.mft_succeeded = true;
                    stats.mode = "mft".into();
                    stats.files_seen = n.file_count;
                    stats.bytes_seen = n.size;
                    stats.total_ms = total_t0.elapsed().as_millis() as u64;
                    tracing::info!(
                        "scan: mode=mft mft_ms={} total_ms={} files={} bytes={}",
                        stats.mft_ms,
                        stats.total_ms,
                        stats.files_seen,
                        stats.bytes_seen,
                    );
                    progress(&ScanProgress {
                        files_seen: n.file_count,
                        bytes_seen: n.size,
                        current_path: "done (mft)".into(),
                    });
                    return Ok((n, stats));
                }
                Ok(_empty) => {
                    stats.mft_ms = mft_t0.elapsed().as_millis() as u64;
                    tracing::warn!(
                        "MFT scan returned an empty tree (files=0, bytes=0) after {} ms, falling back to walkdir",
                        stats.mft_ms
                    );
                }
                Err(e) => {
                    stats.mft_ms = mft_t0.elapsed().as_millis() as u64;
                    tracing::warn!(
                        "MFT scan failed after {} ms, falling back to walkdir: {e:#}",
                        stats.mft_ms
                    );
                }
            }
        }
    }

    stats.mode = "walkdir".into();
    let files_seen = Arc::new(AtomicU64::new(0));
    let bytes_seen = Arc::new(AtomicU64::new(0));
    let last_emit = Arc::new(AtomicU64::new(0));
    // Shared wall-clock for the 500ms progress throttle; the parallel consume
    // loop below touches it from multiple rayon threads.
    let last_emit_at = Arc::new(Mutex::new(Instant::now()));

    // Phase 1: parallel walk, collect (path, size) pairs for every file.
    // Also capture each directory's direct child-dir names here (after the
    // system-trash prune) so build_tree can reconstruct the tree from memory
    // instead of doing a second full read_dir pass over the disk.
    //
    // 并发双保险：绝不使用 jwalk 的 RayonDefaultPool（共享全局 rayon 池）。
    // 它带有 busy_timeout 探活——多个并行 walk 占满全局池时，ReadDirIter::new
    // 的 startup 探活超时后会把整个迭代器静默变成零项（files=0 且 dirs_in_acc=0，
    // 磁盘数据全丢，8 盘并行扫描时 8 次全空）。给每个 walk 一个独立线程池
    // （RayonNewPool），用尽机器核的同时互不干扰；`par_bridge` 消费仍照常。
    let subdirs: Arc<std::sync::Mutex<HashMap<PathBuf, Vec<String>>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    let subdirs_in_walk = subdirs.clone();
    let mut walker = JWalk::new(&root)
        .skip_hidden(false)
        .follow_links(opts.follow_symlinks)
        .parallelism(jwalk::Parallelism::RayonNewPool(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
        ))
        .process_read_dir(move |_, dir, _, children| {
            children.retain(|res| {
                let Ok(entry) = res else { return true };
                if !entry.file_type.is_dir() {
                    return true;
                }
                // parent 名字取 `dir` 的末段（如 `Windows`），供 installer 判断用
                let parent_name = dir.file_name().unwrap_or(dir.as_os_str());
                !crate::is_pruned_system_dir_at(parent_name, &entry.file_name)
            });
            let mut map = subdirs_in_walk.lock().unwrap();
            let list = map.entry(dir.to_path_buf()).or_default();
            for res in children.iter().filter_map(|r| r.as_ref().ok()) {
                if res.file_type.is_dir() {
                    list.push(res.file_name.to_string_lossy().to_string());
                }
            }
        });
    if let Some(d) = opts.max_depth {
        walker = walker.max_depth(d);
    }

    let walk_t0 = Instant::now();
    let cap = opts.keep_files_per_dir;

    // Phase 1b: consume the parallel walker's file entries and accumulate into
    // a per-thread shard map, then merge shards at the end (plan #12). The old
    // single-threaded consume loop was the CPU bottleneck on weak cores —
    // jwalk already walks in parallel, so the tally should too. Cancellation,
    // progress throttling and system-file pruning behave exactly as before.
    let shards: Vec<Result<HashMap<PathBuf, DirAcc>, ()>> = walker
        .into_iter()
        .par_bridge()
        .try_fold(HashMap::new, |mut acc: HashMap<PathBuf, DirAcc>, entry| {
            // A cancelled scan short-circuits the whole pipeline: rayon's
            // try_fold stops pulling further items once a shard errors, so
            // the jwalk iterator is dropped early and the walk truly ends.
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Err(());
            }
            let Ok(entry) = entry else {
                return Ok(acc);
            };
            if !entry.file_type().is_file() {
                return Ok(acc);
            }
            let path = entry.path();
            let m = entry.metadata();
            let size = m.as_ref().map(|m| m.len()).unwrap_or(0);
            // mtime stays None for the MFT path / unreadable metadata; the
            // same metadata call that yields size provides it at zero extra IO.
            let mtime = m
                .as_ref()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| "(none)".into());
            let file_name = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            // Skip pagefile/hiberfil/swapfile — multi-GB system files that are
            // neither cleanable nor worth showing as "reclaimable".
            if is_pruned_system_file(&file_name) {
                return Ok(acc);
            }

            // attribute to immediate parent (with files list) and walk upward (totals only)
            if let Some(parent) = path.parent() {
                let parent_acc = acc.entry(parent.to_path_buf()).or_insert_with(|| DirAcc {
                    files_cap: cap,
                    ..Default::default()
                });
                parent_acc.push_file(file_name, size, mtime);
            }
            // 热路径去克隆：祖先链每层都要按 path 查 DirAcc。绝大对数文件
            // 的祖先目录早已建好（同目录文件共享），先 get_mut 快路径零拷贝；
            // 只有首次遇到某目录才 clone PathBuf 入桶（每目录仅一次）。
            let mut cur = path.parent();
            while let Some(dir) = cur {
                if let Some(dir_acc) = acc.get_mut(dir) {
                    dir_acc.size += size;
                    dir_acc.file_count += 1;
                    // ext 热路径同款：get_mut 借用更新零复制，仅首遇某后缀 clone。
                    if let Some(b) = dir_acc.ext_bytes.get_mut(&ext) {
                        *b += size;
                    } else {
                        dir_acc.ext_bytes.insert(ext.clone(), size);
                    }
                    if let Some(c) = dir_acc.ext_count.get_mut(&ext) {
                        *c += 1;
                    } else {
                        dir_acc.ext_count.insert(ext.clone(), 1);
                    }
                } else {
                    let mut dir_acc = DirAcc {
                        files_cap: cap,
                        ..Default::default()
                    };
                    dir_acc.size = size;
                    dir_acc.file_count = 1;
                    dir_acc.ext_bytes.insert(ext.clone(), size);
                    dir_acc.ext_count.insert(ext.clone(), 1);
                    acc.insert(dir.to_path_buf(), dir_acc);
                }
                if dir == root || !dir.starts_with(&root) {
                    break;
                }
                cur = dir.parent();
            }

            let total_files = files_seen.fetch_add(1, Ordering::Relaxed) + 1;
            bytes_seen.fetch_add(size, Ordering::Relaxed);
            // Throttle progress to ~every 5k files (or 500ms of wall time) to avoid
            // IPC saturation while still looking alive on slow/large dirs.
            {
                let mut last_at = last_emit_at.lock().unwrap();
                if total_files.wrapping_sub(last_emit.load(Ordering::Relaxed)) >= 5000
                    || last_at.elapsed().as_millis() >= 500
                {
                    last_emit.store(total_files, Ordering::Relaxed);
                    *last_at = Instant::now();
                    on_progress(&ScanProgress {
                        files_seen: total_files,
                        bytes_seen: bytes_seen.load(Ordering::Relaxed),
                        current_path: path.to_string_lossy().to_string(),
                    });
                }
            }
            Ok(acc)
        })
        .collect();

    let mut accs: HashMap<PathBuf, DirAcc> = HashMap::new();
    for shard in shards {
        let shard = shard.map_err(|_| anyhow::anyhow!("scan:cancelled: during walk"))?;
        for (dir, shard_acc) in shard {
            match accs.entry(dir) {
                Entry::Occupied(mut e) => e.get_mut().merge(shard_acc),
                Entry::Vacant(e) => {
                    e.insert(shard_acc);
                }
            }
        }
    }

    stats.walk_ms = walk_t0.elapsed().as_millis() as u64;
    stats.files_seen = files_seen.load(Ordering::Relaxed);
    stats.bytes_seen = bytes_seen.load(Ordering::Relaxed);
    stats.dirs_in_acc = accs.len() as u64;
    tracing::info!(
        "scan: walk done walk_ms={} files={} bytes={} dirs_in_acc={}",
        stats.walk_ms,
        stats.files_seen,
        stats.bytes_seen,
        stats.dirs_in_acc,
    );

    on_progress(&ScanProgress {
        files_seen: stats.files_seen,
        bytes_seen: stats.bytes_seen,
        current_path: "done".into(),
    });

    let build_t0 = Instant::now();
    // walker 已被 into_iter 消费、collect 返回时迭代器链（连同其 Arc 克隆）
    // 都已 drop，Arc::try_unwrap 通常能零拷贝拿走整个 map；极端情况下
    // （仍有残留持有者）退回 lock-and-clone，行为与旧版一致。
    let subdirs: HashMap<PathBuf, Vec<String>> = match Arc::try_unwrap(subdirs) {
        Ok(m) => m.into_inner().unwrap(),
        Err(arc) => arc.lock().unwrap().clone(),
    };
    let tree = build_tree(
        &root,
        &accs,
        &subdirs,
        opts.keep_files_per_dir,
        opts.max_dirs_per_level,
    );
    stats.build_tree_ms = build_t0.elapsed().as_millis() as u64;
    stats.total_ms = total_t0.elapsed().as_millis() as u64;
    tracing::info!(
        "scan: mode=walkdir walk_ms={} build_tree_ms={} total_ms={} dirs_in_acc={}",
        stats.walk_ms,
        stats.build_tree_ms,
        stats.total_ms,
        stats.dirs_in_acc,
    );
    Ok((tree, stats))
}

fn build_tree(
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

/// USN Journal 增量扫描入口（Windows / NTFS）。
///
/// 相比全量 walkdir/MFT 扫描，增量路径只回放 Change Journal 里的变更记录
/// （create/delete/rename/data change），再只对变更文件做一次 stat 补大小。
/// 对基本空闲的卷，重新扫描从分钟级降到亚秒级。
///
/// - `cache`：上一次全量扫描得到的树（调用方持久化）。
/// - `cursor`：上一次增量后的游标（journal_id + next_usn）。
///
/// 返回 `(新树, 新游标)`。下列情况调用方应回退到全量扫描：
///   1. 卷不是 NTFS（`query_journal` 失败 / journal 未启用）。
///   2. journal 被重置或已回绕（`cursor_still_valid()` 为 false）。
#[cfg(windows)]
pub fn scan_with_usn(
    root: &Path,
    mut cache: Node,
    cursor: &crate::usn::UsnCursor,
    cancel: Option<&AtomicBool>,
    on_progress: &(dyn Fn(&ScanProgress) + Sync),
) -> anyhow::Result<(Node, crate::usn::UsnCursor)> {
    let letter = drive_letter_of(root)
        .ok_or_else(|| anyhow::anyhow!("scan:usn: root 不在盘根上 (root={})", root.display()))?;
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(anyhow::anyhow!("scan:cancelled: before usn"));
    }

    // 1. 卷上 journal 的当前状态；非 NTFS / journal 未启用 → 错误（调用方回退全量）。
    let state = crate::usn::win::query_journal(letter)
        .with_context(|| format!("USN journal unavailable on {letter}:"))?;
    if !state.cursor_still_valid(cursor) {
        return Err(anyhow::anyhow!(
            "scan:usn: journal reset or rolled over (id {} vs {}, next_usn {} < lowest {}); full re-scan required",
            cursor.journal_id, state.journal_id, cursor.next_usn, state.lowest_valid_usn
        ));
    }

    // 2. 全卷 FRN→(parent,name) 表（MFT 速度枚举，用于把变更解析成绝对路径）。
    let mut frn_table = crate::usn::win::build_frn_table(letter, cancel, |seen, _| {
        on_progress(&ScanProgress {
            files_seen: seen,
            bytes_seen: 0,
            current_path: "USN 枚举 FRN 表…".into(),
        });
    })?;

    // 3. 回放 journal 自 cursor 起的变更。
    let (changes, next_usn) = crate::usn::win::read_changes(letter, cursor)
        .context("scan:usn: reading change journal")?;
    let new_cursor = crate::usn::UsnCursor {
        journal_id: state.journal_id,
        next_usn,
    };

    // 4. 解析成绝对路径（只保留在 root 子树内的）。
    let resolved = crate::usn::resolve_paths(&changes, &frn_table, root);
    let _ = &mut frn_table;

    // 5. 变更合入缓存树（delta 传播聚合，只 stat 变更文件）。
    let applied = crate::usn::merge_changes(
        &mut cache,
        &resolved,
        root,
        crate::ScanOptions::default().keep_files_per_dir,
    );

    on_progress(&ScanProgress {
        files_seen: applied as u64,
        bytes_seen: 0,
        current_path: "USN 增量合并完成".into(),
    });

    tracing::info!(
        "scan:usn: applied={applied} changes={} root={}",
        changes.len(),
        root.display()
    );
    Ok((cache, new_cursor))
}

#[cfg(windows)]
fn drive_letter_of(p: &Path) -> Option<char> {
    let s = p.to_string_lossy();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        let c = bytes[0] as char;
        if c.is_ascii_alphabetic() {
            return Some(c);
        }
    }
    None
}

#[cfg(windows)]
fn is_drive_root(p: &Path) -> bool {
    let s = p.to_string_lossy();
    matches!(s.as_ref(), "C:" | "D:" | "E:" | "F:" | "G:" | "H:")
        || (s.len() == 3
            && s.as_bytes()[1] == b':'
            && (s.as_bytes()[2] == b'\\' || s.as_bytes()[2] == b'/'))
}

/// Pull up to `n` sample paths from a directory, ordered shallowest-first.
pub fn sample_paths<P: AsRef<Path>>(root: P, n: usize) -> Vec<String> {
    let root = root.as_ref();
    walkdir::WalkDir::new(root)
        .max_depth(3)
        .into_iter()
        .filter_entry(|e| {
            if !e.file_type().is_dir() {
                return true;
            }
            let parent_name = e
                .path()
                .parent()
                .and_then(|p| p.file_name())
                .unwrap_or_else(|| e.path().as_os_str());
            !crate::is_pruned_system_dir_at(parent_name, e.file_name())
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .take(n)
        .map(|e| e.path().to_string_lossy().to_string())
        .collect()
}
