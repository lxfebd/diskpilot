//! Steam 库存/创意工坊/URL 跳转/中文名翻译（Tauri 命令层）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::Manager;

#[tauri::command]
pub(crate) async fn list_steam_games() -> Result<diskpilot_steam_inspector::SteamInventory, String>
{
    tokio::task::spawn_blocking(diskpilot_steam_inspector::inspect)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("steam inspect failed: {e:#}"))
}

/// Lazily enumerate every Workshop item under one game's
/// `<library>/steamapps/workshop/content/<appid>/`. Each item entry includes
/// the recursive size and folder mtime — slow enough that we do this on
/// click rather than during the bulk inspect.
#[tauri::command]
pub(crate) async fn list_steam_workshop_items(
    library_root: String,
    appid: u32,
) -> Result<Vec<diskpilot_steam_inspector::WorkshopItem>, String> {
    let path = PathBuf::from(library_root);
    tokio::task::spawn_blocking(move || {
        diskpilot_steam_inspector::list_workshop_items(&path, appid)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("workshop scan failed: {e:#}"))
}
#[tauri::command]
pub(crate) async fn fetch_workshop_titles(ids: Vec<u64>) -> Result<HashMap<u64, String>, String> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    #[derive(serde::Deserialize)]
    struct Resp {
        response: RespInner,
    }
    #[derive(serde::Deserialize)]
    struct RespInner {
        #[serde(default)]
        publishedfiledetails: Vec<Item>,
    }
    #[derive(serde::Deserialize)]
    struct Item {
        publishedfileid: String,
        result: i32,
        #[serde(default)]
        title: Option<String>,
    }

    let form = build_published_file_form(&ids);

    let client = crate::net_http::shared_http();

    let url = "https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/";
    let mut last_err: Option<String> = None;
    let mut resp_opt: Option<reqwest::Response> = None;
    for attempt in 0..2u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(800)).await;
        }
        match client.post(url).form(&form).send().await {
            Ok(r) => {
                resp_opt = Some(r);
                last_err = None;
                break;
            }
            Err(e) => {
                tracing::warn!("workshop title fetch attempt {} failed: {}", attempt + 1, e);
                last_err = Some(e.to_string());
            }
        }
    }
    let resp = resp_opt.ok_or_else(|| {
        format!(
            "Steam 服务器无响应（重试 1 次后仍失败）: {}",
            last_err.unwrap_or_else(|| "unknown".to_string())
        )
    })?;
    if !resp.status().is_success() {
        return Err(format!("Steam 服务器返回 HTTP {}", resp.status()));
    }
    let parsed: Resp = resp
        .json()
        .await
        .map_err(|e| format!("Steam 服务器响应解析失败: {e}"))?;
    let mut out: HashMap<u64, String> = HashMap::new();
    for item in parsed.response.publishedfiledetails {
        if item.result != 1 {
            continue; // 9 = deleted, 16 = banned, etc. — fall back to ID display.
        }
        if let (Ok(id), Some(title)) = (item.publishedfileid.parse::<u64>(), item.title) {
            if !title.is_empty() {
                out.insert(id, title);
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 中文名翻译（§7）——两级 fallback：本地 cache → Steam Storefront API。
//
// 设计稿 §7.1 原定的第三级「Advisor LLM fallback」随 crates/advisor 删除而
// 不可用（且命中率 <1%），按 §附录 B 决策「默认开启 + Storefront + 本地永久
// cache」落为两级。所有失败路径都静默降级：UI 显示英文名即可用，翻译是
// enhancement 不是核心路径（§7.3）。
// ---------------------------------------------------------------------------

const CACHE_FILE: &str = "steam-name-cache.json";
/// Storefront 请求失败后的冷却时长。国内直连 store.steampowered.com 经常
/// 超时（§11.6），冷却期内不再重试，避免每次打开 Inspector 都卡 12s。
const FAILURE_COOLDOWN_SECS: u64 = 24 * 60 * 60;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct CachedName {
    name_cn: String,
    #[serde(default)]
    fetched_at: u64,
    /// 非零 = 上次请求失败的时间戳，冷却期内跳过该 appid。
    #[serde(default)]
    failed_at: u64,
}

/// 翻译一批游戏为简体中文名。幂等：只返回「本地 cache 命中」的结果，
/// 未命中的 appid 会被排除，由前端在组件内再次调用进入网络 fetch 路径。
/// 这样扫描成功（列表渲染）与翻译（异步网络）天然解耦。
#[tauri::command]
pub(crate) async fn translate_steam_names(
    app: tauri::AppHandle,
    appids: Vec<u32>,
) -> Result<HashMap<u32, String>, String> {
    if appids.is_empty() {
        return Ok(HashMap::new());
    }
    let cache_path = steam_cache_path(&app);
    let now = unix_now_secs();
    let cache: HashMap<String, CachedName> = load_cache(&cache_path);
    let mut out: HashMap<u32, String> = HashMap::new();
    for id in &appids {
        match cache.get(&id.to_string()) {
            Some(c) if cache_entry_usable(c, now) && !c.name_cn.is_empty() => {
                out.insert(*id, c.name_cn.clone());
            }
            _ => {}
        }
    }
    Ok(out)
}

/// 从本地 cache 出发，对未命中的 appid 逐个调 Steam Storefront API，
/// 成功写回 cache 文件，失败标记 failed_at（冷却期内跳过）。
#[tauri::command]
pub(crate) async fn translate_steam_names_fetch(
    app: tauri::AppHandle,
    appids: Vec<u32>,
) -> Result<HashMap<u32, String>, String> {
    if appids.is_empty() {
        return Ok(HashMap::new());
    }
    let cache_path = steam_cache_path(&app);
    let now = unix_now_secs();
    let mut cache: HashMap<String, CachedName> = load_cache(&cache_path);

    let mut to_fetch = Vec::new();
    for id in &appids {
        match cache.get(&id.to_string()) {
            Some(c) if cache_entry_usable(c, now) => {}
            _ => to_fetch.push(*id),
        }
    }
    if to_fetch.is_empty() {
        return Ok(HashMap::new());
    }

    let client = crate::net_http::shared_http();
    let mut out: HashMap<u32, String> = HashMap::new();
    let mut changed = false;

    // 逐个查（Storefront 不支持批量）；一次最多跑 4 个并发，手动节流到
    // ~2 req/sec（§7.1 限流约束）。
    for chunk in to_fetch.chunks(4) {
        let mut set = tokio::task::JoinSet::new();
        for &id in chunk {
            let client = client.clone();
            set.spawn(async move {
                let url = format!(
                    "https://store.steampowered.com/api/appdetails?appids={id}&l=schinese&cc=cn"
                );
                let resp = client.get(&url).send().await.ok();
                (id, resp)
            });
        }
        while let Some(joined) = set.join_next().await {
            let (id, resp) = match joined {
                Ok(r) => r,
                Err(_) => continue,
            };
            let name = match resp {
                Some(r) if r.status().is_success() => {
                    let v: serde_json::Value = match r.json().await {
                        Ok(v) => v,
                        Err(_) => {
                            mark_failed(&mut cache, id, now);
                            changed = true;
                            continue;
                        }
                    };
                    // 结构：{"<id>": {"success": true, "data": {"name": "中文名"}}}
                    match extract_storefront_name(&v, id) {
                        Some(n) => n,
                        None => {
                            mark_failed(&mut cache, id, now);
                            changed = true;
                            continue;
                        }
                    }
                }
                _ => {
                    mark_failed(&mut cache, id, now);
                    changed = true;
                    continue;
                }
            };
            cache.insert(
                id.to_string(),
                CachedName {
                    name_cn: name.clone(),
                    fetched_at: now,
                    failed_at: 0,
                },
            );
            out.insert(id, name);
            changed = true;
        }
        // 每批之间 500ms，加上请求本身的往返，约合 2 req/sec。
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    if changed {
        save_cache(&cache_path, &cache);
    }
    Ok(out)
}

fn steam_cache_path(app: &tauri::AppHandle) -> PathBuf {
    // app_data_dir 失败时退回用户数据目录，而不是当前工作目录（"."）——
    // 工作目录兜底会污染 repo / 随机位置（S13）。
    app.path()
        .app_data_dir()
        .ok()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(CACHE_FILE)
}

fn load_cache(path: &Path) -> HashMap<String, CachedName> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_cache(path: &Path, cache: &HashMap<String, CachedName>) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(cache) {
        let _ = std::fs::write(path, text);
    }
}

fn mark_failed(cache: &mut HashMap<String, CachedName>, id: u32, now: u64) {
    cache.insert(
        id.to_string(),
        CachedName {
            name_cn: String::new(),
            fetched_at: 0,
            failed_at: now,
        },
    );
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Hand off to Steam via its custom URI scheme. Action whitelist + numeric
/// id means the URL surface can't be poisoned by arbitrary frontend
/// strings — this is the only way a Steam Inspector destructive intent
/// (uninstall) leaves the app, and it's Steam itself that runs the action.
/// `id` is appid for game actions, or workshop file id for `url/CommunityFilePage`.
/// Build a `steam://` deep link from a whitelisted action + numeric id.
/// Kept as a pure function so the whitelist is unit-testable: `None` on any
/// action that isn't explicitly allowed (S12). `id` is appid for game
/// actions, or workshop file id for `url/CommunityFilePage`.
fn build_steam_url(action: &str, appid: u64) -> Option<String> {
    let url = match action {
        "uninstall" | "rungameid" | "validate" | "nav" => format!("steam://{action}/{appid}"),
        "workshop_page" => format!("steam://url/CommunityFilePage/{appid}"),
        _ => return None,
    };
    Some(url)
}

/// Build the POST form body for GetPublishedFileDetails: `itemcount` plus
/// one `publishedfileids[i]` per id. Kept pure for tests (S12).
fn build_published_file_form(ids: &[u64]) -> Vec<(String, String)> {
    let mut form: Vec<(String, String)> = Vec::with_capacity(ids.len() + 1);
    form.push(("itemcount".to_string(), ids.len().to_string()));
    for (i, id) in ids.iter().enumerate() {
        form.push((format!("publishedfileids[{i}]"), id.to_string()));
    }
    form
}

/// Extract the Chinese name for one appid from the Storefront response JSON.
/// Shape: `{"<id>": {"success": true, "data": {"name": "中文名"}}}`. Returns
/// `None` when the entry is missing / not success / has no name (S12).
fn extract_storefront_name(v: &serde_json::Value, id: u32) -> Option<String> {
    let entry = v.get(id.to_string())?;
    if !entry
        .get("success")
        .and_then(|s| s.as_bool())
        .unwrap_or(false)
    {
        return None;
    }
    let name = entry
        .get("data")
        .and_then(|d| d.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Whether a cached name entry can satisfy a request at `now` (seconds):
/// non-empty name, or a failed attempt still inside the cooldown window.
/// Used to decide if an appid needs a network fetch (S12).
fn cache_entry_usable(c: &CachedName, now: u64) -> bool {
    if !c.name_cn.is_empty() {
        return true;
    }
    c.failed_at != 0 && now.saturating_sub(c.failed_at) < FAILURE_COOLDOWN_SECS
}

#[tauri::command]
pub(crate) fn open_steam_url(action: String, appid: u64) -> Result<(), String> {
    // Whitelist actions; `url/CommunityFilePage` is the workshop-item page.
    let url = build_steam_url(&action, appid)
        .ok_or_else(|| format!("unsupported steam action: {action}"))?;

    // 用 OS 自带的 URL scheme 打开器，不用 shell 字符串拼接：url 完全由
    // 白名单 action + 数字 id 构造，但仍避免 cmd /c 这类会做 shell 解释
    // 的通道，纵深防御（万一将来白名单被改坏也不会变成命令注入）。
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "steam_tests.rs"]
mod tests;
