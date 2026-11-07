//! Cross-platform disk scanner. Returns a tree of directories with size/file_count.
//!
//! v0.1.1: parallel walk via jwalk (rayon under the hood) + per-leaf file
//! children + progress callback. v0.2 will swap in direct NTFS MFT read on
//! Windows for sub-3s C: drive scans.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 盘根下的系统交换/休眠文件（lowercase），占几十 GB 但不可删除。
/// 只在文件收集阶段跳过，避免污染"可回收潜力"统计。
const PRUNED_SYSTEM_FILES: &[&str] = &["hiberfil.sys", "pagefile.sys", "swapfile.sys"];

pub(crate) fn is_pruned_system_file(name: &str) -> bool {
    PRUNED_SYSTEM_FILES.contains(&name)
}

#[cfg(windows)]
mod mft;
#[cfg(windows)]
mod mft_parse;
/// MFT 相关子模块（从 mft.rs 拆出，2026-10-12）：
/// - mft_reader: 512B 对齐卷设备读取器
/// - mft_parse: 记录解析（parse_mft_fast/fixup_all/parse_record）
/// - mft_tree: 条目→Node 树（link_children/rollup/build_node_tree/…）
/// mft.rs 保留 scan_volume 入口 + tests + bench，内部经 pub(crate) use 引用。
#[cfg(windows)]
mod mft_reader;
#[cfg(windows)]
mod mft_tree;
/// bench feature 下把 mft 内部结构暴露给冒烟 CLI（仅 bench::run 一个入口）。
#[cfg(all(windows, feature = "bench"))]
pub mod mft_bench {
    pub use super::mft::bench;
}
/// USN Journal 增量扫描（Windows）：回放变更日志 + 只重扫变更文件。
/// 解析层跨平台纯函数，DeviceIoControl 层仅在 Windows 编译。
#[cfg(windows)]
pub mod usn;
/// USN 变更在缓存树上的合并/应用/删除逻辑（从 usn.rs 拆出，2026-10-12）。
/// 纯树操作，无 DeviceIoControl；经 usn.rs `pub use` 重导出保持旧路径。
#[cfg(windows)]
pub mod usn_tree;
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

impl Node {
    /// 子树节点总数（含自身）。
    pub fn total_nodes(&self) -> u64 {
        1 + self.children.iter().map(|c| c.total_nodes()).sum::<u64>()
    }

    /// 子树内存占用的粗略下界估算（字节）：节点结构体 + 字符串长度 +
    /// 顶层扩展名元数据。不含分配器/对齐/树容器自身开销，仅供日志对比
    /// 「完整树 vs 截断树」的数量级，不作为精确度量。
    pub fn memory_estimate(&self) -> usize {
        let own = std::mem::size_of::<Node>()
            + self.name.len()
            + self.path.len()
            + self.scaffold_id.as_ref().map_or(0, |s| s.len())
            + self
                .top_extensions
                .iter()
                .map(|e| e.ext.len() + 16)
                .sum::<usize>();
        own + self
            .children
            .iter()
            .map(|c| c.memory_estimate())
            .sum::<usize>()
    }
}

#[derive(Debug, Default)]
pub(crate) struct DirAcc {
    pub(crate) size: u64,
    pub(crate) file_count: u64,
    pub(crate) ext_bytes: HashMap<String, u64>,
    pub(crate) ext_count: HashMap<String, u64>,
    /// (file name, size, mtime-unix-sec) — top-K kept on the immediate parent.
    /// Bounded during the walk so a single huge directory (e.g. node_modules,
    /// an unpacked dataset) doesn't grow `files` unbounded before build_tree
    /// prunes it. `mtime` is captured from the same metadata call that yields
    /// `size` (zero extra IO); `None` when the source didn't provide one
    /// (MFT path / unreadable).
    pub(crate) files: Vec<(String, u64, Option<u64>)>,
    /// Max entries kept in `files`; `None` = keep everything (memory hog).
    pub(crate) files_cap: Option<usize>,
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

pub use scan_core::*;
mod scan_core;

// R516 扫描报告导出（markdown/csv/json 渲染，纯内存零 IO）。
mod report;
pub use report::*;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
