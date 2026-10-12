//! 软件清单/卸载/许可证工具处理器（从 tool_sys.rs 拆分，2026-10-12）。
//! 独立 `#[tool_router(router = tool_router_sys_apps)]` 块，mod.rs 合并。

use crate::tools::params::*;
use crate::tools::{apps, blocking_call, files, tool_error, AgentServer};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

#[tool_router(router = tool_router_sys_apps, vis = "pub(crate)")]
impl AgentServer {
    #[tool(
        description = "列出本机已安装程序（只读，来自注册表 Uninstall 键）：名称/版本/发布者/安装位置/预估大小。AI 回答『这台电脑装了什么软件/这个软件能卸载吗』时调用。"
    )]
    async fn app_list(
        &self,
        Parameters(p): Parameters<AppListParams>,
    ) -> Result<CallToolResult, McpError> {
        let keyword = p.keyword.clone().unwrap_or_default().to_ascii_lowercase();
        let limit = p.limit.unwrap_or(50).clamp(1, 200);
        Ok(blocking_call(
            30,
            move || {
                let all = apps::list_installed_apps();
                if all.is_empty() {
                    return Err("未读取到已安装程序（非 Windows 环境或注册表不可读）。".to_string());
                }
                let mut matched: Vec<_> = if keyword.is_empty() {
                    all
                } else {
                    all.into_iter()
                        .filter(|a| {
                            a.name.to_ascii_lowercase().contains(&keyword)
                                || a.publisher.to_ascii_lowercase().contains(&keyword)
                        })
                        .collect()
                };
                let truncated = matched.len() > limit;
                matched.truncate(limit);
                let lines = matched
                    .iter()
                    .map(|a| {
                        let mut s = format!("- {}（{}）", a.name, a.version);
                        if !a.publisher.is_empty() {
                            s.push_str(&format!("，{}", a.publisher));
                        }
                        if let Some(mb) = a.estimated_size_mb {
                            if mb > 0 {
                                s.push_str(&format!("，约 {} MB", mb));
                            }
                        }
                        if !a.install_location.is_empty() {
                            s.push_str(&format!("，位于 {}", a.install_location));
                        }
                        s
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut head = format!("已安装程序（{} 个）：\n{lines}", matched.len());
                if truncated {
                    head.push_str(&format!("\n（只显示前 {} 个）", limit));
                }
                Ok(head)
            },
            "已安装程序列表",
        )
        .await)
    }

    #[tool(
        description = "卸载已安装程序（**写操作**，启动其官方卸载器）。安全守卫：只允许已安装程序目录（Program Files / LocalAppData 等）下 UninstallString 指向的卸载器，卸载参数只允许静默白名单（/S /silent /quiet /qn 等），拒绝 runas/delete 等危险参数。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。AI 在用户明确要求『卸载某软件』时先调 app_list 确认名称再调用；卸载器以分离方式启动（有自己界面/进度），完成后建议 app_list 复查。"
    )]
    async fn uninstall_app(
        &self,
        Parameters(p): Parameters<UninstallAppParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let name = p.name.clone();
        Ok(blocking_call(120, move || apps::uninstall_app(&name), "卸载程序").await)
    }

    #[tool(
        description = "查询软件许可证：Windows 激活状态（slmgr /xpr）+ Office 激活状态（只读，不修改任何系统状态）。用户问『这个系统正版吗/激活了吗/Office 什么版本』时调用。Windows 激活查询可能需要管理员权限，无权限时如实返回。"
    )]
    async fn app_licenses(
        &self,
        _p: Parameters<AppLicensesParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, apps::collect_licenses, "许可证查询").await)
    }
}
