//! TCP / UDP 连接 + PID→进程名。
//!
//! from tools/net.rs 拆分（2026-10-12）。
//!
//! 2026-10-12 再拆分：底层枚举 → `nc_raw`，连接清单聚合 → `nc_main`。

// nc_raw / nc_main 全为 Windows-only 实现（GetExtendedTcpTable 等原生 API），
// 非 Windows 由下方 cfg(not(windows)) 降级 fn 承接 net.rs 重导出链。
#[cfg(windows)]
mod nc_main;
#[cfg(windows)]
mod nc_raw;

#[cfg(windows)]
pub use nc_main::collect_net_connections;

/// 非 Windows 降级：保持 `net::collect_net_connections` 重导出链可编译。
#[cfg(not(windows))]
pub fn collect_net_connections(
    _pid: Option<u32>,
    _keyword: Option<String>,
    _top_n: usize,
) -> Result<String, String> {
    Ok("当前平台不是 Windows，网络连接信息不可用".into())
}
