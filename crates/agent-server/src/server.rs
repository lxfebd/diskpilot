//! MCP 标准能力扩展：resources + prompts（tools 之外的两大能力面）。
//!
//! `#[tool_handler]` 只生成 get_info / call_tool / list_tools；resources/prompts
//! 的 list/read/get 在这里以 `ServerHandler` trait 方法补齐，能力在
//! `ServerCapabilities` 中声明（见 tools/mod.rs）。
//!
//! - **resources**（只读，客户端随时拉取，不占工具调用）：
//!   `system://profile` 系统摘要、`system://disk-usage` 磁盘分区用量、
//!   `system://env` 关键环境变量。内容与同名工具口径一致。
//! - **prompts**（任务引导模板，客户端一键填入）：
//!   `system-report`（生成体检报告）、`cleanup-plan`（清理某目录）。

use crate::tools::{disk, envx, fmt_bytes, system, AgentServer};
use rmcp::{
    model::{
        // R551 rmcp 3.9：AnnotateAble trait 已移除（annotations 默认 None），
        // PromptMessageContent→ContentBlock、PromptMessageRole→Role、RawResource→Resource
        ContentBlock,
        GetPromptRequestMethod,
        GetPromptResult,
        ListPromptsResult,
        ListResourcesResult,
        Prompt,
        PromptArgument,
        PromptMessage,
        ReadResourceRequestParams,
        ReadResourceResult,
        Resource,
        ResourceContents,
        Role,
        ServerCapabilities,
        ServerConfig,
    },
    ErrorData as McpError,
};

/// 资源 URI → 生成文本的映射。同步、快速、无副作用；
/// 内容直接复用现有采集实现，保证与工具返回口径一致。
fn resource_text(uri: &str) -> Option<String> {
    match uri {
        "system://profile" => Some(format_system_summary(&system::collect_system_info())),
        "system://disk-usage" => {
            let disks = disk::collect_disks();
            if disks.is_empty() {
                Some("（未枚举到磁盘分区）".to_string())
            } else {
                let mut lines = Vec::with_capacity(disks.len() + 1);
                lines.push("磁盘分区用量：".to_string());
                for d in &disks {
                    lines.push(format!(
                        "{}\t总 {} / 已用 {}（{:.1}%）/ 剩余 {}",
                        d.drive,
                        fmt_bytes(d.total_bytes),
                        fmt_bytes(d.used_bytes),
                        d.used_percent,
                        fmt_bytes(d.free_bytes)
                    ));
                }
                Some(lines.join("\n"))
            }
        }
        "system://env" => match envx::collect_env_vars(None) {
            Ok(s) => Some(format!("关键环境变量：\n{s}")),
            Err(e) => Some(format!("（环境变量读取失败：{e}）")),
        },
        _ => None,
    }
}

/// 系统摘要文本（与 tool_sys::system_info 工具同一口径）。
fn format_system_summary(s: &system::SystemInfo) -> String {
    let up_days = s.uptime_secs / 86400;
    let up_hrs = (s.uptime_secs % 86400) / 3600;
    format!(
        "系统：{}\nCPU 逻辑核心：{}\n内存：{}，已用 {}（{:.1}%）\n开机时长：{} 天 {} 小时",
        s.os,
        s.cpu_cores,
        fmt_bytes(s.mem_total_bytes),
        fmt_bytes(s.mem_used_bytes),
        s.mem_percent,
        up_days,
        up_hrs
    )
}

/// 三个静态只读资源。
pub(crate) fn list_resources() -> ListResourcesResult {
    let make = |uri: &str, name: &str, description: &str| {
        Resource::new(uri, name)
            .with_description(description)
            .with_mime_type("text/plain")
    };
    ListResourcesResult::with_all_items(vec![
        make(
            "system://profile",
            "系统摘要",
            "主机系统版本 / CPU 核心 / 内存总量与使用率 / 开机时长",
        ),
        make(
            "system://disk-usage",
            "磁盘分区用量",
            "所有盘符的总量 / 已用 / 剩余（字节与百分比）",
        ),
        make(
            "system://env",
            "关键环境变量",
            "常用环境变量快照（PATH / OS / 用户目录等）",
        ),
    ])
}

/// 读取一个资源；未知 URI 返回 RESOURCE_NOT_FOUND。
pub(crate) async fn read_resource(
    request: ReadResourceRequestParams,
) -> Result<ReadResourceResult, McpError> {
    let uri = request.uri.clone();
    match resource_text(&uri) {
        Some(text) => Ok(ReadResourceResult::new(vec![ResourceContents::text(
            text, uri,
        )])),
        None => Err(McpError::resource_not_found(
            format!("未知资源 URI：{uri}"),
            None,
        )),
    }
}

/// 两个任务引导模板。
pub(crate) fn list_prompts() -> ListPromptsResult {
    ListPromptsResult::with_all_items(vec![
        Prompt::new(
            "system-report",
            Some("生成整机体检报告：先读 system://profile 与磁盘用量，再视需要调硬件/传感器工具补全温度与健康状态，最后输出结构化报告。"),
            Some(vec![PromptArgument::new("detail")
                .with_title("报告粒度")
                .with_description("summary=概览（默认）；full=含温度/传感器/告警明细")
                .with_required(false)]),
        ),
        Prompt::new(
            "cleanup-plan",
            Some("为指定目录生成清理建议：先 file_tree / find_files 摸清内容，再用 cleanup_suggestions 找可回收项，最后给出带确认门槛的清理计划。"),
            Some(vec![PromptArgument::new("path")
                .with_title("目标目录")
                .with_description("要分析的目录绝对路径（受路径守卫约束，盘根/系统目录会被拒绝）")
                .with_required(true)]),
        ),
    ])
}

/// 按名取模板内容；未知模板返回 METHOD_NOT_FOUND，缺参数返回 INVALID_PARAMS。
pub(crate) async fn get_prompt(
    request: rmcp::model::GetPromptRequestParams,
) -> Result<GetPromptResult, McpError> {
    let name = request.name.clone();
    let message = |text: String| PromptMessage::new(Role::User, ContentBlock::text(text));
    match name.as_str() {
        "system-report" => {
            let detail = request
                .arguments
                .as_ref()
                .and_then(|m| m.get("detail"))
                .and_then(|v| v.as_str())
                .unwrap_or("summary");
            let mut text = String::from(
                "请为这台机器生成一份体检报告。先读取资源 system://profile 和 system://disk-usage 获取基础信息，",
            );
            if detail == "full" {
                text.push_str(
                    "再依次调用 hw_temperature / hw_sensors / sensor_alert 补全温度与硬件健康状态；",
                );
            }
            text.push_str(
                "最后按『概览 → 磁盘 → 硬件 → 风险与建议』的结构输出报告，所有数据以工具返回为准，不确定的标注为推测。",
            );
            Ok(GetPromptResult::new(vec![message(text)]).with_description("生成整机体检报告"))
        }
        "cleanup-plan" => {
            let path = request
                .arguments
                .as_ref()
                .and_then(|m| m.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path.is_empty() {
                return Err(McpError::invalid_params(
                    "cleanup-plan 需要 path 参数（目标目录绝对路径）",
                    None,
                ));
            }
            Ok(GetPromptResult::new(vec![message(format!(
                "请为目录 {path} 生成清理建议：先 file_tree / find_files 摸清内容，再用 cleanup_suggestions 找出可回收项，最后给出分步清理计划。写操作（file_recycle）前必须先列出将回收的文件并征得用户确认。"
            ))])
            .with_description("生成目录清理计划"))
        }
        _ => Err(McpError::method_not_found::<GetPromptRequestMethod>()),
    }
}

/// 供 tools/mod.rs 合并能力声明：工具 + 资源 + 提示词模板。
pub(crate) fn capabilities() -> ServerCapabilities {
    ServerCapabilities::builder()
        .enable_tools()
        .enable_resources()
        .enable_prompts()
        .build()
}

/// 供 tools/mod.rs 拼 instructions（能力面齐全后说明一次说清）。
pub(crate) fn instructions() -> &'static str {
    "DiskPilot 的 AI 可操作工具集：只读磁盘/系统/文件/进程/程序清单查询。\
     所有路径参数受安全守卫保护（盘根/系统目录/主目录根被拒绝）。\
     另有只读资源（system://profile / system://disk-usage / system://env）\
     与任务模板（system-report / cleanup-plan）可直接使用。\
     回答磁盘空间、系统状态、目录内容、进程、已装软件问题时先用这些工具拿真实数据。"
}

// 显式让编译器知道这些 trait 方法由 mod.rs 转发调用，避免重构时漏接。
#[allow(dead_code)]
fn _api_guard(_: &AgentServer, _: &ServerConfig) {}
