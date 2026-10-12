//! 扫描主逻辑：4 个 scan 入口 + build_tree + USN 增量扫描 + 路径采样。
//!
//! 从 lib.rs 拆分（2026-10-12）：本模块是扫描执行层，类型（Node/ScanOptions/
//! ScanStats 等）仍留在 lib.rs；lib.rs 经 `pub use scan_core::*` 重导出保持
//! `diskpilot_scanner::scan_xxx` / `sample_paths` 顶层路径不变。

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

#[cfg(windows)]
mod usn_scan;
#[cfg(windows)]
pub use usn_scan::scan_with_usn;

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

mod build_tree;
pub(crate) use build_tree::build_tree;
