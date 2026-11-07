//! 插件注册表写操作（安装/更新/回滚 + zip 校验下载，从 plugin_registry.rs 拆分 2026-10-12）。
//!
//! 原文件保留数据层辅助 + 查询类命令；本模块收纳全部写路径：
//! install_zip_bytes / require_registry_signed / fetch_to_cache / plugin_entry_by_id
//! 与 install / update / rollback 三个 `#[tauri::command]`。
//! lib.rs 的 invoke_handler 直接注册 `plugin_registry::plugin_mutate::*`。

use tauri::{AppHandle, State};

use crate::plugin_registry::preg_misc::safe_join;
use crate::plugin_registry::{
    append_ledger, cache_file_path, data_dir, load_envelope, load_ledger, now_secs,
    resolve_tools_root,
};
use crate::plugin_registry_types::{LedgerEntry, RegistryPlugin};

/// R502：签名者锚定比对（纯函数，便于单测）。
/// 远程（注册表）来源安装时，包内 signer 必须等于索引条目声明的 expected_signer；
/// expected 为 None（URL 直装无索引声明）时只要求包内自洽签名存在。
fn check_signer_anchor(pkg_signer: &str, expected: Option<&str>) -> Result<(), String> {
    let pkg = pkg_signer.trim();
    if pkg.is_empty() {
        return Err("插件包未声明签名者（signer 空）".into());
    }
    if let Some(exp) = expected.map(str::trim).filter(|s| !s.is_empty()) {
        if !pkg.eq_ignore_ascii_case(exp) {
            return Err(format!(
                "签名者不匹配：包内 signer 与索引声明不一致（防冒名签名）。包：{pkg:.16}…，索引：{exp:.16}…"
            ));
        }
    }
    Ok(())
}

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
async fn require_registry_signed(
    zip: &std::path::Path,
    expected_signer: Option<&str>,
) -> Result<(), String> {
    let has = diskpilot_toolbelt::signature::plugin_zip_has_ed25519(zip)?;
    if !has {
        let _ = std::fs::remove_file(zip);
        return Err(
            "社区注册表插件必须带 Ed25519 签名（tool.plugin.json 需含 signature+signer，verify=ed25519）。\
             未签名/仅 sha256 的包只能通过本地 zip 安装。"
                .into(),
        );
    }
    // R502：锚定签名者——包内 signer 必须等于索引声明的 expected_signer。
    let pkg_signer = diskpilot_toolbelt::signature::plugin_zip_signer(zip)?;
    if let Err(e) = check_signer_anchor(&pkg_signer, expected_signer) {
        let _ = std::fs::remove_file(zip);
        return Err(e);
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
        let expect_ok = if want_sha256.trim().is_empty() {
            // R505：索引没给 sha256 时不能盲信半截缓存——至少要求非空 zip，
            // 签名门槛由上层 require_registry_signed 兜底。
            std::fs::metadata(&cache)
                .map(|m| m.len() > 0)
                .unwrap_or(false)
        } else {
            got.eq_ignore_ascii_case(want_sha256.trim())
        };
        if expect_ok {
            return Ok((blob_rel, got, true));
        }
        let _ = std::fs::remove_file(&cache); // hash 不符/空文件 → 缓存作废
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
    // R502：注册表来源必须在索引里声明 signer，否则无法锚定包签名、拒绝安装。
    if signer.trim().is_empty() {
        return Err(format!(
            "插件「{}」的索引条目未声明 signer，无法锚定 Ed25519 签名，拒绝安装。请联系索引维护者补全。",
            p.name
        ));
    }
    let (blob, blob_hash, cached) = fetch_to_cache(&app, &p, ver_opt).await?;
    let root = resolve_tools_root(&app)?;
    // blob 相对 app_data_dir（由 cache_file_path 净化生成，可信）；解析回绝对路径。
    let cache = data_dir(&app)
        .map(|d| d.join(&blob))
        .ok_or_else(|| "无法定位数据目录".to_string())?;
    // 社区注册表来源强制 Ed25519 签名 + 签名者锚定（与索引声明比对）；缓存命中也要验。
    require_registry_signed(&cache, Some(&signer)).await?;
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
    // R502：注册表来源必须在索引里声明 signer，否则无法锚定包签名、拒绝更新。
    if target_signer.trim().is_empty() {
        return Err(format!(
            "插件「{}」的索引条目未声明 signer，无法锚定 Ed25519 签名，拒绝更新。请联系索引维护者补全。",
            p.name
        ));
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
    // R502：台账 dir_rel 不可信（本地 JSON 可被篡改），safe_join 拒绝 ../ 与绝对路径，
    // 防止把 Tools 根之外的目录 rename/remove_dir_all 掉。
    let dir = safe_join(&root, &dir_rel)?;
    if !dir.is_dir() {
        return Err(format!("插件目录不存在：{}", dir.display()));
    }

    // 1. 备份旧目录（打包成 zip 存进 blob 缓存，作为回滚恢复源）。
    //    备份必须在移走旧目录之前完成，否则失败时无法恢复。
    //    R502：from_ver 同样来自台账，备份路径也走 safe_join 防穿越。
    let backup_rel = format!("plugin-cache/{}-v{from_ver}-backup.zip", p.id);
    let Some(data) = data_dir(&app) else {
        return Err("无法定位数据目录".into());
    };
    let backup_path = safe_join(&data, &backup_rel)?;
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
    // 社区注册表来源强制 Ed25519 签名 + 签名者锚定（与索引声明比对）；
    // 未签名/冒名包不装，且此刻旧版目录还在原位（校验放移开之前，失败零扰动）。
    require_registry_signed(&cache, Some(&target_signer)).await?;

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
    // R502：台账 dir_rel 不可信，safe_join 拒绝 ../ 与绝对路径（防越出 Tools 根删目录）。
    let dir = safe_join(&root, &cur.dir_rel)?;
    if !dir.is_dir() {
        return Err(format!("插件目录不存在：{}", dir.display()));
    }

    // 目标版本的 blob：优先备份包，其次注册表下载。
    // R502：台账 blob 字段同样不可信，非空时走 safe_join 防路径穿越。
    let mut blob_zip: Option<std::path::PathBuf> = None;
    if let Some(data) = data_dir(&app) {
        if !target.blob.trim().is_empty() {
            let backup = safe_join(&data, &target.blob)?;
            // R505：备份包也要验 blob_hash——台账被本地篡改（改包内容 +
            // 改清单 verify="none"）时，哈希不匹配就拒绝用备份，回退重下载。
            if backup.is_file() && blob_hash_matches(&backup, &target.blob_hash) {
                blob_zip = Some(backup);
            }
        }
    }
    if blob_zip.is_none() {
        // 无备份 → 从注册表重新下载（blob 缓存）。重新下载的来源与全新安装
        // 同标准，强制 Ed25519 签名 + 签名者锚定（备份包例外：是以前装过的版本，已验过）。
        let p = plugin_entry_by_id(&app, &id)?;
        let (blob, _h, _c) = fetch_to_cache(&app, &p, Some(&target_ver)).await?;
        let zip = data_dir(&app).map(|d| d.join(&blob));
        if let Some(z) = &zip {
            let expected = if target.signer.trim().is_empty() {
                None
            } else {
                Some(target.signer.as_str())
            };
            require_registry_signed(z, expected).await?;
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
            // R505 修复：成功分支应删「被回滚掉的当前版」(tmp_dir)，而不是
            // rename 回 dir 再删 dir（Windows rename 到已存在目标必败被吞，
            // 导致实际删掉刚装好的回滚版本、旧版滞留 .dp-old-tmp 丢失）。
            let _ = std::fs::remove_dir_all(&tmp_dir);
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
            // 失败分支：dir 已被移到 tmp_dir，直接移回来恢复当前版。
            let _ = std::fs::rename(&tmp_dir, &dir);
            Err(format!("回滚失败，已恢复当前版：{e}"))
        }
    }
}

/// R505：台账 blob 备份包的 sha256 与台账声明的 blob_hash 比对；
/// 声明为空时视为不可验（返回 false，调用方会回退到重新下载）。
fn blob_hash_matches(zip: &std::path::Path, expect: &str) -> bool {
    let expect = expect.trim();
    if expect.is_empty() {
        return false;
    }
    match diskpilot_toolbelt::signature::sha256_file(zip) {
        Ok(actual) => actual.eq_ignore_ascii_case(expect),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::check_signer_anchor;

    #[test]
    fn signer_anchor_matches_case_insensitive() {
        let exp = "AB".repeat(32);
        let pkg = exp.to_lowercase();
        assert!(check_signer_anchor(&pkg, Some(&exp)).is_ok());
    }

    #[test]
    fn signer_anchor_rejects_mismatch_and_empty() {
        assert!(
            check_signer_anchor("aa", Some("bb")).is_err(),
            "包 signer 与索引声明不同必须拒"
        );
        assert!(
            check_signer_anchor("", Some("bb")).is_err(),
            "包 signer 空必须拒"
        );
        assert!(
            check_signer_anchor("", None).is_err(),
            "无索引声明时包 signer 空也拒"
        );
        assert!(
            check_signer_anchor("aa", None).is_ok(),
            "URL 直装无索引声明时只要求自洽存在"
        );
        assert!(
            check_signer_anchor("aa", Some("  ")).is_ok(),
            "索引 signer 空白视为无声明"
        );
    }
}
