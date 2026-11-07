//! 注册表索引 URL 配置（从 plugin_registry.rs 拆分，2026-10-12）。

use tauri::{AppHandle, State};

use super::{atomic_write, config_path};
use crate::plugin_registry_types::RegistryConfig;

// ── 写命令：索引 URL 配置 ──────────────────────────────────────────────

/// 设置社区索引 URL（写操作：confirmed + plugin.manage 双校验；强制 https）。
#[tauri::command]
pub(crate) async fn plugin_set_registry_url(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    url: String,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "plugin:confirm: 配置社区索引 URL 会改变插件来源，必须由用户明确确认（confirmed=true）。".into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let url = url.trim().to_string();
    let parsed = url::Url::parse(&url).map_err(|e| format!("索引 URL 不是合法地址：{e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!(
            "社区索引 URL 必须使用 https（当前：{}）。为防索引被中间人篡改，http 与自定义协议一律拒绝。",
            parsed.scheme()
        ));
    }
    // R505：字面量 + DNS 二次校验。
    if let Some(blocked) = crate::ai::ssrf_block_reason_async(parsed.as_str()).await {
        return Err(format!("社区索引 URL 不可用（{blocked}）：{url}"));
    }
    let Some(p) = config_path(&app) else {
        return Err("无法定位数据目录".into());
    };
    let cfg = RegistryConfig {
        registry_url: url.clone(),
    };
    let text = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    atomic_write(&p, &text).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "registry_url": url }))
}
