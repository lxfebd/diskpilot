//! 系统/安全参数域（从 params.rs 拆分，2026-10-12）。
//! SysServicesParams..UserAccountsParams。

use serde::Deserialize;

use crate::tools::lenient;

// ── 系统服务/驱动/启动（sys.rs）参数 ──────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SysServicesParams {
    #[schemars(description = "按服务名/显示名关键字过滤（可选），如 sql")]
    pub(crate) keyword: Option<String>,
    #[schemars(description = "是否包含已禁用的服务（默认 false 只看启用/运行中）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) show_disabled: Option<bool>,
    #[schemars(description = "最多返回多少个服务（默认 50，上限 200）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ServiceControlParams {
    #[schemars(description = "服务名（先调 sys_services 确认名字），如 spooler")]
    pub(crate) name: String,
    #[schemars(description = "动作：start 启动 / stop 停止 / restart 重启")]
    pub(crate) action: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SysDriversParams {
    #[schemars(description = "按驱动名关键字过滤（可选），如 nvidia")]
    pub(crate) keyword: Option<String>,
    #[schemars(description = "最多返回多少个驱动（默认 50，上限 200）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SysBootItemsParams {
    #[schemars(description = "是否附带读取计划任务启动项（较慢，默认 false）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) include_scheduled: Option<bool>,
}

// ── 安全/审计（sec.rs）参数 ───────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct EventLogsParams {
    #[schemars(description = "日志名（逗号分隔），如 System 或 System,Application（默认 System）")]
    pub(crate) logs: Option<String>,
    #[schemars(description = "回溯小时数（默认 24，上限 720）")]
    #[serde(default, deserialize_with = "lenient::opt_u32")]
    pub(crate) hours: Option<u32>,
    #[schemars(description = "每个日志最多返回多少条（默认 20，上限 50）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) max_events: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FirewallRulesParams {
    #[schemars(description = "方向过滤：Inbound / Outbound / Any（默认 Inbound）")]
    pub(crate) direction: Option<String>,
    #[schemars(description = "动作过滤：Allow / Block / Any（默认 Allow）")]
    pub(crate) action: Option<String>,
    #[schemars(description = "最多返回多少条（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct LoginEventsParams {
    #[schemars(description = "最多返回多少条（默认 20，上限 50）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) max_events: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DefenderStatusParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct UserAccountsParams {}
