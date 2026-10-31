//! 观测总览域（R485）：observations_overview 只读工具，MCP 侧统一观测入口。

use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

use crate::tools::params::*;
use crate::tools::{blocking_call, report, AgentServer};

#[tool_router(router = tool_router_observations, vis = "pub(crate)")]
impl AgentServer {
    #[tool(
        description = "观测总览（纯只读 L0）：读取统一数据根（DISKPILOT_DATA_DIR，与主进程 app_data_dir 同根）下全部观测域的索引——jsonl 域（entries/corrupt_lines/latest）、state 域（present）、hw-history/ 目录（快照数 + 最新文件名），返回 { v, data_root, domains } JSON。不采集、不写入任何文件。AI 在用户问『这台机器被观测成什么样/有哪些历史记录/扫描过几次盘』时调用，作为周报与洞察的统一入口。"
    )]
    async fn observations_overview(
        &self,
        _p: Parameters<ObservationsOverviewParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(15, report::observations_overview, "观测总览").await)
    }
}
