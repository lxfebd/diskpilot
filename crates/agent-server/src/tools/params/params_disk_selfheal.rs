//! 磁盘深入/自修复参数域（从 params.rs 拆分，2026-10-12）。
//! BiggestFilesParams..ToolbeltRunParams。

use serde::Deserialize;

use crate::tools::lenient;

// ── 磁盘深入（diskx.rs）参数 ──────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct BiggestFilesParams {
    #[schemars(description = "要搜索的目录绝对路径，如 C:\\Users\\me\\Downloads")]
    pub(crate) path: String,
    #[schemars(description = "最多返回多少个文件（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DuplicateFilesParams {
    #[schemars(description = "要搜索的目录绝对路径，如 C:\\Users\\me\\Downloads")]
    pub(crate) path: String,
    #[schemars(description = "最多返回多少组重复（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DiskIoUsageParams {
    #[schemars(description = "采样窗口毫秒（默认 1000）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) sample_ms: Option<u64>,
}

// ── 自修复（selfheal.rs）参数 ─────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FanSelfhealDiagParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FanSelfhealFixParams {
    #[schemars(
        description = "修复动作：install_driver（安装 inpoutx64 驱动，需管理员）/ reset_fan <idx>（恢复风扇 idx 主板控制）"
    )]
    pub(crate) action: String,
    #[schemars(
        description = "只出计划不执行（默认 true）。AI 先 dry-run 给用户确认后再传 false 真正执行"
    )]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FanControlParams {
    #[schemars(description = "风扇索引（数字，先 fan_selfheal_diag 查 fans 列表确认）")]
    pub(crate) idx: String,
    #[schemars(description = "目标转速百分比 0-100（越界自动钳制）")]
    pub(crate) pct: f64,
    #[schemars(
        description = "只出计划不执行（默认 true）。AI 先 dry-run 给用户确认后再传 false 真正执行"
    )]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ToolbeltListParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ToolbeltRunParams {
    #[schemars(
        description = "工具箱工具名（先调 toolbelt_list 确认名称），如 crystaldiskinfo / wiztree / defraggler"
    )]
    pub(crate) tool: String,
    #[schemars(
        description = "传给工具的 CLI 参数列表（可选，透传不拼 shell；空则用工具默认参数）"
    )]
    pub(crate) args: Option<Vec<String>>,
    #[schemars(
        description = "用户已确认执行（medium/high 风险必须 true，low 风险可省略）。AI 应先把要执行的命令展示给用户，用户同意后再传 true"
    )]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) confirmed: Option<bool>,
    #[schemars(description = "执行超时秒数（默认 30，上限 300；超过强制终止）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) timeout_secs: Option<u64>,
}
