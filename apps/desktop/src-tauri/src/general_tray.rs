//! 通用配置（general.json）+ 托盘常驻（close_to_tray / tray_sync）。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：硬件加速 / Tools 根目录 / 关闭行为配置
//! 的读写，以及系统托盘图标与菜单。仅依赖 AppHandle 与 tauri menu/tray。

use tauri::AppHandle;
use tauri::Manager;

// ── 通用配置（硬件加速等，持久化到 app_data_dir/general.json）────────────
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub(crate) struct GeneralConfig {
    /// true = 用 GPU 渲染（默认）；false = 加 --disable-gpu。
    #[serde(default = "default_true")]
    pub(crate) hardware_accel: bool,
    /// 图吧工具箱 Tools 根目录覆盖；留空 = 从 exe/cwd 逐级上溯自动定位。
    #[serde(default)]
    pub(crate) tools_root: Option<String>,
    /// 托盘常驻：点窗口关闭按钮时最小化到托盘而不是退出（默认开）。
    #[serde(default = "default_true")]
    pub(crate) close_to_tray: bool,
}

fn default_true() -> bool {
    true
}

/// 前端启动时读当前配置（主要给硬件加速开关回显用）。
#[tauri::command]
pub(crate) fn general_config(app: AppHandle) -> GeneralConfig {
    general_config_at(&app)
}

pub(crate) fn general_config_at(app: &AppHandle) -> GeneralConfig {
    let Some(p) = app.path().app_data_dir().ok() else {
        return GeneralConfig::default();
    };
    std::fs::read_to_string(p.join("general.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) fn set_general(app: AppHandle, hardware_accel: bool) -> Result<(), String> {
    let Some(dir) = app.path().app_data_dir().ok() else {
        return Err("无法定位数据目录".into());
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // 读旧值合并写回：避免翻开关时把 tools_root / close_to_tray 抹掉
    let mut cfg = general_config_at(&app);
    cfg.hardware_accel = hardware_accel;
    let text = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("general.json"), text).map_err(|e| e.to_string())?;
    Ok(())
}

/// 工具箱 Tools 根目录设置（前端「工具箱」面板的设置入口；空串 = 恢复自动定位）。
#[tauri::command]
pub(crate) fn set_tools_root(app: AppHandle, path: String) -> Result<(), String> {
    let Some(dir) = app.path().app_data_dir().ok() else {
        return Err("无法定位数据目录".into());
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut cfg = general_config_at(&app);
    cfg.tools_root = if path.trim().is_empty() {
        None
    } else {
        Some(path)
    };
    let text = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("general.json"), text).map_err(|e| e.to_string())?;
    Ok(())
}

// ── 托盘常驻（P4-2）─────────────────────────────────────────────────
// 关闭窗口按钮：close_to_tray 开 → 隐藏到系统托盘继续常驻（清理提醒线程、
// 后台扫描不中断）；关 → 正常退出。托盘菜单用前端推来的 i18n 文案（tray_sync），
// 语言切换后由 Settings / 前台启动时重推。
const TRAY_ID: &str = "diskpilot-tray";
const TRAY_SHOW_ID: &str = "tray-show";
const TRAY_QUIT_ID: &str = "tray-quit";

#[tauri::command]
pub(crate) fn set_close_to_tray(app: AppHandle, enabled: bool) -> Result<(), String> {
    let Some(dir) = app.path().app_data_dir().ok() else {
        return Err("无法定位数据目录".into());
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut cfg = general_config_at(&app);
    cfg.close_to_tray = enabled;
    let text = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("general.json"), text).map_err(|e| e.to_string())?;
    Ok(())
}

/// 前端把当前语言的托盘菜单文案推过来（菜单项 id 固定，重建菜单换 text + tooltip）。
#[tauri::command]
pub(crate) fn tray_sync(
    app: AppHandle,
    show_label: String,
    quit_label: String,
    tooltip: String,
) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    let Some(_tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(()); // 平台不支持（如无 appindicator 的 Linux）时静默
    };
    let show = MenuItem::with_id(&app, TRAY_SHOW_ID, show_label, true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(&app, TRAY_QUIT_ID, quit_label, true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(
        &app,
        &[
            &show,
            &PredefinedMenuItem::separator(&app).expect("separator"),
            &quit,
        ],
    )
    .map_err(|e| e.to_string())?;
    let tray = app.tray_by_id(TRAY_ID).expect("tray exists");
    tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
    tray.set_tooltip(Some(&tooltip))
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn spawn_tray(app: &tauri::App) {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIconBuilder;

    // 用 app 图标（bundle icon）作托盘图标；拿不到就跳过（非致命）。
    let Some(icon) = app.default_window_icon().cloned() else {
        return;
    };

    let show = MenuItem::with_id(app, TRAY_SHOW_ID, "显示 DiskPilot", true, None::<&str>)
        .expect("tray show item");
    let quit =
        MenuItem::with_id(app, TRAY_QUIT_ID, "退出", true, None::<&str>).expect("tray quit item");
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &PredefinedMenuItem::separator(app).expect("separator"),
            &quit,
        ],
    )
    .expect("tray menu");

    let result = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("DiskPilot")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            TRAY_SHOW_ID => show_main_window(app),
            TRAY_QUIT_ID => {
                // 从托盘退出属于显式退出：不走 close_to_tray，直接退出。
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            use tauri::tray::{MouseButton, MouseButtonState};
            if let tauri::tray::TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app);

    if let Err(e) = result {
        tracing::warn!("tray icon unavailable: {e}");
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        win.show().ok();
        win.unminimize().ok();
        win.set_focus().ok();
    }
}
