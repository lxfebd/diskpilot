//! TCP / UDP 连接 + PID→进程名。
//!
//! from tools/net.rs 拆分（2026-10-12）。
//!
//! 2026-10-12 再拆分：底层枚举 → `nc_raw`，连接清单聚合 → `nc_main`。

mod nc_main;
mod nc_raw;

pub use nc_main::collect_net_connections;
