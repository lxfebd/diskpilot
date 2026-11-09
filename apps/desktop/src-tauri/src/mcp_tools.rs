//! MCP 工具清单与调用（从 mcp.rs 拆分，2026-10-12）。
//! mcp_list_tools（合并去重）/ mcp_call_tool（权限映射 + 审计 + 60s 超时）。

use rmcp::model::CallToolRequestParams;
use tauri::{AppHandle, State};

use super::mcp_audit::{append_audit, audit_args_snapshot, audit_ts};
use super::mcp_probe::probe_tools;
use super::mcp_store::{load_servers, load_servers_explicit};
use super::McpToolMeta;
use super::{CALL_TOOL_TIMEOUT_SECS, PROBE_TIMEOUT_SECS};
use crate::mcp_bridge::connect_server;

/// 所有 enabled 服务器的工具合并清单（只读 L0，前端 refreshMcpTools 拉取）。
/// 配置读失败（存在但损坏）报错：空工具清单会让 AI 静默失去全部工具
/// （读失败伪装成空能力集，R81 同族）。
#[tauri::command]
pub(crate) async fn mcp_list_tools(app: AppHandle) -> Result<Vec<McpToolMeta>, String> {
    let servers = load_servers_explicit(&app)?;
    let mut out = Vec::new();
    for s in servers.iter().filter(|s| s.enabled) {
        // 单服务器 10s 兜底超时；失败跳过不拖垮整体（health 在 list_servers 里可见）。
        let tools = match tokio::time::timeout(
            std::time::Duration::from_secs(PROBE_TIMEOUT_SECS),
            probe_tools(s),
        )
        .await
        {
            Ok(Ok(t)) => t,
            _ => continue,
        };
        for t in tools {
            let raw = t.name.to_string();
            let (perm, _) = s.resolve_perm(&raw);
            out.push(McpToolMeta {
                server_id: s.id.clone(),
                name: s.tool_name(&raw),
                description: t.description.unwrap_or_default().to_string(),
                input_schema: serde_json::Value::Object(t.input_schema.as_ref().clone()),
                writable: s.writable,
                perm,
            });
        }
    }
    // 按 server_id + name 去重（多服务器同名工具时保留先注册的）。
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| seen.insert(format!("{}:{}", t.server_id, t.name)));
    Ok(out)
}

/// 调用某个 MCP 服务器上的工具（工具级权限映射 + 审计，2026-09-11）。
/// 权限判定（`permission_map` 解析，见 [`McpServerConfig::resolve_perm`]）：
/// - L0 → 直接放行（只读，无确认）；
/// - L1 → 需 confirmed=true；
/// - L2 → 需 confirmed=true + 权限（映射的 perm id 或缺省 mcp.manage）；
/// - L3 → 拒绝（不可解锁）。
/// 未映射时按 writable 推断（true → L2，false → L0）。
#[tauri::command]
pub(crate) async fn mcp_call_tool(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    server_id: String,
    name: String,
    arguments: serde_json::Value,
    confirmed: Option<bool>,
) -> Result<crate::agent::AgentToolCall, String> {
    let servers = load_servers(&app);
    let cfg = servers
        .iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| format!("未找到 MCP 服务器：{server_id}"))?;
    // R505：调用侧必须用「原始名」（剥掉 tool_prefix）查 permission_map——
    // 前端拿到的是带前缀的注册名又原样传回，直接查映射必然落空、回退到
    // writable 推断，L3 永禁会被降级成 L0 放行。
    let raw_name = cfg.strip_tool_prefix(&name).to_string();
    let (mut perm, perm_id) = cfg.resolve_perm(&raw_name);
    // R505 不变式：服务器整体 writable=true 时，即使映射成 L0 也至少要求确认（L1）。
    if cfg.writable && perm == "L0" {
        perm = "L1".to_string();
    }
    let required_perm = perm_id.as_deref().unwrap_or("mcp.manage");
    let mut denied_reason: Option<String> = None;
    match perm.as_str() {
        "L0" => {}
        "L1" => {
            if confirmed != Some(true) {
                denied_reason = Some(
                    "mcp:confirm: 该工具为受控级别（L1），调用必须由用户明确确认（confirmed=true）。".into(),
                );
            }
        }
        "L2" => {
            if confirmed != Some(true) {
                denied_reason = Some(
                    format!("mcp:confirm: 工具 {name} 为高危级别（L2），调用必须由用户明确确认（confirmed=true）。"),
                );
            } else if !state.perm_grants.lock().unwrap().contains(required_perm) {
                denied_reason = Some(format!(
                    "mcp:denied: 未开启「{required_perm}」权限。请先在权限中心开启后重试。"
                ));
            }
        }
        "L3" => {
            denied_reason = Some(format!(
                "mcp:denied: 工具 {name} 为永禁级别（L3），不可解锁。请先在服务器配置中调整权限映射。"
            ));
        }
        _ => {}
    }
    if let Some(reason) = denied_reason {
        append_audit(
            &app,
            serde_json::json!({
                "ts": audit_ts(),
                "op": "call_tool",
                "server_id": server_id,
                "tool": name,
                "raw_tool": raw_name,
                "perm": perm,
                "granted": false,
                "args": audit_args_snapshot(&arguments),
                "ok": false,
                "error": reason,
            }),
        );
        return Err(reason);
    }

    let args_snapshot = audit_args_snapshot(&arguments);
    let args_obj = match arguments {
        serde_json::Value::Object(m) => Some(m),
        serde_json::Value::Null => None,
        _ => return Err(format!("参数必须是 JSON 对象，收到：{arguments}")),
    };

    // stdio 每次重新 spawn（阻塞式 IO），放 spawn_blocking 防卡主线程；
    // http 连接本身 async，无需阻塞线程。统一走 async 即可。
    // 端到端 60s 超时：连接 + 调用一起包——MCP 服务器挂死时宁可报「调用超时」，
    // 也不让 AI 回合永久悬挂（call 是同步 await，无超时则前端 busy 永不清除）。
    let cfg = cfg.clone();
    let call = async {
        let service = connect_server(&cfg)
            .await
            .map_err(|e| format!("连接 MCP 服务器失败：{e}"))?;
        // R505：服务器只认识原始工具名，必须传 raw_name（此前传带前缀名，
        // 配置了 tool_prefix 的服务器上调用必然失败或调错工具）。
        let mut params = CallToolRequestParams::new(raw_name.clone());
        if let Some(args) = &args_obj {
            params = params.with_arguments(args.clone());
        }
        service
            .call_tool(params)
            .await
            .map_err(|e| format!("tools/call 失败：{e}"))
    };
    let result =
        match tokio::time::timeout(std::time::Duration::from_secs(CALL_TOOL_TIMEOUT_SECS), call)
            .await
        {
            Err(_) => Err(format!(
                "调用超时（>{CALL_TOOL_TIMEOUT_SECS}s）：MCP 服务器无响应，已放弃本次调用。"
            )),
            Ok(r) => r,
        };
    let result = match result {
        Ok(r) => r,
        Err(msg) => {
            append_audit(
                &app,
                serde_json::json!({
                    "ts": audit_ts(),
                    "op": "call_tool",
                    "server_id": server_id,
                    "tool": name,
                    "raw_tool": raw_name,
                    "perm": perm,
                    "granted": true,
                    "args": args_snapshot,
                    "ok": false,
                    "error": msg,
                }),
            );
            return Err(msg);
        }
    };

    let text = result
        .content
        .into_iter()
        .filter_map(|c| match c {
            rmcp::model::ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let ok = !result.is_error.unwrap_or(false);
    append_audit(
        &app,
        serde_json::json!({
            "ts": audit_ts(),
            "op": "call_tool",
            "server_id": server_id,
            "tool": name,
            "raw_tool": raw_name,
            "perm": perm,
            "granted": true,
            "args": args_snapshot,
            "ok": ok,
            "error": if ok { serde_json::Value::Null } else { serde_json::json!(text) },
        }),
    );
    Ok(crate::agent::AgentToolCall {
        is_error: result.is_error.unwrap_or(false),
        text,
    })
}
