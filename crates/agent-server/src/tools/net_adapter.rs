//! 网卡清单（原生 iphlpapi + PowerShell 兜底）。
//!
//! from tools/net.rs 拆分（2026-10-12）。collect_net_status / collect_adapter_detail 的数据来源。
//!
//! 2026-10-12 再拆分：识别 → `nad_ident`，原生枚举 → `nad_native`，PS 兜底 → `nad_ps`。

mod nad_ident;
mod nad_native;
mod nad_ps;

pub(crate) use nad_ident::is_virtual_adapter;
pub(crate) use nad_native::native_adapters;
pub(crate) use nad_ps::ps_adapters_fallback;
