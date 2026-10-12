//! 网卡清单（原生 iphlpapi + PowerShell 兜底）。
//!
//! from tools/net.rs 拆分（2026-10-12）。collect_net_status / collect_adapter_detail 的数据来源。
//!
//! 2026-10-12 再拆分：识别 → `nad_ident`，原生枚举 → `nad_native`，PS 兜底 → `nad_ps`。

// 三个子模块全为 Windows-only 实现（iphlpapi / 注册表 / PowerShell），
// 消费方 net_status.rs 的 collect_* 均带 cfg(windows) + 非 Windows 降级分支。
#[cfg(windows)]
mod nad_ident;
#[cfg(windows)]
mod nad_native;
#[cfg(windows)]
mod nad_ps;

#[cfg(windows)]
pub(crate) use nad_ident::is_virtual_adapter;
#[cfg(windows)]
pub(crate) use nad_native::native_adapters;
#[cfg(windows)]
pub(crate) use nad_ps::ps_adapters_fallback;
