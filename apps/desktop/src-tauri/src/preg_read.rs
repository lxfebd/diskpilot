//! 注册表只读命令（从 plugin_registry.rs 拆分，2026-10-12）。
//! 列表 / 搜索 / 校验 / 已装清单 / 配置读取。

use tauri::AppHandle;

use super::load_ledger;
use crate::plugin_registry_types::{InstalledPlugin, RegistryPlugin};

// ── 只读命令 ───────────────────────────────────────────────────────────

/// 兼容旧调用：直接读配置文件；这是唯一读索引 URL 的入口。
pub(crate) fn load_registry_url(app: &AppHandle) -> String {
    super::load_config(app).registry_url
}

/// 内置工具目录的插件 id 集合（复用 toolbelt 的 manifest 清单：plugin 元数据 id；
/// 无插件元数据的传统工具按名字净化——与内置市场同一口径）。社区列表据此给
/// 「与内置重复」的条目打 `builtin` 标记，避免用户装一个已经内置的插件。
/// 统一存小写：filter_builtin_dupes 比较时两端都折叠大小写，防远端大小写变体漏剔。
fn builtin_plugin_ids() -> std::collections::HashSet<String> {
    diskpilot_toolbelt::all_manifests()
        .iter()
        .filter_map(|m| {
            diskpilot_toolbelt::plugin_id_from_name(&m.name)
                .ok()
                .map(|id| id.to_ascii_lowercase())
        })
        .collect()
}

/// 剔除与内置重复的条目，返回保留条目 + 剔除数。纯函数便于单测。
/// 两端 id 大小写不敏感：远端索引若发 `CRYSTALDISKINFO` 也应命中内置
/// `crystaldiskinfo`，避免「与内置重复的插件还能装」的漏剔。
pub(crate) fn filter_builtin_dupes(
    plugins: Vec<RegistryPlugin>,
    builtin_ids: &std::collections::HashSet<String>,
) -> (Vec<RegistryPlugin>, usize) {
    let mut kept = Vec::with_capacity(plugins.len());
    let mut skipped = 0usize;
    for p in plugins {
        let name_id = diskpilot_toolbelt::plugin_id_from_name(&p.name).unwrap_or_default();
        let builtin = builtin_ids.contains(p.id.to_ascii_lowercase().as_str())
            || builtin_ids.contains(&name_id);
        if builtin {
            skipped += 1;
        } else {
            kept.push(p);
        }
    }
    (kept, skipped)
}

/// 社区列表去重后的条目（已装标记 + 与内置重复的条目被剔除）+ 剔除计数。
pub(crate) fn dedup_plugins(
    app: &AppHandle,
    plugins: Vec<RegistryPlugin>,
) -> (Vec<serde_json::Value>, usize) {
    let builtin_ids = builtin_plugin_ids();
    let installed_ids = installed_plugin_ids(app);
    let (kept, skipped) = filter_builtin_dupes(plugins, &builtin_ids);
    let items = kept
        .into_iter()
        .map(|p| {
            let installed = installed_ids.contains(&p.id);
            let mut v = serde_json::to_value(&p).unwrap_or(serde_json::Value::Null);
            if let serde_json::Value::Object(m) = &mut v {
                m.insert("builtin".into(), serde_json::json!(false));
                m.insert("installed".into(), serde_json::json!(installed));
            }
            v
        })
        .collect();
    (items, skipped)
}

/// 台账里当前「已安装」的插件 id 集合：按每条事件正序回放，install/update 标记已装、
/// uninstall 移除——以最新事件状态为准（只聚合 install/update 会同 ③-3 审计一样
/// 让已卸载插件永久标记为已安装）。
fn installed_plugin_ids(app: &AppHandle) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    for e in load_ledger(app).iter().filter(|e| e.kind != "index") {
        match e.kind.as_str() {
            "install" | "update" => {
                ids.insert(e.id.clone());
            }
            "uninstall" => {
                ids.remove(&e.id);
            }
            _ => {}
        }
    }
    ids
}

/// 社区注册表列表（只读 L0）。返回信封 + 条目 + 索引 URL 配置状态。
/// 与内置重复的条目被剔除（`skipped_builtin` 计数），已安装的条目带 `installed` 标记。
#[tauri::command]
pub(crate) fn plugin_registry_list(app: AppHandle) -> serde_json::Value {
    let (env, warn) = super::load_envelope(&app);
    let url = load_registry_url(&app);
    let verified = if env.signature.trim().is_empty() {
        false
    } else {
        env.verify().unwrap_or(false)
    };
    let (items, skipped) = dedup_plugins(&app, env.plugins);
    serde_json::json!({
        "items": items,
        "skipped_builtin": skipped,
        "warning": warn,
        "updated_at": super::now_secs(),
        "schema": env.schema,
        "source_url": env.source_url,
        "fetched_at": env.fetched_at,
        "signed": !env.signature.trim().is_empty() && !env.signer.trim().is_empty(),
        "verified": verified,
        "signer": env.signer,
        "registry_url": url,
        "indexed": !url.trim().is_empty(),
    })
}

/// 社区注册表搜索（只读 L0）：按名称/描述/标签/分类/作者关键字过滤。
/// 与内置重复的条目同样被剔除。
#[tauri::command]
pub(crate) fn plugin_registry_search(app: AppHandle, q: String) -> serde_json::Value {
    let (env, warn) = super::load_envelope(&app);
    let kw = q.trim().to_ascii_lowercase();
    let filtered: Vec<RegistryPlugin> = if kw.is_empty() {
        env.plugins
    } else {
        env.plugins
            .into_iter()
            .filter(|p| {
                p.name.to_ascii_lowercase().contains(&kw)
                    || p.description.to_ascii_lowercase().contains(&kw)
                    || p.category.to_ascii_lowercase().contains(&kw)
                    || p.author.to_ascii_lowercase().contains(&kw)
                    || p.tags.iter().any(|t| t.to_ascii_lowercase().contains(&kw))
            })
            .collect()
    };
    let (items, skipped) = dedup_plugins(&app, filtered);
    serde_json::json!({
        "items": items,
        "skipped_builtin": skipped,
        "warning": warn,
        "query": q,
        "schema": env.schema,
        "fetched_at": env.fetched_at,
        "source_url": env.source_url,
        "registry_url": load_registry_url(&app),
        "indexed": !load_registry_url(&app).trim().is_empty(),
    })
}

/// 校验索引签名与条目字段完备性（只读 L0）。
#[tauri::command]
pub(crate) fn plugin_registry_verify(app: AppHandle) -> serde_json::Value {
    let (env, warn) = super::load_envelope(&app);
    let mut items = Vec::with_capacity(env.plugins.len());
    for p in &env.plugins {
        let (_, url, sha256, signer, _, _) = p.resolve(None);
        items.push(serde_json::json!({
            "id": p.id,
            "name": p.name,
            "version": p.version,
            "has_url": !url.trim().is_empty(),
            "https_url": url.trim().starts_with("https://"),
            "has_sha256": !sha256.trim().is_empty(),
            "has_signer": !signer.trim().is_empty(),
            "verified": p.verified,
            "issue": if url.trim().is_empty() {
                Some("尚未提供下载地址（即将上架）")
            } else if !url.trim().starts_with("https://") {
                Some("下载地址必须 https")
            } else if signer.trim().is_empty() {
                Some("未提供签名者公钥（远程安装要求 Ed25519 签名）")
            } else {
                None
            },
        }));
    }
    let signed = !env.signature.trim().is_empty() && !env.signer.trim().is_empty();
    // R503：验签只算一次（Ed25519 验签有开销），结果复用到 ok/error 两个字段。
    let verify_res: Option<Result<bool, String>> = if signed { Some(env.verify()) } else { None };
    let envelope_ok = verify_res.as_ref().map(|r| r.is_ok()).unwrap_or(false);
    let envelope_error = match &verify_res {
        Some(Ok(_)) => None,
        Some(Err(e)) => Some(e.clone()),
        None => Some("索引未签名".into()),
    };
    serde_json::json!({
        "signed": signed,
        "envelope_ok": envelope_ok,
        "envelope_error": envelope_error,
        "schema": env.schema,
        "fetched_at": env.fetched_at,
        "items": items,
        "warning": warn,
    })
}

/// 已安装插件清单（只读 L0）：按台账最新条目聚合，带可回滚历史。
/// `index` 事件（索引刷新，id 固定 "registry"）不是插件，直接跳过；
/// `uninstall` 事件是终止态——该插件已不在本地，从清单里移除。
#[tauri::command]
pub(crate) fn plugin_list_installed(app: AppHandle) -> serde_json::Value {
    let ledger = load_ledger(&app);
    let mut by_id: std::collections::BTreeMap<String, InstalledPlugin> =
        std::collections::BTreeMap::new();
    for e in ledger.iter().rev() {
        if e.kind == "index" {
            continue;
        }
        if e.kind == "uninstall" {
            by_id.remove(&e.id);
            continue;
        }
        match by_id.entry(e.id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(InstalledPlugin {
                    id: e.id.clone(),
                    name: e.name.clone(),
                    version: e.version.clone(),
                    source: e.source.clone(),
                    dir_rel: e.dir_rel.clone(),
                    installed_at: e.ts,
                    signer: e.signer.clone(),
                    blob: e.blob.clone(),
                    blob_hash: e.blob_hash.clone(),
                    history: Vec::new(),
                });
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                if let Some(cur) = by_id.get_mut(&e.id) {
                    cur.history.push(e.clone());
                }
            }
        }
    }
    serde_json::json!({
        "installed": by_id.values().collect::<Vec<_>>(),
        "total_events": ledger.len(),
    })
}

/// 索引 URL 配置读取（只读 L0）。
#[tauri::command]
pub(crate) fn plugin_registry_config(app: AppHandle) -> serde_json::Value {
    serde_json::json!({ "registry_url": load_registry_url(&app) })
}
