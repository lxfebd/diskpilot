//! Cross-platform disk scanner. Returns a tree of directories with size/file_count.
//!
//! v0.1.1: parallel walk via jwalk (rayon under the hood) + per-leaf file
//! children + progress callback. v0.2 will swap in direct NTFS MFT read on
//! Windows for sub-3s C: drive scans.

use anyhow::Context;
use jwalk::WalkDir as JWalk;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// 盘根下的系统交换/休眠文件（lowercase），占几十 GB 但不可删除。
/// 只在文件收集阶段跳过，避免污染"可回收潜力"统计。
const PRUNED_SYSTEM_FILES: &[&str] = &["hiberfil.sys", "pagefile.sys", "swapfile.sys"];

fn is_pruned_system_file(name: &str) -> bool {
    PRUNED_SYSTEM_FILES.contains(&name)
}

#[cfg(windows)]
mod mft;
/// bench feature 下把 mft 内部结构暴露给冒烟 CLI（仅 bench::run 一个入口）。
#[cfg(all(windows, feature = "bench"))]
pub mod mft_bench {
    pub use super::mft::bench;
}
/// USN Journal 增量扫描（Windows）：回放变更日志 + 只重扫变更文件。
/// 解析层跨平台纯函数，DeviceIoControl 层仅在 Windows 编译。
#[cfg(windows)]
pub mod usn;
/// 路径剪枝 / glob 匹配 / 目录 scope 统计原语。desktop 的 scaffold 扫描
/// 与 scanner 的主扫共用同一份 PRUNED_SYSTEM_DIRS 黑名单与 walker 策略，
/// 避免两份不一致的剪枝名单再次漂移。
mod walker;
pub use walker::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub file_count: u64,
    pub children: Vec<Node>,
    #[serde(default)]
    pub scaffold_id: Option<String>,
    #[serde(default)]
    pub top_extensions: Vec<ExtShare>,
    /// 目录被 `tag_and_truncate` 截断前的原始子项数；`Some(n)` 表示该层
    /// children 已被砍到 cap，前端展开时应从后端内存树按需取回完整子项。
    /// 文件或未截断目录为 `None`。
    #[serde(default)]
    pub children_truncated: Option<u64>,
    /// 最后修改时间（unix 秒，`metadata.modified()`）。`Some` 表示该节点
    /// 携带真实时间戳，供树上做「N 天前」过滤；`None` 表示来源未提供
    /// （如 MFT 分支尚未落该字段）。`#[serde(default)]` 保证旧序列化
    /// 缓存反序列化不炸。
    #[serde(default)]
    pub mtime: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtShare {
    pub ext: String,
    pub bytes: u64,
    pub count: u64,
}

#[derive(Debug, Default)]
struct DirAcc {
    size: u64,
    file_count: u64,
    ext_bytes: HashMap<String, u64>,
    ext_count: HashMap<String, u64>,
    /// (file name, size, mtime-unix-sec) — top-K kept on the immediate parent.
    /// Bounded during the walk so a single huge directory (e.g. node_modules,
    /// an unpacked dataset) doesn't grow `files` unbounded before build_tree
    /// prunes it. `mtime` is captured from the same metadata call that yields
    /// `size` (zero extra IO); `None` when the source didn't provide one
    /// (MFT path / unreadable).
    files: Vec<(String, u64, Option<u64>)>,
    /// Max entries kept in `files`; `None` = keep everything (memory hog).
    files_cap: Option<usize>,
}

impl DirAcc {
    /// Push a file into the bounded top-K slot. Only the largest `cap` files
    /// survive, in descending order, so `build_tree` doesn't need to re-sort.
    fn push_file(&mut self, name: String, size: u64, mtime: Option<u64>) {
        let Some(cap) = self.files_cap else {
            self.files.push((name, size, mtime));
            return;
        };
        if cap == 0 {
            return;
        }
        if self.files.len() < cap {
            self.files.push((name, size, mtime));
            self.files.sort_by_key(|f| std::cmp::Reverse(f.1));
            return;
        }
        // Full: only replace the smallest when the new file is bigger.
        if let Some(last) = self.files.last() {
            if size > last.1 {
                self.files.pop();
                self.files.push((name, size, mtime));
                self.files.sort_by_key(|f| std::cmp::Reverse(f.1));
            }
        }
    }

    /// Fold another per-thread accumulator into `self`. Used at the end of the
    /// parallel walk to merge each rayon shard's private `HashMap` into the
    /// final one. Totals add; the bounded top-K file list re-runs `push_file`
    /// so the merged result stays exactly the largest `cap` files.
    fn merge(&mut self, other: DirAcc) {
        self.size = self.size.saturating_add(other.size);
        self.file_count = self.file_count.saturating_add(other.file_count);
        // entry API 一次哈希查找完成累加，别先 entry 再 get 二次查找
        for (k, v) in other.ext_bytes {
            let e = self.ext_bytes.entry(k).or_insert(0);
            *e = e.saturating_add(v);
        }
        for (k, v) in other.ext_count {
            let e = self.ext_count.entry(k).or_insert(0);
            *e = e.saturating_add(v);
        }
        for (name, size, mtime) in other.files {
            self.push_file(name, size, mtime);
        }
    }
}

pub struct ScanOptions {
    pub follow_symlinks: bool,
    pub max_depth: Option<usize>,
    /// How many files to keep per directory in the returned tree. None = all (memory hog on large dirs).
    pub keep_files_per_dir: Option<usize>,
    /// How many subdirectories to keep per directory in the returned tree.
    /// Symmetric to `keep_files_per_dir`: bounds the tree (and the in-memory
    /// `scan_tree` cache) on volumes with hundreds of thousands of dirs,
    /// where keeping every directory node with its full path String would be
    /// a memory hog. None = all.
    pub max_dirs_per_level: Option<usize>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            follow_symlinks: false,
            max_depth: None,
            keep_files_per_dir: Some(500),
            max_dirs_per_level: Some(200),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanProgress {
    pub files_seen: u64,
    pub bytes_seen: u64,
    pub current_path: String,
}

/// Phase-level timings for a scan. Diagnostic only — emit via the Tauri command
/// alongside the tree so the UI / packaged binary can show "where the time went"
/// without needing RUST_LOG=info.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanStats {
    pub mode: String, // "mft" | "walkdir"
    pub mft_attempted: bool,
    pub mft_succeeded: bool,
    pub mft_ms: u64,  // total time spent in the MFT branch (success or fallback)
    pub walk_ms: u64, // jwalk consume loop (only set in walkdir mode)
    pub build_tree_ms: u64, // build_tree recursion + 2nd read_dir pass
    pub total_ms: u64,
    pub files_seen: u64,
    pub bytes_seen: u64,
    pub dirs_in_acc: u64, // accs.len() — proxy for memory pressure (walkdir mode only)
}

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
                mft::scan_volume(
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
                !is_pruned_system_dir_at(parent_name, &entry.file_name)
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
                .map(|(k, &b)| ExtShare {
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
            sub_nodes.push(build_tree(&dir.join(sub), accs, subdirs, keep_files, max_dirs));
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
    cursor: &usn::UsnCursor,
    cancel: Option<&AtomicBool>,
    on_progress: &(dyn Fn(&ScanProgress) + Sync),
) -> anyhow::Result<(Node, usn::UsnCursor)> {
    let letter = drive_letter_of(root)
        .ok_or_else(|| anyhow::anyhow!("scan:usn: root 不在盘根上 (root={})", root.display()))?;
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(anyhow::anyhow!("scan:cancelled: before usn"));
    }

    // 1. 卷上 journal 的当前状态；非 NTFS / journal 未启用 → 错误（调用方回退全量）。
    let state = usn::win::query_journal(letter)
        .with_context(|| format!("USN journal unavailable on {letter}:"))?;
    if !state.cursor_still_valid(cursor) {
        return Err(anyhow::anyhow!(
            "scan:usn: journal reset or rolled over (id {} vs {}, next_usn {} < lowest {}); full re-scan required",
            cursor.journal_id, state.journal_id, cursor.next_usn, state.lowest_valid_usn
        ));
    }

    // 2. 全卷 FRN→(parent,name) 表（MFT 速度枚举，用于把变更解析成绝对路径）。
    let mut frn_table = usn::win::build_frn_table(letter, cancel, |seen, _| {
        on_progress(&ScanProgress {
            files_seen: seen,
            bytes_seen: 0,
            current_path: "USN 枚举 FRN 表…".into(),
        });
    })?;

    // 3. 回放 journal 自 cursor 起的变更。
    let (changes, next_usn) =
        usn::win::read_changes(letter, cursor).context("scan:usn: reading change journal")?;
    let new_cursor = usn::UsnCursor {
        journal_id: state.journal_id,
        next_usn,
    };

    // 4. 解析成绝对路径（只保留在 root 子树内的）。
    let resolved = usn::resolve_paths(&changes, &frn_table, root);
    let _ = &mut frn_table;

    // 5. 变更合入缓存树（delta 传播聚合，只 stat 变更文件）。
    let applied = usn::merge_changes(
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
            !is_pruned_system_dir_at(parent_name, e.file_name())
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .take(n)
        .map(|e| e.path().to_string_lossy().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scans_temp_dir() {
        let dir = tempdir_path();
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::write(dir.join("a/file1.txt"), b"hello").unwrap();
        fs::write(dir.join("a/b/file2.txt"), b"world!").unwrap();

        let node = scan(&dir).unwrap();
        assert_eq!(node.size, 11);
        assert_eq!(node.file_count, 2);
        // file leaves should appear as children of their directory
        let a = node.children.iter().find(|c| c.name == "a").unwrap();
        assert!(a
            .children
            .iter()
            .any(|c| !c.is_dir && c.name == "file1.txt"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_prunes_system_trash_dirs() {
        let dir = tempdir_path();
        fs::create_dir_all(dir.join("normal")).unwrap();
        fs::write(dir.join("normal/keep.txt"), b"x").unwrap();
        for trashy in &[
            "$RECYCLE.BIN",
            "System Volume Information",
            ".Trash",
            ".Trashes",
        ] {
            let d = dir.join(trashy);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("inside.txt"), b"x").unwrap();
        }

        let node = scan(&dir).unwrap();

        let names: Vec<String> = node.children.iter().map(|c| c.name.clone()).collect();
        assert!(names.contains(&"normal".to_string()), "got: {names:?}");
        for trashy in &[
            "$RECYCLE.BIN",
            "System Volume Information",
            ".Trash",
            ".Trashes",
        ] {
            assert!(
                !names.iter().any(|n| n.eq_ignore_ascii_case(trashy)),
                "scanner leaked `{trashy}` into Node tree, names: {names:?}"
            );
        }
        assert_eq!(node.file_count, 1, "only `normal/keep.txt` should count");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_scans_each_return_real_files() {
        // 回归测试：多个并行 scan 共享同一 rayon 默认池时会互相挤占，
        // jwalk 的 busy_timeout 探活超时后把整个迭代器静默变成零项
        // （files=0 dirs_in_acc=0，磁盘数据全丢）。用多个独立临时目录
        // 并发扫描验证：修复后每次扫描都必须返回真实文件数。
        let dirs: Vec<PathBuf> = (0..6)
            .map(|i| {
                let d = tempdir_path();
                // 每目录 6 个深层子目录 × 800 文件 ≈ 4800 文件/路。
                // 规模要让 6 路并行的 read_dir 任务在默认池里排起队，
                // 探活超时才会真实暴露（太小秒扫完体现不出队列压力）。
                for k in 0..6 {
                    let sd = d.join(format!("sub{k}"));
                    fs::create_dir_all(sd.join("nested")).unwrap();
                    for j in 0..400u32 {
                        fs::write(sd.join(format!("f{j}.txt")), vec![b'x'; 10]).unwrap();
                        fs::write(sd.join("nested").join(format!("g{j}.bin")), vec![b'x'; 10])
                            .unwrap();
                    }
                }
                let _ = i;
                d
            })
            .collect();
        let handles: Vec<_> = dirs
            .iter()
            .map(|d| {
                let d = d.clone();
                std::thread::spawn(move || {
                    let (node, stats) =
                        scan_with_stats(&d, ScanOptions::default(), |_| {}).unwrap();
                    (node, stats)
                })
            })
            .collect();
        for (d, h) in dirs.iter().zip(handles) {
            let (node, stats) = h.join().unwrap();
            assert_eq!(
                stats.files_seen,
                4800,
                "{} 并发扫描丢数据（本次 fs={}）",
                d.display(),
                stats.files_seen
            );
            assert_eq!(
                node.file_count, 4800,
                "树文件数应为 4800，实得 {}",
                node.file_count
            );
            let _ = fs::remove_dir_all(d);
        }
    }

    #[test]
    fn scan_ext_tally_is_exact_across_deep_dirs_and_mixed_exts() {
        // 回归测试：ext 统计热路径改为 get_mut+insert（未命中才 clone）后，
        // 深层目录 + 多后缀混合下 top_extensions 的字节/计数必须与文件实际一致。
        let dir = tempdir_path();
        fs::create_dir_all(dir.join("a/b/c")).unwrap();
        // 3 个 .txt：根 1（10B）+ a/b/c 2（20B+30B）= 60B / 3 个
        fs::write(dir.join("root.txt"), vec![b'x'; 10]).unwrap();
        fs::write(dir.join("a/b/c/t1.txt"), vec![b'x'; 20]).unwrap();
        fs::write(dir.join("a/b/c/t2.txt"), vec![b'x'; 30]).unwrap();
        // 2 个 .bin：a 1（100B）+ a/b/c 1（40B）= 140B / 2 个
        fs::write(dir.join("a/data.bin"), vec![b'x'; 100]).unwrap();
        fs::write(dir.join("a/b/c/f.bin"), vec![b'x'; 40]).unwrap();
        // 1 个 .log：根（5B）
        fs::write(dir.join("app.log"), vec![b'x'; 5]).unwrap();

        let node = scan(&dir).unwrap();
        assert_eq!(node.size, 205, "总字节 = 10+20+30+100+40+5");
        assert_eq!(node.file_count, 6);

        // 根目录的 top_extensions 覆盖全部 5 个文件（都在子树里）
        let exts: Vec<&ExtShare> = node.top_extensions.iter().collect();
        let txt = exts.iter().find(|e| e.ext == "txt").expect("应有 txt");
        assert_eq!(txt.bytes, 60, ".txt 总字节");
        assert_eq!(txt.count, 3, ".txt 文件数");
        let bin = exts.iter().find(|e| e.ext == "bin").expect("应有 bin");
        assert_eq!(bin.bytes, 140, ".bin 总字节");
        assert_eq!(bin.count, 2, ".bin 文件数");
        let log = exts.iter().find(|e| e.ext == "log").expect("应有 log");
        assert_eq!(log.bytes, 5, ".log 总字节");
        assert_eq!(log.count, 1, ".log 文件数");

        // 深层目录 a/b/c 只应有 .txt(2 个) + .bin(1 个)，不应有 .log（在根）
        let c_node = node
            .children
            .iter()
            .find(|c| c.name == "a")
            .and_then(|a| a.children.iter().find(|b| b.name == "b"))
            .and_then(|b| b.children.iter().find(|c| c.name == "c"))
            .expect("a/b/c 应存在");
        let c_exts: Vec<&ExtShare> = c_node.top_extensions.iter().collect();
        assert!(
            !c_exts.iter().any(|e| e.ext == "log"),
            "a/b/c 不应统计到根的 .log"
        );
        let c_txt = c_exts
            .iter()
            .find(|e| e.ext == "txt")
            .expect("a/b/c 应有 txt");
        assert_eq!(c_txt.bytes, 50, "a/b/c 的 .txt = 20+30");
        assert_eq!(c_txt.count, 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ancestor_chain_shared_dirs_accumulate_exactly() {
        // 回归测试：祖先链热路径从 entry().or_insert_with（每文件每层克隆
        // PathBuf + 哈希两次）改为 get_mut 快路径（同目录文件共享已建好的
        // acc，仅首遇克隆）后，深层目录下多个文件共享同一祖先链时，
        // size/file_count/ext 累计必须与文件实际完全一致——这是该优化最
        // 容易踩的坑：get_mut 未命中分支若漏插或误插会静默错账。
        let dir = tempdir_path();
        fs::create_dir_all(dir.join("x/y/z")).unwrap();
        // 同目录 4 个文件共享 x/y/z 整条祖先链
        let sizes = [10u64, 20, 40, 30];
        for (i, s) in sizes.iter().enumerate() {
            fs::write(dir.join(format!("x/y/z/f{i}.dat")), vec![b'x'; *s as usize]).unwrap();
        }
        // 另一个子目录 2 个文件，验证祖先 x 的累计包含两棵子树
        fs::write(dir.join("x/other.dat"), vec![b'x'; 100]).unwrap();
        fs::write(dir.join("x/y/g.txt"), vec![b'x'; 5]).unwrap();

        let node = scan(&dir).unwrap();
        assert_eq!(node.file_count, 6, "共 6 个文件");
        assert_eq!(node.size, 10 + 20 + 40 + 30 + 100 + 5);

        let x = node
            .children
            .iter()
            .find(|c| c.name == "x")
            .expect("x 应存在");
        assert_eq!(x.size, 10 + 20 + 40 + 30 + 100 + 5, "x 含全部子树");
        assert_eq!(x.file_count, 6);

        let y = x.children.iter().find(|c| c.name == "y").expect("y 应存在");
        assert_eq!(y.size, 10 + 20 + 40 + 30 + 5, "y 不含 x/other.dat");
        assert_eq!(y.file_count, 5);

        let z = y.children.iter().find(|c| c.name == "z").expect("z 应存在");
        assert_eq!(z.size, 10 + 20 + 40 + 30, "z 只含 4 个 dat");
        assert_eq!(z.file_count, 4);

        // ext 在深层目录只统计自己的直系文件（x/y/z 无 txt/log）
        let z_exts: Vec<&ExtShare> = z.top_extensions.iter().collect();
        let dat = z_exts.iter().find(|e| e.ext == "dat").expect("z 应有 dat");
        assert_eq!(dat.bytes, 100, "z 的 .dat = 10+20+40+30");
        assert_eq!(dat.count, 4);
        assert!(
            !z_exts.iter().any(|e| e.ext == "txt"),
            "z 不应统计到 y 的 txt"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn subdirs_map_rebuilds_full_directory_children() {
        // 契约测试：build_tree 从 walk 期收集的 subdirs 内存 map 重建目录
        // children（不再二次 read_dir）。若 subdirs map 丢失/漏收集，最直接的
        // 表现是「树缺子目录」——file_count 不变（文件统计来自 accs），所以
        // 只断言 file_count 查不出这个 bug。本测试钉死：每个建了的子目录都
        // 必须作为目录节点出现在树里，且嵌套层级完整。
        let dir = tempdir_path();
        // 三棵并列子树 + 两层嵌套：刻意覆盖「同目录多个子目录」「深层链」
        for name in &["alpha", "beta", "gamma"] {
            fs::create_dir_all(dir.join(name)).unwrap();
            fs::write(dir.join(name).join("leaf.txt"), b"x").unwrap();
        }
        fs::create_dir_all(dir.join("deep/nested/inner")).unwrap();
        fs::write(dir.join("deep/nested/inner/data.bin"), vec![b'y'; 5]).unwrap();
        fs::write(dir.join("deep/top.log"), b"z").unwrap();

        let node = scan(&dir).unwrap();

        // 根：4 个子目录（alpha/beta/gamma/deep），全部出现
        let dirs: Vec<&Node> = node.children.iter().filter(|c| c.is_dir).collect();
        let names: Vec<&str> = dirs.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names.len(), 4, "子目录应全量出现在树里: {names:?}");
        for want in ["alpha", "beta", "gamma", "deep"] {
            assert!(names.contains(&want), "缺子目录 {want}，实际 {names:?}");
        }
        // 根的文件 leaf：alpha/beta/gamma 下的 leaf.txt 是各自目录的 children，
        // 根目录本身不应冒出来（避免 subdirs 与 accs 错绑）
        assert!(
            !node.children.iter().any(|c| !c.is_dir),
            "根目录文件列表不应包含子目录里的文件"
        );

        // 深层链完整：deep → nested → inner 逐层下钻
        let deep = dirs.iter().find(|c| c.name == "deep").unwrap();
        let nested = deep
            .children
            .iter()
            .find(|c| c.is_dir && c.name == "nested")
            .expect("deep 应有 nested");
        let inner = nested
            .children
            .iter()
            .find(|c| c.is_dir && c.name == "inner")
            .expect("nested 应有 inner");
        assert!(inner
            .children
            .iter()
            .any(|c| !c.is_dir && c.name == "data.bin"));
        // deep 自己的文件 top.log 仍挂在 deep 下
        assert!(deep
            .children
            .iter()
            .any(|c| !c.is_dir && c.name == "top.log"));

        // 目录 size = 子树聚合（嵌套层的文件都算进祖先）
        assert_eq!(deep.size, 6, "deep = data.bin(5) + top.log(1)");
        assert_eq!(inner.size, 5, "inner = data.bin(5)");
        assert_eq!(node.file_count, 5, "leaf.txt×3 + data.bin + top.log");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn keep_files_per_dir_top_k_semantics() {
        // 契约测试：top-K 语义（walkdir 与 mft 两条路径共享）——目录全保留，
        // 每目录文件按大小降序取前 keep；ext 统计覆盖全部直系文件（不受
        // top-K 截断影响）。任何一条路径改动都不能破坏这个契约。
        let dir = tempdir_path();
        fs::create_dir_all(dir.join("sub")).unwrap();
        // 根目录 20 个文件 + sub 5 个文件，大小各不相同以便验证排序/截断
        for i in 0..20u32 {
            fs::write(
                dir.join(format!("f{i:02}.bin")),
                vec![b'x'; (i * 7) as usize],
            )
            .unwrap();
        }
        for i in 0..5u32 {
            fs::write(
                dir.join(format!("sub/s{i}.log")),
                vec![b'x'; (i * 3) as usize],
            )
            .unwrap();
        }

        // keep=3：每目录只保留最大 3 个文件，但目录本身全部保留
        let node = scan_with(
            &dir,
            ScanOptions {
                keep_files_per_dir: Some(3),
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();

        // 目录全保留：sub 必须出现在 children
        assert!(
            node.children.iter().any(|c| c.is_dir && c.name == "sub"),
            "目录不应被 top-K 截断，children: {:?}",
            node.children
                .iter()
                .map(|c| (&c.name, c.is_dir))
                .collect::<Vec<_>>()
        );

        // 根目录文件 top-3：最大的三个是 f19(133) f18(126) f17(119)
        let root_files: Vec<&Node> = node.children.iter().filter(|c| !c.is_dir).collect();
        assert_eq!(root_files.len(), 3, "keep=3 应只留 3 个文件");
        let mut sorted: Vec<u64> = root_files.iter().map(|f| f.size).collect();
        sorted.sort_by_key(|s| std::cmp::Reverse(*s));
        assert_eq!(sorted, vec![133, 126, 119], "文件应按大小降序取 top-3");

        // ext 统计不受 top-K 截断：根目录 .bin 应覆盖全部 20 个文件
        let bin = node
            .top_extensions
            .iter()
            .find(|e| e.ext == "bin")
            .expect("应有 bin");
        assert_eq!(
            bin.count, 20,
            "ext 统计须覆盖全部直系文件（含被 top-K 丢弃的）"
        );
        assert_eq!(bin.bytes, (0..20u32).map(|i| i * 7).sum::<u32>() as u64);

        // sub 目录同理：5 个 .log 全被统计，但只保留最大 3 个
        let sub = node.children.iter().find(|c| c.name == "sub").unwrap();
        assert_eq!(sub.children.len(), 3);
        let log = sub
            .top_extensions
            .iter()
            .find(|e| e.ext == "log")
            .expect("应有 log");
        assert_eq!(log.count, 5);
        assert_eq!(log.bytes, (0..5u32).map(|i| i * 3).sum::<u32>() as u64);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn max_dirs_per_level_bounds_directory_breadth() {
        // 契约测试：目录广度上限（max_dirs_per_level）——超过上限的层按 size
        // 保留最大的 max_dirs 个子目录，并标记 children_truncated 供前端 +N
        // 取回；文件节点不受影响。这把整盘扫描树（含 scan_tree 内存缓存）
        // 钉在有界范围内。
        let dir = tempdir_path();
        // 根下 5 个子目录，大小各不相同（越大越应该保留）
        for i in 0..5u32 {
            fs::create_dir_all(dir.join(format!("d{i}"))).unwrap();
            fs::write(dir.join(format!("d{i}/big.bin")), vec![b'x'; (i * 100) as usize])
                .unwrap();
        }
        // 根下 2 个文件：不受目录 top-K 影响
        fs::write(dir.join("f1.bin"), vec![b'a'; 10]).unwrap();
        fs::write(dir.join("f2.bin"), vec![b'b'; 20]).unwrap();

        // max_dirs=2：只保留最大的 2 个子目录（d4=400, d3=300），文件全保留
        let node = scan_with(
            &dir,
            ScanOptions {
                max_dirs_per_level: Some(2),
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();

        let dirs: Vec<&Node> = node.children.iter().filter(|c| c.is_dir).collect();
        let mut names: Vec<&str> = dirs.iter().map(|c| c.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["d3", "d4"], "目录应按 size 保留最大的 2 个");

        // children_truncated 标记截断前目录总数（5；前端徽标 = 5 - 保留 2）
        assert_eq!(
            node.children_truncated,
            Some(5),
            "children_truncated 应记录截断前目录总数"
        );

        // 文件不受目录 top-K 影响：2 个全保留
        let files: Vec<&Node> = node.children.iter().filter(|c| !c.is_dir).collect();
        assert_eq!(files.len(), 2, "文件节点不应被目录 top-K 截断");

        // max_dirs=None：目录全保留（无上限语义）
        let node_all = scan_with(
            &dir,
            ScanOptions {
                max_dirs_per_level: None,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(
            node_all.children.iter().filter(|c| c.is_dir).count(),
            5,
            "max_dirs=None 时目录应全保留"
        );
        assert_eq!(node_all.children_truncated, None);

        let _ = fs::remove_dir_all(&dir);
    }

    fn tempdir_path() -> PathBuf {
        // 并发下 SystemTime::now().as_nanos() 可能撞名（Windows 时钟精度 ~100ns，
        // 并行测试线程几乎同时取时），导致两个测试共用目录互相污染计数。
        // 改用进程内原子计数器保证唯一。
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("diskpilot-test-{pid}-{seq}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
