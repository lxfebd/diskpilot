//! 长尾/盘点参数域（从 params.rs 拆分，2026-10-12）。
//! HwCpuFeaturesParams..EnvVarsParams。

use serde::Deserialize;

use crate::tools::lenient;

// ── 阶段四：信息补全（hw.rs 扩展）参数 ───────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwCpuFeaturesParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwDramTimingsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwDisplaysParams {}

// ── 阶段五：长尾（bench_gpu / IPMI / ACPI）参数 ────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct BenchGpuParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwIpmiParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwAcpiParams {}

// ── 盘点/建议（steam.rs + cleanup.rs）参数 ────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SteamGamesParams {
    #[schemars(description = "每个库最多列出多少个游戏（按大小降序，默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct CleanupSuggestionsParams {
    #[schemars(
        description = "要统计的目录绝对路径：**通常直接传盘根**（如 C:\\），本工具为只读统计允许盘根；也可传盘下任意目录（系统目录/用户主目录根仍会被守卫拒绝）"
    )]
    pub(crate) root: String,
    #[schemars(description = "返回按可清理字节降序的 Top N 个项（默认 20，上限 50）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct TopDirectoriesParams {
    #[schemars(description = "要统计的目录绝对路径，如 C:\\Users\\me\\Downloads")]
    pub(crate) path: String,
    #[schemars(description = "返回最大的 N 个子目录（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ProcessCpuUsageParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct UsbDevicesParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DiskSmartRawParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RecycleBinStatsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct EnvVarsParams {
    pub(crate) name: Option<String>,
}
