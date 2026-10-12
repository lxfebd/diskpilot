//! 系统服务/驱动/启动工具处理器（从 tool_sys.rs 拆分，2026-10-12）。
//! 独立 `#[tool_router(router = tool_router_sys_services)]` 块，mod.rs 合并。

use crate::tools::params::*;
use crate::tools::{blocking_call, files, sys, tool_error, AgentServer};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

#[tool_router(router = tool_router_sys_services, vis = "pub(crate)")]
impl AgentServer {
    // ── 硬件（hw.rs）──────────────────────────────────────────────

    #[tool(
        description = "列出 Windows 服务：服务名/显示名/状态/启动类型/进程 PID（只读，不需要管理员权限）。可按关键字过滤、可含已禁用服务——服务启动类型与 ImagePath 直接读注册表，准确度高。用户问『什么服务在跑/某服务是干嘛的/怎么启动方式』时调用。"
    )]
    async fn sys_services(
        &self,
        Parameters(p): Parameters<SysServicesParams>,
    ) -> Result<CallToolResult, McpError> {
        let keyword = p.keyword.clone();
        let show_disabled = p.show_disabled.unwrap_or(false);
        let top_n = p.top_n.unwrap_or(50);
        Ok(blocking_call(
            45,
            move || sys::collect_services(keyword, show_disabled, top_n),
            "服务列表读取",
        )
        .await)
    }

    #[tool(
        description = "列出驱动（设备驱动）：名称/类型/厂商/驱动日期与版本/设备状态（只读，不需要管理员权限）。可按关键字过滤。用户问『装了什么驱动/某驱动是干嘛的/驱动是不是太旧』时调用。"
    )]
    async fn sys_drivers(
        &self,
        Parameters(p): Parameters<SysDriversParams>,
    ) -> Result<CallToolResult, McpError> {
        let keyword = p.keyword.clone();
        let top_n = p.top_n.unwrap_or(50);
        Ok(blocking_call(
            45,
            move || sys::collect_drivers(keyword, top_n),
            "驱动列表读取",
        )
        .await)
    }

    #[tool(
        description = "列出开机启动项：注册表 Run 键（含策略强制项与 32 位视图）+ 启动文件夹（只读，不删除不禁用），可附带计划任务。僵尸启动项（文件已不存在）会标红提示。用户问『开机自动启动了什么/怎么精简开机项』时调用。"
    )]
    async fn sys_boot_items(
        &self,
        Parameters(p): Parameters<SysBootItemsParams>,
    ) -> Result<CallToolResult, McpError> {
        let include_scheduled = p.include_scheduled.unwrap_or(false);
        Ok(blocking_call(
            45,
            move || sys::collect_boot_items(include_scheduled),
            "启动项读取",
        )
        .await)
    }

    #[tool(
        description = "控制 Windows 服务：启动 / 停止 / 重启（**写操作，可逆**）。系统关键服务（内核/系统核心）会被安全守卫拒绝。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。用户明确要求『帮我停掉某服务/启动某服务/重启某服务』时调用；参数 action 取 start / stop / restart。"
    )]
    async fn service_control(
        &self,
        Parameters(p): Parameters<ServiceControlParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let name = p.name.clone();
        let action = p.action.clone();
        Ok(blocking_call(60, move || sys::control_service(&name, &action), "服务控制").await)
    }
}
