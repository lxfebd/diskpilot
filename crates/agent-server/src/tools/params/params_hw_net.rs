//! 硬件/网络参数域（从 params.rs 拆分，2026-10-12）。
//! HwCpuParams..NetWifiParams。

use serde::Deserialize;

use crate::tools::lenient;

// ── 硬件（hw.rs）参数 ─────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwCpuParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwGpuParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwMemoryParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwMotherboardParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwTemperatureParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwSuperioParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwSensorsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwBatteryParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwDiskSmartParams {}

// ── 网络（net.rs）参数 ────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetStatusParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetAdapterDetailParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetConnectionsParams {
    #[schemars(description = "按进程 PID 过滤（可选），如 1234")]
    #[serde(default, deserialize_with = "lenient::opt_u32")]
    pub(crate) pid: Option<u32>,
    #[schemars(description = "按关键字过滤（可选）：匹配进程名或远端 IP，大小写不敏感")]
    pub(crate) keyword: Option<String>,
    #[schemars(description = "最多返回多少条（默认 30，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetSpeedParams {
    #[schemars(description = "采样窗口毫秒（1000-10000，默认 2000）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) interval_ms: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetShareParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetWifiParams {}
