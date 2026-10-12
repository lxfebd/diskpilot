//! MCP 服务器增删改命令（从 mcp.rs 拆分，2026-10-12）。
//! mcp_add_server / mcp_update_server / mcp_remove_server（confirmed + mcp.manage 双校验）。

use tauri::{AppHandle, State};

use super::mcp_audit::{append_audit, audit_ts};
use super::mcp_store::{gen_id, load_servers, save_servers};
use super::{McpServerConfig, McpServerPayload, McpTransport};
use crate::mcp_bridge::validate_http_url;

/// 添加 MCP 服务器（写命令：confirmed + perm_grants["mcp.manage"] 双校验）。
#[tauri::command]
pub(crate) async fn mcp_add_server(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    payload: McpServerPayload,
    confirmed: Option<bool>,
) -> Result<McpServerConfig, String> {
    if confirmed != Some(true) {
        return Err("mcp:confirm: 添加 MCP 服务器会使其工具接入 AI 工具表，必须由用户明确确认（confirmed=true）。".into());
    }
    if !state.perm_grants.lock().unwrap().contains("mcp.manage") {
        return Err(
            "mcp:denied: 未开启「管理 MCP 服务器」权限（mcp.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    if payload.name.trim().is_empty() {
        return Err("服务器名称不能为空".into());
    }
    match &payload.transport {
        McpTransport::Stdio { command, .. } => {
            if command.trim().is_empty() {
                return Err("本地命令不能为空".into());
            }
        }
        McpTransport::Http { url, .. } => validate_http_url(url)?,
    }

    let mut servers = load_servers(&app);
    let cfg = McpServerConfig {
        id: gen_id(&app, &payload.name),
        name: payload.name.trim().to_string(),
        enabled: payload.enabled.unwrap_or(true),
        writable: payload.writable.unwrap_or(false),
        tool_prefix: payload.tool_prefix,
        permission_map: payload.permission_map,
        transport: payload.transport,
    };
    servers.push(cfg.clone());
    save_servers(&app, &servers)?;
    append_audit(
        &app,
        serde_json::json!({
            "ts": audit_ts(),
            "op": "add_server",
            "server_id": cfg.id,
            "server_name": cfg.name,
            "writable": cfg.writable,
            "ok": true,
        }),
    );
    Ok(cfg)
}

/// 编辑 MCP 服务器（写命令，双校验）。
#[tauri::command]
pub(crate) async fn mcp_update_server(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    payload: McpServerPayload,
    confirmed: Option<bool>,
) -> Result<McpServerConfig, String> {
    if confirmed != Some(true) {
        return Err(
            "mcp:confirm: 修改 MCP 服务器配置必须由用户明确确认（confirmed=true）。".into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("mcp.manage") {
        return Err(
            "mcp:denied: 未开启「管理 MCP 服务器」权限（mcp.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    if payload.name.trim().is_empty() {
        return Err("服务器名称不能为空".into());
    }
    match &payload.transport {
        McpTransport::Stdio { command, .. } => {
            if command.trim().is_empty() {
                return Err("本地命令不能为空".into());
            }
        }
        McpTransport::Http { url, .. } => validate_http_url(url)?,
    }

    let mut servers = load_servers(&app);
    let Some(cfg) = servers.iter_mut().find(|s| s.id == id) else {
        return Err(format!("未找到 MCP 服务器：{id}"));
    };
    cfg.name = payload.name.trim().to_string();
    cfg.enabled = payload.enabled.unwrap_or(true);
    cfg.writable = payload.writable.unwrap_or(false);
    cfg.tool_prefix = payload.tool_prefix;
    // None = 未编辑权限映射，保留原值（前端 toggle 开关只发 name/enabled/writable/transport，
    // 不带 permission_map 时不能清空已有映射）。
    if payload.permission_map.is_some() {
        cfg.permission_map = payload.permission_map;
    }
    cfg.transport = payload.transport;
    let cloned = cfg.clone();
    save_servers(&app, &servers)?;
    append_audit(
        &app,
        serde_json::json!({
            "ts": audit_ts(),
            "op": "update_server",
            "server_id": cloned.id,
            "server_name": cloned.name,
            "writable": cloned.writable,
            "ok": true,
        }),
    );
    Ok(cloned)
}

/// 删除 MCP 服务器（写命令，双校验；只删配置，不删任何文件）。
#[tauri::command]
pub(crate) async fn mcp_remove_server(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    confirmed: Option<bool>,
) -> Result<(), String> {
    if confirmed != Some(true) {
        return Err("mcp:confirm: 删除 MCP 服务器会使它的工具从 AI 工具表移除，必须由用户明确确认（confirmed=true）。".into());
    }
    if !state.perm_grants.lock().unwrap().contains("mcp.manage") {
        return Err(
            "mcp:denied: 未开启「管理 MCP 服务器」权限（mcp.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let mut servers = load_servers(&app);
    let before = servers.len();
    servers.retain(|s| s.id != id);
    if servers.len() == before {
        return Err(format!("未找到 MCP 服务器：{id}"));
    }
    save_servers(&app, &servers)?;
    append_audit(
        &app,
        serde_json::json!({
            "ts": audit_ts(),
            "op": "remove_server",
            "server_id": id,
            "ok": true,
        }),
    );
    Ok(())
}
