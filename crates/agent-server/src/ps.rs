//! PowerShell 执行器：跑一段只读 PowerShell/CIM 脚本并捕获 stdout。
//!
//! **自 v0.2.3 架构重构起改为薄壳**：实现统一迁入共享 crate
//! `crates/psutil`（diskpilot-psutil），本模块只做 re-export，消费方
//! `use crate::ps::ps_capture` 等调用面完全不变。
//!
//! 历史沿革：原实现从 desktop `hw.rs::ps_capture` 移植（零耦合纯函数），
//! 加上 UTF-8 首行注入改良；为避免 desktop/agent-server 双份漂移，统一
//! 收进 psutil。实现要点（见 psutil）：
//! - CREATE_NO_WINDOW 语义（stdin/stderr null）、try_wait 轮询超时强杀；
//! - 脚本首行强制 `[Console]::OutputEncoding = UTF8`，根治中文系统 GBK 乱码
//!   （GBK 兜底降为二阶保险）；
//! - 中文 Windows 上控制台输出是 GBK，UTF-8 失败时按 GBK 兜底解码（encoding_rs）。
//!
//! 硬件/网络/服务/事件查询全走 CIM/PowerShell，**不需要管理员权限**（个别除外，见工具描述）。
//!
//! **跨平台**：`ps_capture` / `ps_capture_timeout` 仅在 Windows 有真实现（PowerShell.exe）；
//! 非 Windows 构建返回明确错误，不 panic、不 spawn 失败命令。各消费模块（hw/net/sys/sec/…）
//! 自身已有 `#[cfg(not(windows))]` 降级分支，运行时不会走到这里的非 Windows 实现——
//! 它只为让那些模块顶层的 `use crate::ps::ps_capture` 能在 macOS/Linux 上通过编译。
//! `json_array_of` 是纯文本函数，全平台可用。

pub use diskpilot_psutil::{json_array_of, ps_capture, ps_capture_timeout, ADL_PROBE_PS};

/// 单引号转义重导出（兼容 `crate::ps::escape_ps_single_quoted` 旧路径，
/// 实际消费方 sys.rs 经 `crate::tools::sys::escape_ps_single_quoted` 引用）。
#[allow(unused_imports)]
pub use diskpilot_psutil::escape_ps_single_quoted;
