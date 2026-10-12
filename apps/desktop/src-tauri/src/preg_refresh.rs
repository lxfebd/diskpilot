//! 注册表索引刷新（从 plugin_registry.rs 拆分，2026-10-12）。
//! parse_and_verify_envelope + plugin_registry_refresh。

use tauri::{AppHandle, State};

use super::{append_ledger, atomic_write, load_registry_url, registry_path};
use crate::plugin_registry_types::{LedgerEntry, RegistryEnvelope, REGISTRY_SCHEMA};

// ── 写命令：刷新索引 ───────────────────────────────────────────────────

/// 解析 + 校验远端索引信封：JSON 格式 → schema 版本 → 非空 → 可选验签 →
/// 补上 `source_url`（实际拉取地址）与 `fetched_at`（拉取时刻）。返回校验
/// 结果 `verified`（未签名 → false，有签名验签失败 → Err，永不静默放行）。
///
/// 抽成纯函数是因为 `plugin_registry_refresh` 的这五步原本与 AppHandle 绑定，
/// 单测无法触达；契约层与实连冒烟都打这一条路径。
pub(crate) fn parse_and_verify_envelope(
    body: &str,
    source_url: &str,
    fetched_at: u64,
) -> Result<(RegistryEnvelope, bool), String> {
    let env: RegistryEnvelope =
        serde_json::from_str(body).map_err(|e| format!("索引解析失败（JSON 格式错误）：{e}"))?;
    if env.schema != REGISTRY_SCHEMA {
        return Err(format!(
            "索引 schema 版本不兼容（索引 {}，客户端 {}）。请升级 DiskPilot 或使用匹配的索引。",
            env.schema, REGISTRY_SCHEMA
        ));
    }
    if env.plugins.is_empty() {
        return Err("索引为空（plugins 数组无条目）".into());
    }
    let verified = env.verify()?;
    let env = RegistryEnvelope {
        schema: REGISTRY_SCHEMA,
        source_url: source_url.to_string(),
        fetched_at,
        signature: env.signature,
        signer: env.signer,
        plugins: env.plugins,
    };
    Ok((env, verified))
}

/// 从配置的索引 URL 拉取远端索引（写操作：confirmed + plugin.manage 双校验）。
/// 校验链：https → schema 兼容 → 可选 Ed25519 验签（失败保留旧索引）→ 原子写。
#[tauri::command]
pub(crate) async fn plugin_registry_refresh(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "plugin:confirm: 刷新索引会从网络拉取插件目录，必须由用户明确确认（confirmed=true）。"
                .into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let url = load_registry_url(&app);
    if url.trim().is_empty() {
        return Err("尚未配置社区索引 URL（设置 → 插件市场 → 索引地址）。".into());
    }
    if let Some(blocked) = crate::ai::blocked_private_target(url.trim()) {
        return Err(format!("社区索引 URL 不可用（{blocked}）：{url}"));
    }

    // SSRF 安全拉取（禁重定向跟随 + 逐跳私网校验 + 响应体上限 32MB 足矣）。
    let body = crate::net_http::ssrf_safe_get(url.trim(), 32 * 1024 * 1024, None).await?;
    let body = String::from_utf8_lossy(&body).to_string();

    // 解析 → schema 校验 → 验签 → 非空校验 → 补 source_url/fetched_at。
    // 这五步收在纯函数里（不依赖 AppHandle），单测与实连冒烟都打这一条路径。
    let (env, verified) = parse_and_verify_envelope(&body, url.trim(), super::now_secs())?;

    let Some(p) = registry_path(&app) else {
        return Err("无法定位数据目录".into());
    };
    let text = serde_json::to_string_pretty(&env).map_err(|e| e.to_string())?;
    atomic_write(&p, &text).map_err(|e| e.to_string())?;

    // 追加一条台账记录（索引刷新也可追溯）。
    let _ = append_ledger(
        &app,
        LedgerEntry {
            ts: super::now_secs(),
            kind: "index".into(),
            id: "registry".into(),
            name: "社区索引".into(),
            version: format!("{} 条", env.plugins.len()),
            source: "registry".into(),
            dir_rel: String::new(),
            blob: String::new(),
            blob_hash: String::new(),
            signer: env.signer.clone(),
            from_version: String::new(),
        },
    );

    Ok(serde_json::json!({
        "ok": true,
        "error": serde_json::Value::Null,
        "registry_url": url,
        "count": env.plugins.len(),
        "plugins": env.plugins.len(),
        "fetched_at": env.fetched_at,
        "verified": verified,
        "signed": !env.signer.trim().is_empty(),
        "source_url": env.source_url,
        "warning": serde_json::Value::Null,
    }))
}
