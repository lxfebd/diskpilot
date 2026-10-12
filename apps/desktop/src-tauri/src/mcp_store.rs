//! MCP 服务器配置持久化（从 mcp.rs 拆分，2026-10-12）。
//! config_path / load_servers / save_servers / gen_id。

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use super::McpServerConfig;

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("mcp-servers.json"))
}

pub(crate) fn load_servers(app: &AppHandle) -> Vec<McpServerConfig> {
    let Some(p) = config_path(app) else {
        return Vec::new();
    };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn save_servers(app: &AppHandle, servers: &[McpServerConfig]) -> Result<(), String> {
    let Some(dir) = app.path().app_data_dir().ok() else {
        return Err("无法定位数据目录".into());
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(servers).map_err(|e| e.to_string())?;
    std::fs::write(config_path(app).expect("app_data_dir 已确认"), text).map_err(|e| e.to_string())
}

pub(crate) fn gen_id(app: &AppHandle, name: &str) -> String {
    let base =
        diskpilot_toolbelt::plugin_id_from_name(name).unwrap_or_else(|_| "mcp-server".to_string());
    // 重名追加数字后缀（-2 / -3 …），保证唯一。
    let existing: std::collections::HashSet<String> =
        load_servers(app).into_iter().map(|s| s.id).collect();
    let mut id = base.clone();
    let mut n = 2;
    while existing.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}
