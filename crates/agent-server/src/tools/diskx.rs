//! 磁盘/文件深度只读扫描工具（大文件 / 重复文件 / 磁盘 IO 速率）。
//!
//! **纯工具文件**：不接路由，由后续 `tools/mod.rs` 薄壳对接 MCP tool。
//! **全部只读**：不写注册表、不改文件、不动回收站、不改权限；读文件只做头部哈希。
//!
//! ## 导出函数清单
//!
//! 1. `pub fn collect_biggest_files(path: String, top_n: usize, progress: Option<&dyn Fn(usize, usize)>) -> Result<String, String>`
//!    递归扫描某目录下所有文件大小，返回 Top N（默认 20，钳 5..=100）。
//!    降序：同大小按路径字典序。最多看 500,000 个文件，超过标 truncated。
//!    `progress` 可选回调（files_seen, 上限），供长扫描时上报进度。
//!
//! 2. `pub fn collect_duplicate_files(path: String, top_n: usize, progress: Option<&dyn Fn(usize, usize)>) -> Result<String, String>`
//!    按 `size` 分组（≥2 才是候选）→ 组内做头部 64KB `DefaultHasher` 判定，输出重复组。
//!    最多看 200,000 个文件；同 size 组内文件数 > top_n 时不深比对，只报"未深度比对"。
//!
//! 3. `pub fn collect_disk_io_usage(sample_ms: u64) -> Result<String, String>`
//!    走 CIM `Win32_PerfFormattedData_PerfDisk_PhysicalDisk` 取每盘实时读/写速率与占用率。
//!    过滤：`_Total` = 总计，`0 C:` / `1 D:` 等 = 单盘，`HarddiskN` / `2 C:` 等副本忽略。
//!    `sample_ms` 仅为参考提示（PerfFormattedData 已给当前速率，不做真实双采样）。
//!
//! 2026-10-12 拆分：大文件 → `dkx_big`，重复文件 → `dkx_dup`，
//! 磁盘 IO → `dkx_io`，目录占用 → `dkx_tops`。

mod dkx_big;
mod dkx_dup;
mod dkx_io;
mod dkx_tops;

pub use dkx_big::collect_biggest_files;
pub use dkx_dup::collect_duplicate_files;
pub use dkx_io::collect_disk_io_usage;
pub use dkx_tops::collect_top_directories;
