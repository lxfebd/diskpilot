//! 盘点/杂项工具处理器（从 tool_sys.rs 拆分，2026-10-12）。
//! 独立 `#[tool_router(router = tool_router_sys_misc)]` 块，mod.rs 合并。

use crate::tools::params::*;
use crate::tools::{blocking_call, envx, hw, sec, AgentServer};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

#[tool_router(router = tool_router_sys_misc, vis = "pub(crate)")]
impl AgentServer {
    #[tool(
        description = "列出 USB 设备：控制器/集线器/外设 + 厂商/服务/状态（只读，PNP 枚举，普通权限，不读设备内容）。用户问『插了什么 USB 设备/U 盘/键鼠/摄像头识别到没有』时调用。"
    )]
    async fn usb_devices(
        &self,
        _p: Parameters<UsbDevicesParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_usb_devices, "USB 设备枚举").await)
    }

    #[tool(
        description = "读取 Windows Defender 安全中心状态（只读）：服务状态 + 实时保护开关 + 病毒库/引擎版本 + 最近快速扫描时间。用户问『杀毒软件开没开/Windows 安全中心状态/病毒库新不新』时调用；若系统用第三方杀软（Defender 被替换）会如实说明。"
    )]
    async fn sys_defender_status(
        &self,
        _p: Parameters<DefenderStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, sec::collect_defender_status, "Defender 状态读取").await)
    }

    #[tool(
        description = "枚举本机用户账户（只读）：账户名/全名/是否禁用/是否锁定/本地或域账户/SID 尾段，排除内置系统账户（defaultuser0 等）。用户问『这台电脑有哪些用户/有几个账户/禁用账户是谁』时调用；无需管理员即可读取，受限时如实降级。隐私红线：绝不输出任何密码/token 信息。"
    )]
    async fn sys_user_accounts(
        &self,
        _p: Parameters<UserAccountsParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, sec::collect_user_accounts, "用户账户枚举").await)
    }

    #[tool(
        description = "查询本机回收站状态（只读，Windows API）：逐卷文件数与总大小 + 合计。用户问『回收站占了多少空间/回收站里有多少东西/能不能清回收站腾空间』时调用。只读状态，不执行清空。"
    )]
    async fn recycle_bin_stats(
        &self,
        _p: Parameters<RecycleBinStatsParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, envx::collect_recycle_bin_stats, "回收站状态查询").await)
    }

    #[tool(
        description = "读取环境变量值（只读）：传 name 查指定变量；不传列出常用子集（PATH/TEMP/USERPROFILE/APPDATA 等）。用户问『某个路径在哪/环境变量配没配/命令找不到是不是 PATH 问题』时调用。"
    )]
    async fn env_vars(
        &self,
        Parameters(p): Parameters<EnvVarsParams>,
    ) -> Result<CallToolResult, McpError> {
        let name = p.name.clone();
        Ok(blocking_call(30, move || envx::collect_env_vars(name), "环境变量读取").await)
    }
}
