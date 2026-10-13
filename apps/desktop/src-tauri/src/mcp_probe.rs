//! MCP 服务器探测（从 mcp.rs 拆分，2026-10-12）。
//! probe 缓存 + mcp_list_servers / probe_tools / mcp_test_server。

use tauri::AppHandle;

use super::mcp_audit::audit_ts;
use super::mcp_store::load_servers;
use super::{McpServerConfig, McpServerStatus, McpToolMeta, McpTransport};
use super::{PROBE_TIMEOUT_SECS, PROBE_TTL_SECS};
use crate::mcp_bridge::connect_server;

/// probe 结果缓存条目：(探测时间戳, 工具数, 错误)。
type ProbeResult = (u64, Option<usize>, Option<String>);

/// probe 结果缓存：server_id → ProbeResult。
/// 5s TTL：5s 内重复 list_servers/list_tools 命中缓存免重连（对齐 hw 5s 轮询）。
fn probe_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, ProbeResult>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, ProbeResult>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 探测某服务器并写缓存（带 10s 全局超时，stdio 无 timeout 时兜底）。
/// `refresh=true` 时强制重连（手动「测试连接」）。
/// 返回 (工具数, 错误)。缓存命中且未超时且未强制刷新 → 直接回缓存。
async fn probe_and_cache(cfg: &McpServerConfig, refresh: bool) -> (Option<usize>, Option<String>) {
    let now = audit_ts();
    if !refresh {
        if let Some((at, count, err)) = probe_cache().lock().unwrap().get(&cfg.id).cloned() {
            if now.saturating_sub(at) < PROBE_TTL_SECS {
                return (count, err);
            }
        }
    }
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(PROBE_TIMEOUT_SECS),
        probe_tools(cfg),
    )
    .await;
    let (count, err) = match result {
        Err(_) => (None, Some(format!("连接超时（>{}s）", PROBE_TIMEOUT_SECS))),
        Ok(Ok(tools)) => (Some(tools.len()), None),
        Ok(Err(e)) => (None, Some(e)),
    };
    probe_cache()
        .lock()
        .unwrap()
        .insert(cfg.id.clone(), (now, count, err.clone()));
    (count, err)
}

/// 服务器列表 + 每个 enabled 服务器的连接健康探测（只读 L0）。
/// 配置读失败（存在但损坏）报错：前端显示「未读到」而非空列表（诚实性族规）。
#[tauri::command]
pub(crate) async fn mcp_list_servers(app: AppHandle) -> Result<Vec<McpServerStatus>, String> {
    let servers = super::mcp_store::load_servers_explicit(&app)?;
    let mut out = Vec::with_capacity(servers.len());
    for s in &servers {
        let transport_type = match &s.transport {
            McpTransport::Stdio { .. } => "stdio",
            McpTransport::Http { .. } => "http",
        }
        .to_string();
        // 权限映射摘要：统计映射的级别分布
        let perm_scope = match &s.permission_map {
            Some(m) if !m.is_empty() => {
                let mut lv: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for v in m.values() {
                    let up = v.trim().to_ascii_uppercase();
                    *lv.entry(if matches!(up.as_str(), "L0" | "L1" | "L2" | "L3") {
                        up
                    } else {
                        "L2".into()
                    })
                    .or_insert(0) += 1;
                }
                lv.into_iter()
                    .filter(|(_, n)| *n > 0)
                    .map(|(k, n)| format!("{n} 个 {k}"))
                    .collect::<Vec<_>>()
                    .join(" / ")
            }
            _ if s.writable => "全部 L2".to_string(),
            _ => "全部 L0".to_string(),
        };
        let mut status = McpServerStatus {
            id: s.id.clone(),
            name: s.name.clone(),
            enabled: s.enabled,
            writable: s.writable,
            tool_prefix: s.tool_prefix.clone(),
            transport_type,
            transport: s.transport.clone(),
            tool_count: None,
            error: None,
            perm_scope,
            permission_map: s.permission_map.clone(),
        };
        if s.enabled {
            let (count, err) = probe_and_cache(s, false).await;
            status.tool_count = count;
            status.error = err;
        }
        out.push(status);
    }
    Ok(out)
}

/// 探测某服务器：连接 + tools/list，返回工具清单（只读，无缓存；
/// 供 probe_and_cache 内部调用与 mcp_test_server 用）。
pub(crate) async fn probe_tools(cfg: &McpServerConfig) -> Result<Vec<rmcp::model::Tool>, String> {
    let service = connect_server(cfg).await?;
    let tools = service
        .list_all_tools()
        .await
        .map_err(|e| format!("tools/list 失败：{e}"))?;
    Ok(tools)
}

/// 测试单个服务器：连接 + list_tools，返回工具清单（只读 L0，前端「测试连接」按钮）。
/// 强制刷新（不走 5s 缓存，用户点测试就是要最新）。
#[tauri::command]
pub(crate) async fn mcp_test_server(
    app: AppHandle,
    id: String,
) -> Result<Vec<McpToolMeta>, String> {
    let servers = load_servers(&app);
    let cfg = servers
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("未找到 MCP 服务器：{id}"))?;
    let (count, err) = probe_and_cache(cfg, true).await;
    if let Some(e) = err {
        return Err(e);
    }
    let Some(_) = count else {
        return Err("探测无结果".into());
    };
    let tools = probe_tools(cfg).await?;
    Ok(tools
        .into_iter()
        .map(|t| {
            let raw = t.name.to_string();
            let (perm, _) = cfg.resolve_perm(&raw);
            McpToolMeta {
                server_id: cfg.id.clone(),
                name: cfg.tool_name(&raw),
                description: t.description.unwrap_or_default().to_string(),
                input_schema: serde_json::Value::Object(t.input_schema.as_ref().clone()),
                writable: cfg.writable,
                perm,
            }
        })
        .collect())
}
