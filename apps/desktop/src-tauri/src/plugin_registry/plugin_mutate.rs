//! 插件注册表写操作（安装/更新/回滚 + zip 校验下载，从 plugin_registry.rs 拆分 2026-10-12）。
//!
//! 原文件保留数据层辅助 + 查询类命令；本模块收纳全部写路径：
//! install_zip_bytes / require_registry_signed / fetch_to_cache / plugin_entry_by_id
//! 与 install / update / rollback 三个 `#[tauri::command]`。
//! lib.rs 的 invoke_handler 直接注册 `plugin_registry::plugin_mutate::*`。

use tauri::{AppHandle, State};

use crate::plugin_registry::{
    append_ledger, cache_file_path, data_dir, load_envelope, load_ledger, now_secs,
    resolve_tools_root,
};
use crate::plugin_registry_types::{LedgerEntry, RegistryPlugin};

// ── 写操作：安装 / 更新 / 回滚 ──────────────────────────────────────────

/// 把 blob 缓存/下载来的 zip 安装到 Tools，返回 dir_rel。
async fn install_zip_bytes(
    zip_path: &std::path::Path,
    root: &std::path::Path,
) -> Result<String, String> {
    let zip = zip_path.to_path_buf();
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || diskpilot_toolbelt::install_plugin_zip(&zip, &root))
        .await
        .map_err(|e| format!("安装任务崩溃: {e}"))?
        .map_err(|e| e.to_string())
}

/// 强制 Ed25519 签名检查（与 URL 安装 plugin_install_url_inner 同标准，③-2）：
/// 社区注册表下载的插件包必须带完整签名；未签名/仅 sha256 的包只能
/// 本地 zip 直装。索引声明不验签（verify="none"）也在这里被拦下。
async fn require_registry_signed(zip: &std::path::Path) -> Result<(), String> {
    let has = diskpilot_toolbelt::signature::plugin_zip_has_ed25519(zip)?;
    if !has {
        let _ = std::fs::remove_file(zip);
        return Err(
            "社区注册表插件必须带 Ed25519 签名（tool.plugin.json 需含 signature+signer，verify=ed25519）。\
             未签名/仅 sha256 的包只能通过本地 zip 安装。"
                .into(),
        );
    }
    Ok(())
}

/// 下载指定版本到 blob 缓存（缓存命中直接返回；未命中走 https 下载 + sha256 校验）。
/// 返回缓存文件相对 app_data_dir 路径 + sha256。
async fn fetch_to_cache(
    app: &AppHandle,
    p: &RegistryPlugin,
    version: Option<&str>,
) -> Result<(String, String, bool), String> {
    let (ver, url, want_sha256, _signer, _size, _notes) = p.resolve(version);
    let Some(cache) = cache_file_path(app, &p.id, &ver) else {
        return Err("无法定位数据目录".into());
    };
    let Some(data) = data_dir(app) else {
        return Err("无法定位数据目录".into());
    };
    let hash_of = |path: &std::path::Path| -> Result<String, String> {
        diskpilot_toolbelt::signature::sha256_file(path).map_err(|e| e.to_string())
    };

    // 缓存命中：存在且（无期望 hash 或 hash 匹配）。
    let blob_rel = cache
        .strip_prefix(&data)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| cache.to_string_lossy().into_owned());
    if cache.is_file() {
        let got = hash_of(&cache)?;
        if want_sha256.trim().is_empty() || got.eq_ignore_ascii_case(want_sha256.trim()) {
            return Ok((blob_rel, got, true));
        }
        let _ = std::fs::remove_file(&cache); // hash 不符 → 缓存作废
    }

    // 缓存未命中 → 下载。
    if url.trim().is_empty() {
        return Err(format!("插件「{}」尚未提供下载地址（即将上架）", p.name));
    }
    crate::plugin_remote::plugin_download_to_path(app, url.clone(), &cache).await?;
    let got = hash_of(&cache)?;
    if !want_sha256.trim().is_empty() && !got.eq_ignore_ascii_case(want_sha256.trim()) {
        let _ = std::fs::remove_file(&cache);
        return Err(format!(
            "插件包 sha256 校验失败（索引 {}，实际 {}）。为防篡改，已拒绝安装。",
            want_sha256.trim(),
            got
        ));
    }
    Ok((blob_rel, got, false))
}

pub(crate) fn plugin_entry_by_id(app: &AppHandle, id: &str) -> Result<RegistryPlugin, String> {
    let (env, _) = load_envelope(app);
    env.plugins
        .into_iter()
        .find(|p| p.id == id.trim())
        .ok_or_else(|| format!("注册表里没有插件「{id}」"))
}

/// 从注册表安装指定插件（可选目标版本）。写操作：confirmed + plugin.manage 双校验。
#[tauri::command]
pub(crate) async fn plugin_registry_install(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    version: Option<String>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "plugin:confirm: 从社区注册表安装插件会下载并解压到 Tools 目录，必须由用户明确确认（confirmed=true）。"
                .into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let p = plugin_entry_by_id(&app, &id)?;
    let ver_opt = version.as_deref();
    let (ver, url, _sha256, signer, _size, _notes) = p.resolve(ver_opt);
    if url.trim().is_empty() {
        return Err(format!("插件「{}」尚未提供下载地址（即将上架）", p.name));
    }
    let (blob, blob_hash, cached) = fetch_to_cache(&app, &p, ver_opt).await?;
    let root = resolve_tools_root(&app)?;
    // blob 相对 app_data_dir；解析回绝对路径（含 plugin-cache/ 前缀）。
    let cache = data_dir(&app)
        .map(|d| d.join(&blob))
        .ok_or_else(|| "无法定位数据目录".to_string())?;
    // 社区注册表来源强制 Ed25519 签名（与 URL 安装同标准）；缓存命中也要验。
    require_registry_signed(&cache).await?;
    let dir_rel = install_zip_bytes(&cache, &root).await?;
    let _ = append_ledger(
        &app,
        LedgerEntry {
            ts: now_secs(),
            kind: "install".into(),
            id: p.id.clone(),
            name: p.name.clone(),
            version: ver.clone(),
            source: "registry".into(),
            dir_rel: dir_rel.clone(),
            blob: blob.clone(),
            blob_hash: blob_hash.clone(),
            signer: signer.clone(),
            from_version: String::new(),
        },
    );
    Ok(serde_json::json!({
        "id": p.id,
        "name": p.name,
        "version": ver,
        "dir_rel": dir_rel,
        "cached": cached,
        "blob_hash": blob_hash,
    }))
}

/// 更新已安装插件到注册表最新版本（或指定版本）。
/// 流程：备份旧目录 → 下载新包（blob 缓存）→ 安装新版 → 台账记录 from→to。
/// 写操作：confirmed + plugin.manage 双校验。
#[tauri::command]
pub(crate) async fn plugin_registry_update(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    version: Option<String>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "plugin:confirm: 更新插件会替换 Tools 目录里的插件文件，必须由用户明确确认（confirmed=true）。".into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let p = plugin_entry_by_id(&app, &id)?;
    let (target_ver, target_url, _sha, target_signer, _sz, _nt) = p.resolve(version.as_deref());
    if target_url.trim().is_empty() {
        return Err(format!("插件「{}」尚未提供下载地址（即将上架）", p.name));
    }
    // 已装版本（台账最新条目）。
    let ledger = load_ledger(&app);
    let (from_ver, dir_rel) = ledger
        .iter()
        .rev()
        .find(|e| e.id == id.trim() && !e.kind.eq("index"))
        .map(|e| (e.version.clone(), e.dir_rel.clone()))
        .ok_or_else(|| format!("插件「{id}」尚未安装（台账无记录），请先安装"))?;
    if from_ver == target_ver {
        return Err(format!("插件「{}」已是最新版本（v{target_ver}）", p.name));
    }
    let root = resolve_tools_root(&app)?;
    let dir = root.join(dir_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !dir.is_dir() {
        return Err(format!("插件目录不存在：{}", dir.display()));
    }

    // 1. 备份旧目录（打包成 zip 存进 blob 缓存，作为回滚恢复源）。
    //    备份必须在移走旧目录之前完成，否则失败时无法恢复。
    let backup_rel = format!("plugin-cache/{}-v{from_ver}-backup.zip", p.id);
    let Some(data) = data_dir(&app) else {
        return Err("无法定位数据目录".into());
    };
    let backup_path = data.join(&backup_rel);
    if let Some(parent) = backup_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    tokio::task::spawn_blocking({
        let d = dir.clone();
        let b = backup_path.clone();
        move || diskpilot_toolbelt::export_plugin_zip(&d, &b)
    })
    .await
    .map_err(|e| format!("备份任务崩溃: {e}"))?
    .map_err(|e| format!("备份旧版失败：{e}"))?;

    // 2. 下载新包到 blob 缓存。
    let (blob, blob_hash, _cached) = fetch_to_cache(&app, &p, version.as_deref()).await?;
    let cache = data.join(&blob);
    // 社区注册表来源强制 Ed25519 签名（与 URL 安装同标准）；未签名包不装，
    // 且此刻旧版目录还在原位（校验放移开之前，失败零扰动）。
    require_registry_signed(&cache).await?;

    // 3. 移开旧目录（install_plugin_zip 对已存在同名插件会报错）。
    let tmp_dir = dir.with_extension("dp-old-tmp");
    if let Err(e) = std::fs::rename(&dir, &tmp_dir) {
        return Err(format!("移开旧版目录失败：{e}"));
    }
    // 4. 安装新版；失败则移回旧目录（新版包留在缓存供下次重试）。
    let res = install_zip_bytes(&cache, &root).await;
    match res {
        Ok(new_rel) => {
            // 新版成功 → 旧目录已备份在 zip，删除临时目录。
            let _ = std::fs::remove_dir_all(&tmp_dir);
            let _ = append_ledger(
                &app,
                LedgerEntry {
                    ts: now_secs(),
                    kind: "update".into(),
                    id: p.id.clone(),
                    name: p.name.clone(),
                    version: target_ver.clone(),
                    source: "registry".into(),
                    dir_rel: new_rel.clone(),
                    blob: blob.clone(),
                    blob_hash: blob_hash.clone(),
                    signer: target_signer.clone(),
                    from_version: from_ver.clone(),
                },
            );
            Ok(serde_json::json!({
                "id": p.id,
                "from": from_ver,
                "to": target_ver,
                "dir_rel": new_rel,
                "backup": backup_rel,
            }))
        }
        Err(e) => {
            // 安装失败 → 恢复旧目录，清理临时物。
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::rename(&tmp_dir, &dir);
            Err(format!("新版安装失败，已恢复旧版：{e}"))
        }
    }
}

/// 回滚到上一版本：从台账找旧版本的 blob 备份 → 移开当前目录 → 解压备份 → 台账记录。
/// 写操作：confirmed + plugin.manage 双校验。
#[tauri::command]
pub(crate) async fn plugin_registry_rollback(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    version: Option<String>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "plugin:confirm: 回滚插件会替换 Tools 目录里的插件文件，必须由用户明确确认（confirmed=true）。".into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let ledger = load_ledger(&app);
    let cur = ledger
        .iter()
        .rev()
        .find(|e| e.id == id.trim() && !e.kind.eq("index"))
        .ok_or_else(|| format!("插件「{id}」尚未安装（台账无记录）"))?;
    // 目标版本：显式指定，或取上一条非回滚记录的版本。
    let target_ver = match version.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => v.to_string(),
        None => ledger
            .iter()
            .rev()
            .skip_while(|e| e.id != id.trim() || e.kind.eq("index"))
            .skip(1)
            .find_map(|e| {
                if e.version != cur.version {
                    Some(e.version.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| format!("插件「{id}」没有可回滚的旧版本（台账历史里没有其他版本）"))?,
    };
    let target = ledger
        .iter()
        .rev()
        .find(|e| e.id == id.trim() && e.version == target_ver)
        .ok_or_else(|| format!("台账里没有插件「{id}」的 v{target_ver} 记录"))?;
    let root = resolve_tools_root(&app)?;
    let dir = root.join(cur.dir_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !dir.is_dir() {
        return Err(format!("插件目录不存在：{}", dir.display()));
    }

    // 目标版本的 blob：优先备份包，其次注册表下载。
    let mut blob_zip: Option<std::path::PathBuf> = None;
    if let Some(data) = data_dir(&app) {
        let backup = data.join(&target.blob);
        if backup.is_file() {
            blob_zip = Some(backup);
        }
    }
    if blob_zip.is_none() {
        // 无备份 → 从注册表重新下载（blob 缓存）。重新下载的来源与全新安装
        // 同标准，强制 Ed25519 签名（备份包例外：是以前装过的版本，已验过）。
        let p = plugin_entry_by_id(&app, &id)?;
        let (blob, _h, _c) = fetch_to_cache(&app, &p, Some(&target_ver)).await?;
        let zip = data_dir(&app).map(|d| d.join(&blob));
        if let Some(z) = &zip {
            require_registry_signed(z).await?;
        }
        blob_zip = zip;
    }
    let blob_zip = blob_zip.ok_or_else(|| "无法定位回滚包".to_string())?;

    let tmp_dir: std::path::PathBuf = dir.with_extension("dp-old-tmp");
    if let Err(e) = std::fs::rename(&dir, &tmp_dir) {
        return Err(format!("移开当前版目录失败：{e}"));
    }
    let res = install_zip_bytes(&blob_zip, &root).await;
    match res {
        Ok(new_rel) => {
            let _ = std::fs::rename(&tmp_dir, &dir);
            let _ = std::fs::remove_dir_all(&dir);
            let _ = append_ledger(
                &app,
                LedgerEntry {
                    ts: now_secs(),
                    kind: "rollback".into(),
                    id: id.clone(),
                    name: target.name.clone(),
                    version: target_ver.clone(),
                    source: "registry".into(),
                    dir_rel: new_rel.clone(),
                    blob: target.blob.clone(),
                    blob_hash: target.blob_hash.clone(),
                    signer: target.signer.clone(),
                    from_version: cur.version.clone(),
                },
            );
            Ok(serde_json::json!({
                "id": id,
                "from": cur.version,
                "to": target_ver,
                "dir_rel": new_rel,
            }))
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::rename(&tmp_dir, &dir);
            Err(format!("回滚失败，已恢复当前版：{e}"))
        }
    }
}
