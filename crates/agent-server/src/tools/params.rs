//! 全部 MCP 工具的入参 struct（参数 → 纯函数 → 格式化结果 薄壳的参数定义层）。
//!
//! 从 tools/mod.rs 拆分（2026-10-12）：参数 struct 是纯数据定义，与路由/方法体分离，
//! 按域分组（磁盘/文件/系统/网络/安全/硬件/基准/报告/盘点）。mod.rs 里以
//! `mod params;` + `use params::*;` 引入（原 impl 体内引用各 struct 的点不变）。
//!
//! 注意：宏 `#[tool]` 参数类型引用这些 struct 时走 `crate::tools::params::xxx`，
//! rmcp 要求类型在宏展开处可见，故 `mod params` 声明为同 crate 内（无 pub 即可）。
//! `schemars::JsonSchema` 是 derive 里的完整路径，无需 `use schemars`。
//!
//! 2026-10-12 再拆分：文件域 → `params_files`，硬件/网络 → `params_hw_net`，
//! 系统/安全 → `params_sys_sec`，磁盘/自修复 → `params_disk_selfheal`，
//! 基准/报告 → `params_bench_report`，长尾/盘点 → `params_misc`。

mod params_bench_report;
mod params_disk_selfheal;
mod params_files;
mod params_hw_net;
mod params_misc;
mod params_sys_sec;

pub(crate) use params_bench_report::*;
pub(crate) use params_disk_selfheal::*;
pub(crate) use params_files::*;
pub(crate) use params_hw_net::*;
pub(crate) use params_misc::*;
pub(crate) use params_sys_sec::*;
