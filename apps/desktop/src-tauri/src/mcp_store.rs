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

/// 展示用服务器列表读取（mcp_list_servers）：**区分「首次（文件不存在）」
/// 与「读失败」**。文件不存在 = 从未配置（真空态）→ 空列表是诚实的；
/// 存在但读失败/损坏 = 必须报错，否则前端把空列表当真实状态展示
/// （读失败伪装成空列表，诚实性族规 R81 同族）。
pub(crate) fn load_servers_explicit(app: &AppHandle) -> Result<Vec<McpServerConfig>, String> {
    let Some(p) = config_path(app) else {
        return Err("无法定位数据目录".into());
    };
    read_servers_at_path(&p)
}

/// load_servers_explicit 的核心逻辑（路径参数化，便于测试）。
pub(crate) fn read_servers_at_path(p: &std::path::Path) -> Result<Vec<McpServerConfig>, String> {
    if !p.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(p).map_err(|e| format!("读取 MCP 配置失败: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("MCP 配置损坏: {e}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_read_missing_file_is_empty_not_error() {
        // 展示路径诚实性（R83 同族）：文件不存在 = 从未配置（真空态）→
        // 空列表不报错；只有文件存在但读失败/损坏才报错。
        let dir = std::env::temp_dir().join(format!("dp-mcp-explicit-{}", std::process::id()));
        let p = dir.join("mcp-servers.json");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!p.exists(), "前置：文件不存在");
        let r = read_servers_at_path(&p);
        assert!(r.is_ok(), "文件不存在必须回空而不是报错");
        assert!(r.unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn explicit_read_corrupt_file_is_error_not_empty() {
        // 文件存在但内容损坏 → 必须 Err（绝不能静默返回空列表）：
        // 否则前端把空列表当真实状态展示（读失败伪装成空列表）。
        let dir = std::env::temp_dir().join(format!("dp-mcp-corrupt-{}", std::process::id()));
        let p = dir.join("mcp-servers.json");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&p, "{ not-json !!!").unwrap();
        let r = read_servers_at_path(&p);
        assert!(r.is_err(), "损坏配置必须报错，不得静默回退空列表");
        assert!(r.unwrap_err().contains("MCP 配置损坏"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn explicit_read_valid_json_roundtrips() {
        // 正常 JSON 正常读出（真空/损坏之外还有一个方向：有效配置不被误伤）。
        let dir = std::env::temp_dir().join(format!("dp-mcp-valid-{}", std::process::id()));
        let p = dir.join("mcp-servers.json");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&p, "[]").unwrap();
        let r = read_servers_at_path(&p);
        assert!(r.is_ok(), "合法空数组是真空态，不报错");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
