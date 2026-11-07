//! 图吧工具箱 CLI/GUI 接入门面（Tauri 命令层）。
//!
//! 2026-10-12 再拆分：插件生命周期命令 → `toolbelt_plugin`（卸载/安装/市场/
//! 激活/停用/导出）。本文件保留 catalog 缓存、启动/运行/回收与工具墙命令。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::AppHandle;

use crate::general_tray::general_config_at;

#[path = "toolbelt_plugin.rs"]
pub(crate) mod toolbelt_plugin;

/// 工具墙 catalog 缓存：`catalog_tree` 全目录扫描 + 每目录抽取 exe 图标较重
/// （实测 ~百 ms~秒级），同一会话内反复切工具墙直接命中缓存秒回；TTL 过期后
/// 自动重扫，捕捉用户新增/删除工具的变化。key = Tools 根路径（未指定用 "auto"）。
struct CatalogCacheEntry {
    key: String,
    at: Instant,
    value: diskpilot_toolbelt::ToolbeltCatalog,
}
static CATALOG_CACHE: OnceLock<Mutex<Option<CatalogCacheEntry>>> = OnceLock::new();
const CATALOG_TTL: Duration = Duration::from_secs(60);

fn catalog_cached(explicit: Option<&std::path::Path>) -> diskpilot_toolbelt::ToolbeltCatalog {
    let key = explicit
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "auto".to_string());
    let lock = CATALOG_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = lock.lock() {
        if let Some(e) = guard.as_ref() {
            if e.key == key && e.at.elapsed() < CATALOG_TTL {
                return e.value.clone();
            }
        }
        let v = diskpilot_toolbelt::catalog_tree(explicit);
        *guard = Some(CatalogCacheEntry {
            key,
            at: Instant::now(),
            value: v.clone(),
        });
        v
    } else {
        diskpilot_toolbelt::catalog_tree(explicit)
    }
}

/// 目标 exe 的 manifest 声明 requireAdministrator（磁盘工具/刷写工具常见）时，
/// 普通 CreateProcess 会返回 ERROR_ELEVATION_REQUIRED（os error 740）——不是
/// 程序坏了，是 Windows 要求先提权。这里用 ShellExecuteExW 的 `runas` 动词
/// 拉起 UAC 提示，用户同意后以管理员权限启动。
#[cfg(windows)]
fn launch_with_runas(exe: &std::path::Path, cwd: &std::path::Path) -> Result<(), std::io::Error> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };

    let wide = |s: &OsStr| -> Vec<u16> { s.encode_wide().chain(std::iter::once(0)).collect() };
    let verb = wide(OsStr::new("runas"));
    let file = wide(exe.as_os_str());
    let dir = wide(cwd.as_os_str());
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = std::ptr::null();
    info.lpDirectory = dir.as_ptr();
    info.nShow = 1; // SW_SHOWNORMAL
    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    if !info.hProcess.is_null() {
        unsafe { CloseHandle(info.hProcess) };
    }
    Ok(())
}
pub(crate) fn toolbelt_explicit_root(app: &AppHandle) -> Option<std::path::PathBuf> {
    general_config_at(app)
        .tools_root
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
}

/// 工具箱状态：Tools 根是否找到 + 每个 CLI 工具的就位情况。
#[tauri::command]
pub(crate) fn toolbelt_status(app: AppHandle) -> diskpilot_toolbelt::ToolbeltStatus {
    diskpilot_toolbelt::status(toolbelt_explicit_root(&app).as_deref())
}

/// 全目录扫描（缝口 A）：整个 Tools/ 根的工具墙——17 个 CLI 白名单之外的所有
/// 图形工具都列出来，带主启动文件 / 架构变体 / link.json 软链 / tools.json 元数据。
#[tauri::command]
pub(crate) fn toolbelt_catalog(app: AppHandle) -> diskpilot_toolbelt::ToolbeltCatalog {
    catalog_cached(toolbelt_explicit_root(&app).as_deref())
}

/// 单工具完整用法（参数表/示例 + 解析出的 exe 绝对路径）。
/// AI/前端在执行前必须先读它——同 tubatools 的 get_cli_tool_usage。
#[tauri::command]
pub(crate) fn toolbelt_usage(app: AppHandle, tool: String) -> Result<serde_json::Value, String> {
    let root = diskpilot_toolbelt::find_tools_root(toolbelt_explicit_root(&app).as_deref())
        .map_err(|e| e.to_string())?;
    let entry =
        diskpilot_toolbelt::find(&tool).ok_or_else(|| format!("未找到 CLI 工具「{tool}」"))?;
    let exe = entry
        .exe_rel
        .as_ref()
        .map(|rel| {
            root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
                .display()
                .to_string()
        })
        .unwrap_or_else(|| "（文档未收录路径）".into());
    Ok(serde_json::json!({
        "tool": entry.name,
        "category": entry.category,
        "exe": exe,
        "risk": entry.risk,
        "usage": entry.detail,
    }))
}

#[derive(serde::Serialize)]
pub(crate) struct ToolbeltRunReport {
    /// 实际执行的完整命令（展示/审计用）。
    command_line: String,
    risk: diskpilot_toolbelt::Risk,
    outcome: diskpilot_toolbelt::RunOutcome,
}

/// 执行工具箱 CLI 工具。medium/high 风险必须 confirmed=true（前端弹确认，
/// AI 通道拿到用户同意后再置位）——同 toolbelt.toml 的「medium 及以上需确认」。
/// 纵深防御：即使绕过前端直接 invoke，`cli.run` 权限未开启也会被后端拒绝
/// （前端 execCliTool 同款校验，双保险——权限中心关闭时任何调用路径都执行不了）。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn toolbelt_run(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    tool: String,
    args: Vec<String>,
    timeout_secs: Option<u64>,
    confirmed: Option<bool>,
    window: tauri::Window,
    confirm_nonce: Option<String>,
) -> Result<ToolbeltRunReport, String> {
    // R508：medium+ 风险工具走完整闸门（来源 + nonce + 频率）；low 风险只需来源校验。
    crate::confirm_nonce::require_main_window(&window)?;
    let needs_gate = {
        let explicit0 = toolbelt_explicit_root(&app);
        diskpilot_toolbelt::find_tools_root(explicit0.as_deref())
            .ok()
            .and_then(|root| {
                diskpilot_toolbelt::toolbelt_command_for(&tool, &args, &root, timeout_secs).ok()
            })
            .map(|c| c.risk >= diskpilot_toolbelt::Risk::Medium)
            .unwrap_or(true) // 算不出风险等级时按高风险处理（fail-closed）
    };
    if needs_gate {
        crate::confirm_nonce::write_gate(
            &state.confirm_guard,
            "toolbelt_run",
            false,
            confirmed,
            confirm_nonce.as_deref(),
        )
        .map_err(|e| format!("toolbelt:confirm: {e}"))?;
    }
    if !state.perm_grants.lock().unwrap().contains("cli.run") {
        return Err(
            "拒绝执行：未开启「运行工具箱 CLI 工具」权限（cli.run）。请先在权限中心开启。".into(),
        );
    }
    let explicit = toolbelt_explicit_root(&app);
    let report = tokio::task::spawn_blocking(move || -> Result<ToolbeltRunReport, String> {
        let root =
            diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())?;
        let cmd = diskpilot_toolbelt::toolbelt_command_for(&tool, &args, &root, timeout_secs)
            .map_err(|e| e.to_string())?;
        // 纵深防御：manifest 声明 L3 的工具永久禁止（BIOS 刷写/格式化等），
        // 即使前端被绕过、或 manifest 未来加新 L3 工具，后端也直接拒绝。
        let manifest_lvl = diskpilot_toolbelt::find_manifest(&tool)
            .map(|m| m.permission_level.clone())
            .unwrap_or_default();
        if manifest_lvl == "L3" {
            return Err(format!(
                "「{}」为 L3 永久禁止操作（BIOS 刷写/格式化等），不可执行。",
                cmd.tool
            ));
        }
        if cmd.risk >= diskpilot_toolbelt::Risk::Medium && confirmed != Some(true) {
            return Err(format!(
                "toolbelt:confirm: 「{}」为 {:?} 风险工具，需用户确认后执行（参数：{}）",
                cmd.tool,
                cmd.risk,
                cmd.args.join(" ")
            ));
        }
        let command_line = format!("{} {}", cmd.program.display(), cmd.args.join(" "));
        let risk = cmd.risk;
        let outcome = diskpilot_toolbelt::run(&cmd);
        Ok(ToolbeltRunReport {
            command_line,
            risk,
            outcome,
        })
    })
    .await
    .map_err(|e| format!("执行任务崩溃: {e}"))??;
    Ok(report)
}

/// 全部 Tool Manifest（能力说明书）：前端详情卡 + AI 工具路由共用。
/// 返回编译期内嵌的 `tool_manifests.json` 解析结果，21 个工具五段式 schema。
#[tauri::command]
pub(crate) fn toolbelt_manifests() -> Vec<diskpilot_toolbelt::ToolManifest> {
    diskpilot_toolbelt::all_manifests().to_vec()
}

/// 按工具名启动 GUI 工具（双击工具墙/详情卡「启动」用）：只负责拉起 exe
/// 并立刻返回，不等待退出、不捕获输出——GUI 工具没有可解析的 stdout。
/// 解析顺序：CLI 白名单（快）→ 全目录 catalog 兜底（覆盖工具墙里所有图形工具）。
/// 风险确认（medium/high）由前端完成；这里只兜底拒绝 unknown 工具 + L3 永久禁止。
#[tauri::command]
pub(crate) fn toolbelt_launch(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    tool: String,
) -> Result<(), String> {
    // 纵深防御：`cli.run` 权限门双保险（前端双击/详情卡启动同款校验，绕过后端
    // 直接 invoke 也会被拒）。GUI 启动也是「运行工具箱工具」，同一条权限线。
    if !state.perm_grants.lock().unwrap().contains("cli.run") {
        return Err(
            "拒绝启动：未开启「运行工具箱 CLI 工具」权限（cli.run）。请先在权限中心开启。".into(),
        );
    }
    // manifest 声明 L3 的工具（BIOS 刷写/格式化等）永久禁止启动，即使绕过前端
    // 直接 invoke 也拦在 spawn 前。
    if let Some(m) = diskpilot_toolbelt::find_manifest(&tool) {
        if m.permission_level == "L3" {
            return Err(format!(
                "「{tool}」为 L3 永久禁止操作（BIOS 刷写/格式化等），不可启动。"
            ));
        }
    }
    use std::path::PathBuf;
    let explicit = toolbelt_explicit_root(&app);
    let root =
        diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())?;

    // 1) CLI 文档白名单命中（零全目录 IO）——返回 (exe, 是否插件入口)
    let exe = (|| -> Option<(PathBuf, bool)> {
        let entry = diskpilot_toolbelt::find(&tool)?;
        let rel = entry.exe_rel.as_deref()?;
        let p = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        p.is_file().then_some((p, false))
    })();

    // 2) 全目录工具墙兜底（图形工具大多不在 CLI 白名单）。入口解析顺序：
    //    tools.json 的 launchTarget（原版语义：显式指定入口 exe）→ 插件
    //    tool.plugin.json 的 entry（相对插件目录）→ catalog 判定出的主启动文件。
    let exe = exe.or_else(|| {
        let catalog = catalog_cached(explicit.as_deref());
        let item = catalog.categories.iter().flat_map(|c| &c.tools).find(|t| {
            t.name.eq_ignore_ascii_case(&tool)
                || t.name
                    .to_ascii_lowercase()
                    .contains(&tool.to_ascii_lowercase())
        })?;
        if let Some(lt) = item.launch_target.as_deref() {
            let cand = root.join(lt.replace('/', std::path::MAIN_SEPARATOR_STR));
            if cand.is_file() {
                return Some((cand, false));
            }
        }
        if let Some(plugin) = item.plugin.as_ref() {
            let base = root.join(item.dir_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            let cand = base.join(plugin.entry.replace('/', std::path::MAIN_SEPARATOR_STR));
            if cand.is_file() {
                // R505：标记为插件入口——插件包可来自 verify=none 的本地 zip，
                // 其 exe 自带 requireAdministrator 会骗 UAC，插件一律禁 runas。
                return Some((cand, true));
            }
        }
        let rel = item.exe_rel.as_deref()?;
        let p = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        p.is_file().then_some((p, false))
    });
    let Some((exe, is_plugin)) = exe else {
        // 用目录名再试一次：用户看到的往往是「工具墙卡片名」（= 主文件 stem），
        // catalog 匹配已覆盖；到这里说明工具不存在或目录里没有可执行主文件。
        return Err(format!(
            "未找到可启动的工具「{tool}」（或该工具目录缺少主程序）"
        ));
    };
    // R505 纵深防御：不管 entry / launchTarget / exe_rel 元数据怎么写，最终要启动的
    // exe 经 canonicalize 后必须仍位于 Tools 根内——插件 entry 写 `..\..\x.exe`
    // 这种穿越在这里被一刀拦住（fail-closed，解析失败也拒绝）。
    let exe = diskpilot_toolbelt::ensure_path_under(&root, &exe).map_err(|e| e.to_string())?;
    let cwd = exe.parent().unwrap_or(&root).to_path_buf();
    // 工具若带控制台（urwtest 等）会闪黑框；加 CREATE_NO_WINDOW 隐藏。
    // 备注：GUI 工具的创建设置在子进程里仍有效——正在运行的窗口保留，缺省终端不创建。
    let mut launch = std::process::Command::new(&exe);
    launch.current_dir(&cwd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        launch.creation_flags(CREATE_NO_WINDOW);
    }
    match launch.spawn() {
        Ok(_) => Ok(()),
        Err(e) if e.raw_os_error() == Some(740) => {
            // ERROR_ELEVATION_REQUIRED：工具的 manifest 声明必须管理员运行
            //（磁盘类工具很常见）。普通 CreateProcess 直接被拒，改用
            // ShellExecute `runas` 拉起 UAC，用户确认后以管理员身份启动。
            // R505：插件入口（第三方 zip）一律不给 runas——插件可自带
            // requireAdministrator 的 exe 骗用户点 UAC 确认。
            if is_plugin {
                return Err(format!(
                    "插件「{tool}」声明需要管理员权限，但第三方插件不允许 UAC 提权；请卸载该插件或以管理员身份手动运行其程序。"
                ));
            }
            #[cfg(windows)]
            match launch_with_runas(&exe, &cwd) {
                Ok(()) => Ok(()),
                Err(e2) if e2.raw_os_error() == Some(1223) => Err(format!(
                    "「{tool}」需要管理员权限才能运行，已在用户账户控制（UAC）中取消。"
                )),
                Err(e2) => Err(format!("启动「{tool}」需要管理员权限，提权失败：{e2}")),
            }
            #[cfg(not(windows))]
            Err(format!("启动「{tool}」失败：{e}"))
        }
        Err(e) => Err(format!("启动「{tool}」失败：{e}")),
    }
}

/// 回收一个工具目录（工具墙「移除」）：按工具名在 catalog 里定位目录，
/// 整个目录移入系统回收站（可恢复），并写一条 undo 日志——与卸载插件同一条
/// 撤销/恢复链路，用户可以在「最近清理」里一键还原。
///
/// 安全边界：只接受 catalog 里解析出的工具目录（必然位于某分类之下），
/// 拒绝 Tools 根 / 分类根 / 含穿越段的相对路径，杜绝把非工具目录误回收。
///
/// 写操作纵深防御：必须 `confirmed = true`（前端 `recycleAsk` 两步确认弹窗通过
/// 后才带）——绕过确认窗直接 invoke 一律拒绝，与 `plugin_uninstall` 同一口径。
#[tauri::command]
pub(crate) async fn toolbelt_recycle(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    tool: String,
    confirmed: Option<bool>,
    window: tauri::Window,
    confirm_nonce: Option<String>,
) -> Result<serde_json::Value, String> {
    // R508：来源校验 + 一次性 nonce + 频率限制。
    crate::confirm_nonce::require_main_window(&window)?;
    crate::confirm_nonce::write_gate(
        &state.confirm_guard,
        "toolbelt_recycle",
        false,
        confirmed,
        confirm_nonce.as_deref(),
    )
    .map_err(|e| format!("toolbelt:confirm: {e}"))?;
    let explicit = toolbelt_explicit_root(&app);
    let root =
        diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())?;

    let catalog = catalog_cached(explicit.as_deref());
    let item = catalog
        .categories
        .iter()
        .flat_map(|c| &c.tools)
        .find(|t| {
            t.name.eq_ignore_ascii_case(&tool)
                || t.name
                    .to_ascii_lowercase()
                    .contains(&tool.to_ascii_lowercase())
        })
        .ok_or_else(|| format!("未找到工具「{tool}」"))?;
    let dir_rel = item.dir_rel.clone();
    if dir_rel.is_empty() || dir_rel == "." || dir_rel.split(['/', '\\']).any(|s| s == "..") {
        return Err(format!("拒绝回收非工具目录：{dir_rel}"));
    }
    let name = item.name.clone();
    let dir = root.join(dir_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !dir.is_dir() {
        return Err(format!("工具目录不存在：{}", dir.display()));
    }
    // R505 纵深：回收前确认目录 canonicalize 后仍在 Tools 根内（防目录被替换为
    // 指向根外的 symlink/junction——穿越段已在上方拒绝，这里兜底解析后的真实落点）。
    let dir = diskpilot_toolbelt::ensure_path_under(&root, &dir).map_err(|e| e.to_string())?;

    let undo_log = state.undo_log.clone();
    let quarantine_root = state.quarantine_root.clone();
    let plan = diskpilot_executor::Plan {
        action: diskpilot_executor::Action::Recycle,
        paths: vec![dir],
        reason: format!("移除工具「{name}」"),
    };
    let out = tokio::task::spawn_blocking(move || {
        diskpilot_executor::execute(&plan, false, &undo_log, &quarantine_root)
    })
    .await
    .map_err(|e| format!("回收任务崩溃: {e}"))?
    .map_err(|e| format!("回收失败：{e}"))?;

    Ok(serde_json::json!({
        "name": name,
        "dir_rel": dir_rel,
        "recycled": out.iter().map(|u| u.source.display().to_string()).collect::<Vec<_>>(),
    }))
}
