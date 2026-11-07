//! 插件生命周期命令（从 toolbelt.rs 拆分，2026-10-12）。
//! plugin_uninstall / plugin_install / plugin_market / plugin_activate /
//! plugin_deactivate / plugin_export —— 全部 confirmed + plugin.manage 双校验。

use tauri::AppHandle;

use super::{catalog_cached, toolbelt_explicit_root};

/// 卸载插件：把工具目录移入系统回收站（可恢复），并写一条 undo 日志。
/// 只允许卸载带 `tool.plugin.json` 的插件目录；拒绝 Tools 根/分类根等
/// 非插件路径。返回回收的目录相对 Tools 根路径。
/// 写操作纵深防御：必须 confirmed=true（前端权限门 + 确认卡之后才带）。
#[tauri::command]
pub(crate) async fn plugin_uninstall(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    id: String,
    confirmed: Option<bool>,
    window: tauri::Window,
    confirm_nonce: Option<String>,
) -> Result<serde_json::Value, String> {
    crate::confirm_nonce::require_main_window(&window)?;
    crate::confirm_nonce::write_gate(
        &state.confirm_guard,
        "plugin_uninstall",
        false,
        confirmed,
        confirm_nonce.as_deref(),
    )
    .map_err(|e| format!("plugin_uninstall:confirm: {e}"))?;
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let explicit = toolbelt_explicit_root(&app);
    let root =
        diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())?;

    let catalog = catalog_cached(explicit.as_deref());
    let item = catalog
        .categories
        .iter()
        .flat_map(|c| &c.tools)
        .find(|t| t.plugin.as_ref().map(|p| p.id.as_str()) == Some(id.as_str()))
        .ok_or_else(|| format!("未找到插件「{id}」（需要目录内有 tool.plugin.json）"))?;
    let dir_rel = item.dir_rel.clone();
    let name = item.name.clone();
    let version = item
        .plugin
        .as_ref()
        .map(|p| p.version.clone())
        .unwrap_or_default();
    let dir = root.join(dir_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !dir.is_dir() {
        return Err(format!("插件目录不存在：{}", dir.display()));
    }

    let undo_log = state.undo_log.clone();
    let quarantine_root = state.quarantine_root.clone();
    let plan = diskpilot_executor::Plan {
        action: diskpilot_executor::Action::Recycle,
        paths: vec![dir],
        reason: format!("卸载插件「{name}」"),
    };
    // undo 日志与隔离区跟随桌面主进程的清理配置（同 cleanup 命令），
    // 保证用户能在「最近清理/撤销」里恢复。
    let out = tokio::task::spawn_blocking(move || {
        diskpilot_executor::execute(&plan, false, &undo_log, &quarantine_root)
    })
    .await
    .map_err(|e| format!("卸载任务崩溃: {e}"))?
    .map_err(|e| format!("卸载失败：{e}"))?;

    // 台账补一条 uninstall 事件：installed_plugin_ids 按 install/update 聚合，
    // 不写的话已卸载插件会永久显示「已安装」，社区列表无法重装。
    crate::plugin_registry::append_uninstall_ledger(&app, &id, &name, &dir_rel, &version);

    Ok(serde_json::json!({
        "id": id,
        "name": name,
        "dir_rel": dir_rel,
        "recycled": out.iter().map(|u| u.source.display().to_string()).collect::<Vec<_>>(),
    }))
}

/// 从本地 `.zip` 插件包安装到 Tools 根：解析包内 tool.plugin.json 的
/// id/category，解压到 `<分类>/<id>`，返回安装目录相对 Tools 根路径。
/// 已存在同名插件时报错（先卸载再重装，卸载走回收站可恢复）。
/// 写操作纵深防御：必须 confirmed=true（zip 包内容会解压写入磁盘）。
#[tauri::command]
pub(crate) async fn plugin_install(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    zip_path: String,
    confirmed: Option<bool>,
    window: tauri::Window,
    confirm_nonce: Option<String>,
) -> Result<String, String> {
    crate::confirm_nonce::require_main_window(&window)?;
    crate::confirm_nonce::write_gate(
        &state.confirm_guard,
        "plugin_install",
        false,
        confirmed,
        confirm_nonce.as_deref(),
    )
    .map_err(|e| format!("plugin_install:confirm: {e}"))?;
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let explicit = toolbelt_explicit_root(&app);
    let root =
        diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())?;
    let zip = std::path::PathBuf::from(zip_path);
    if !zip.is_file() {
        return Err(format!("插件包不存在：{}", zip.display()));
    }
    let dir_rel =
        tokio::task::spawn_blocking(move || diskpilot_toolbelt::install_plugin_zip(&zip, &root))
            .await
            .map_err(|e| format!("安装任务崩溃: {e}"))?
            .map_err(|e| e.to_string())?;
    Ok(dir_rel)
}

/// 内置插件市场索引：把 toolbelt 收录的 CLI 工具当「官方插件目录」，
/// 对比工具墙 catalog 标出每个插件 id / 是否已插件化（目录内有
/// tool.plugin.json）/ 名称 / 分类 / 用途 / 风险 / 权限级。
#[tauri::command]
pub(crate) fn plugin_market(app: AppHandle) -> Vec<serde_json::Value> {
    let explicit = toolbelt_explicit_root(&app);
    let catalog = catalog_cached(explicit.as_deref());
    let installed_ids: std::collections::HashSet<String> = catalog
        .categories
        .iter()
        .flat_map(|c| &c.tools)
        .filter_map(|t| t.plugin.as_ref().map(|p| p.id.clone()))
        .collect();
    diskpilot_toolbelt::all_manifests()
        .iter()
        .map(|m| {
            // id 必须是合法 kebab-case（validate_plugin_id 约束）：
            // 旧的 to_lowercase().replace(' ','-') 会产生 `_`/非 ASCII 产出，
            // 与插件安装/AI 工具注册的 id 契约不兼容；统一走净化函数。
            let id = diskpilot_toolbelt::plugin_id_from_name(&m.name)
                .unwrap_or_else(|_| format!("t-{}", m.name.len()));
            serde_json::json!({
                "id": id,
                "name": m.name,
                "category": m.category,
                "purpose": m.purpose,
                "risk": m.risk,
                "permission_level": m.permission_level,
                "publisher": m.publisher,
                "exe_rel": m.exe_rel,
                "installed": installed_ids.contains(&id),
            })
        })
        .collect()
}

/// 市场「安装」= 给 Tools 里已存在的工具目录补写 tool.plugin.json
/// （工具本体已在工具箱内，插件化清单让工具墙/权限/AI 把它当插件管理）。
/// 工具目录取 manifest 的 exe_rel 父目录；已插件化则报错（避免覆盖）。
/// 写操作纵深防御：必须 confirmed=true。
#[tauri::command]
pub(crate) fn plugin_activate(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    tool: String,
    confirmed: Option<bool>,
    window: tauri::Window,
    confirm_nonce: Option<String>,
) -> Result<String, String> {
    crate::confirm_nonce::require_main_window(&window)?;
    crate::confirm_nonce::write_gate(
        &state.confirm_guard,
        "plugin_activate",
        false,
        confirmed,
        confirm_nonce.as_deref(),
    )
    .map_err(|e| format!("plugin_activate:confirm: {e}"))?;
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let explicit = toolbelt_explicit_root(&app);
    let root =
        diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())?;
    let m = diskpilot_toolbelt::find_manifest(&tool)
        .ok_or_else(|| format!("市场里没有「{tool}」（内置 CLI 工具清单未收录）"))?;
    let exe = root.join(m.exe_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let dir = exe
        .parent()
        .ok_or_else(|| "manifest 路径无父目录".to_string())?;
    let plugin_file = dir.join("tool.plugin.json");
    if plugin_file.exists() {
        return Err(format!(
            "「{}」已经是插件（{} 已存在），无需重复安装",
            m.name,
            plugin_file.display()
        ));
    }
    let id = m.name.to_ascii_lowercase().replace([' ', '/'], "-");
    let meta = serde_json::json!({
        "schema": 1,
        "id": id,
        "name": m.name,
        "version": "1.0",
        "author": m.publisher,
        "description": m.purpose,
        "entry": exe.file_name().and_then(|s| s.to_str()).unwrap_or(""),
        "category": m.category,
        "risk": m.risk,
        "permissions": ["run"],
    });
    // 原子写（先 .tmp 再 rename）：中途崩溃不留下半截 JSON，与 registry 侧
    // atomic_write 同标准——半截清单会被 plugin loader 误读成损坏插件。
    crate::plugin_registry::atomic_write(
        &plugin_file,
        &serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写入插件清单失败: {e}"))?;
    Ok(plugin_file.display().to_string())
}

/// 市场「卸载」= 移除 tool.plugin.json（工具本体保留，只是不再是插件）。
/// 与 plugin_uninstall（整个目录回收）不同：这里只摘掉插件身份。
/// 写操作纵深防御：必须 confirmed=true（同 plugin_install/uninstall 一致性）。
#[tauri::command]
pub(crate) fn plugin_deactivate(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    tool: String,
    confirmed: Option<bool>,
    window: tauri::Window,
    confirm_nonce: Option<String>,
) -> Result<String, String> {
    crate::confirm_nonce::require_main_window(&window)?;
    crate::confirm_nonce::write_gate(
        &state.confirm_guard,
        "plugin_deactivate",
        false,
        confirmed,
        confirm_nonce.as_deref(),
    )
    .map_err(|e| format!("plugin_deactivate:confirm: {e}"))?;
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let explicit = toolbelt_explicit_root(&app);
    let root =
        diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())?;
    let m =
        diskpilot_toolbelt::find_manifest(&tool).ok_or_else(|| format!("市场里没有「{tool}」"))?;
    let exe = root.join(m.exe_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let dir = exe
        .parent()
        .ok_or_else(|| "manifest 路径无父目录".to_string())?;
    let plugin_file = dir.join("tool.plugin.json");
    if !plugin_file.is_file() {
        return Err(format!(
            "「{}」本来就不是插件（无 {}）",
            m.name,
            plugin_file.display()
        ));
    }
    std::fs::remove_file(&plugin_file).map_err(|e| format!("移除插件清单失败: {e}"))?;
    Ok(plugin_file.display().to_string())
}

/// 把已安装插件（目录含 tool.plugin.json）打包成可分发的 zip——插件分发的
/// 「打包导出」半边：装得了、卸得掉、能市场化，现在也能整包发出去在别的
/// 机器/分类树上直接安装。按插件 id 定位（同 plugin_uninstall），目标
/// zip 已存在时覆盖。out_zip 为空时自动放到 Tools 根下
/// `<id>-v<version>.zip`（AI 通道没给路径也能导出）。
/// 写操作纵深防御：必须 confirmed=true（会写 zip 文件）。
#[tauri::command]
pub(crate) async fn plugin_export(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    id: String,
    out_zip: String,
    confirmed: Option<bool>,
    window: tauri::Window,
    confirm_nonce: Option<String>,
) -> Result<serde_json::Value, String> {
    crate::confirm_nonce::require_main_window(&window)?;
    crate::confirm_nonce::write_gate(
        &state.confirm_guard,
        "plugin_export",
        false,
        confirmed,
        confirm_nonce.as_deref(),
    )
    .map_err(|e| format!("plugin_export:confirm: {e}"))?;
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let explicit = toolbelt_explicit_root(&app);
    let root =
        diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())?;

    let catalog = catalog_cached(explicit.as_deref());
    let item = catalog
        .categories
        .iter()
        .flat_map(|c| &c.tools)
        .find(|t| t.plugin.as_ref().map(|p| p.id.as_str()) == Some(id.as_str()))
        .ok_or_else(|| format!("未找到插件「{id}」（需要目录内有 tool.plugin.json）"))?;
    let dir_rel = item.dir_rel.clone();
    let name = item.name.clone();
    let dir = root.join(dir_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !dir.is_dir() {
        return Err(format!("插件目录不存在：{}", dir.display()));
    }
    let out = if out_zip.trim().is_empty() {
        let ver = item
            .plugin
            .as_ref()
            .map(|p| p.version.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "0".to_string());
        root.join(format!("{id}-v{ver}.zip"))
    } else {
        // 非空 out_zip：必须落到 Tools 根内（含根下任意子目录），杜绝把插件
        // 包任意写到用户磁盘任意位置/覆盖任意文件。相对路径按 Tools 根解析，
        // 越出根或含穿越段一律拒绝。
        if out_zip.contains("..") {
            return Err(format!("导出目标不允许含 `..` 路径穿越：{out_zip}"));
        }
        let raw = std::path::PathBuf::from(&out_zip);
        let abs = if raw.is_absolute() {
            raw
        } else {
            root.join(&raw)
        };
        if !abs.starts_with(&root) {
            return Err(format!(
                "导出目标必须在 Tools 根内（不允许写到外部）：{}",
                out_zip
            ));
        }
        abs
    };
    if out.is_dir() {
        return Err(format!(
            "导出目标必须是文件路径（不能是目录）：{}",
            out.display()
        ));
    }
    let dir2 = dir.clone();
    let out2 = out.clone();
    tokio::task::spawn_blocking(move || diskpilot_toolbelt::export_plugin_zip(&dir2, &out2))
        .await
        .map_err(|e| format!("导出任务崩溃: {e}"))?
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "id": id,
        "name": name,
        "dir_rel": dir_rel,
        "zip": out.display().to_string(),
    }))
}
