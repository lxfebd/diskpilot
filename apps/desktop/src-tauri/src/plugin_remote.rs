//! 插件远程下载安装（B2，2026-09-09）：从 https URL 下载插件 zip →
//! 校验（必须 https、签名强制）→ 临时文件 → 复用 install_plugin_zip 落盘。
//!
//! 与本地 `plugin_install`（zip_path）唯一区别是**来源**：URL 安装是最
//! 高危路径（包内容完全来自网络），所以：
//! 1. scheme 强制 `https`（拒绝 http/任意协议）
//! 2. 下载后走与本地包相同的安装管线（含 B1 的 digest/Ed25519 验签），
//!    但强制要求签名：`tool.plugin.json` 里没有完整 Ed25519 签名一律拒绝。
//! 3. 进度通过 `app.emit("plugin-download-progress", {url, downloaded, total, done})`
//!    实时推给前端（市场 tab 的下载条）。
//!
//! 写操作纵深防御：confirmed=true + perm_grants["plugin.manage"] 双硬校验，
//! 与 plugin_install 一致。

use std::io::Write;

use tauri::{AppHandle, Emitter, State};

/// 下载 URL 到目标路径（registry 的 blob 缓存复用）。
/// 强制 https + SSRF 私网校验（含逐跳重定向校验）；失败时删除半截文件（不留半个包）。
/// 不校验签名——调用方（registry 安装/更新）负责后续 sha256 + install_plugin_zip 验签。
pub(crate) async fn plugin_download_to_path(
    app: &AppHandle,
    url: String,
    out: &std::path::Path,
) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建缓存目录失败: {e}"))?;
    }
    let app = app.clone();
    let body = {
        let url = url.clone();
        crate::ssrf_safe_get(&url, crate::SSRF_MAX_BODY, None).await?
    };
    // 写盘 + 进度上报（256KB 阈值，与之前一致）。
    let mut f = std::fs::File::create(out).map_err(|e| format!("创建缓存文件失败: {e}"))?;
    let total = body.len() as u64;
    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;
    for chunk in body.chunks(64 * 1024) {
        if f.write_all(chunk).is_err() {
            let _ = std::fs::remove_file(out);
            return Err("写入缓存文件失败".into());
        }
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded.saturating_sub(last_emit) >= 256 * 1024 || downloaded >= total {
            last_emit = downloaded;
            let _ = app.emit(
                "plugin-download-progress",
                serde_json::json!({
                    "url": url,
                    "downloaded": downloaded,
                    "total": total,
                    "done": downloaded >= total,
                }),
            );
        }
    }
    drop(f);
    Ok(())
}

/// 从 URL 下载并安装插件 zip。成功返回安装目录相对 Tools 根路径（同本地安装）。
#[tauri::command]
pub(crate) async fn plugin_install_url(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    url: String,
    confirmed: Option<bool>,
) -> Result<String, String> {
    if confirmed != Some(true) {
        return Err(
            "plugin:confirm: 从网络下载并安装插件会把包内容解压写入 Tools 目录，必须由用户明确确认（confirmed=true）。"
                .into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    plugin_install_url_inner(app, state, url).await
}

/// URL 下载安装的核心逻辑（plugin_install_url 与 plugin_registry_install 共用）。
/// 调用方必须先做 confirmed + plugin.manage 双校验。
pub(crate) async fn plugin_install_url_inner(
    app: AppHandle,
    _state: State<'_, crate::AppState>,
    url: String,
) -> Result<String, String> {
    // 1. URL 白名单 + SSRF 私网校验（禁重定向跟随、逐跳校验）全部在
    //    ssrf_safe_get 内完成；这里直接下载到临时目录（不直接进 Tools：
    //    下载/校验失败不留半个包）。
    let tmp_dir = std::env::temp_dir().join("diskpilot_plugin_dl");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建临时下载目录失败: {e}"))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let tmp_zip = tmp_dir.join(format!("{ts}.zip"));

    // 边下载边写临时文件，每 256KB 发一次进度事件（前端 MarketPanel 进度条）。
    let app2 = app.clone();
    let url2 = url.clone();
    let body = crate::ssrf_safe_get(
        &url,
        crate::SSRF_MAX_BODY,
        Some(Box::new(move |down, total| {
            let _ = app2.emit(
                "plugin-download-progress",
                serde_json::json!({
                    "url": url2,
                    "downloaded": down,
                    "total": total,
                    "done": total > 0 && down >= total,
                }),
            );
        })),
    )
    .await?;
    std::fs::write(&tmp_zip, &body).map_err(|e| format!("写入临时文件失败: {e}"))?;
    drop(body);

    // 3. URL 来源强制要求完整 Ed25519 签名（未签名/仅 sha256 只能本地装）。
    let has_sig = diskpilot_toolbelt::signature::plugin_zip_has_ed25519(&tmp_zip)?;
    if !has_sig {
        let _ = std::fs::remove_file(&tmp_zip);
        return Err(
            "远程插件必须带 Ed25519 签名（tool.plugin.json 需含 signature+signer，verify=ed25519）。\
             未签名/仅 sha256 的包只能通过本地 zip 安装。"
                .into(),
        );
    }

    // 4. 复用本地安装管线（含 B1 验签），成功才落盘到 Tools；失败清理临时文件。
    let explicit = crate::toolbelt::toolbelt_explicit_root(&app);
    let root =
        diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())?;
    let dir_rel = {
        let zip_path = tmp_zip.clone();
        let install_res = tokio::task::spawn_blocking(move || {
            diskpilot_toolbelt::install_plugin_zip(&zip_path, &root)
        })
        .await
        .map_err(|e| format!("安装任务崩溃: {e}"))?;
        match install_res {
            Ok(rel) => rel,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_zip);
                return Err(e.to_string());
            }
        }
    };

    let _ = std::fs::remove_file(&tmp_zip);
    let _ = app.emit(
        "plugin-download-progress",
        serde_json::json!({
            "url": url,
            "done": true,
            "installed": dir_rel,
        }),
    );

    Ok(dir_rel)
}
