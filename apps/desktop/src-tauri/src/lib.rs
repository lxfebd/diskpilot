use std::collections::HashMap;
pub(crate) mod agent;
pub(crate) mod ai;
mod ai_net;
mod cleanup;
mod cleanup_logic;
mod conda;
mod drivers;
mod drives;
mod dups;
mod executor;
mod general_tray;
pub(crate) mod hw;
mod hw_history;
mod hw_report;
pub(crate) mod mcp;
mod mcp_bridge;
mod net_http;
mod net_proxy;
mod plugin_registry;
mod plugin_registry_types;
mod plugin_remote;
mod reminder;
mod scaffold_cmds;
mod scaffold_registry;
mod scan;
mod space_history;
pub(crate) mod startup;
pub(crate) mod steam;
pub(crate) mod system;
pub(crate) mod toolbelt;
pub(crate) mod updates;
mod webview;
mod weekly_report;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use diskpilot_scaffold::Scaffold;
use diskpilot_scanner::Node;

use tauri::Manager;

pub(crate) use cleanup_logic::CleanupCacheEntry;

struct AppState {
    scaffolds: Mutex<Vec<Scaffold>>,
    quarantine_root: PathBuf,
    undo_log: PathBuf,
    /// Shared abort flag for in-flight scans. Reset to false at the start of
    /// each `scan_path`, flipped by `cancel_scan` so a long walk/MFT pass can
    /// be interrupted from the frontend instead of hanging until it finishes.
    scan_cancel: Arc<AtomicBool>,
    /// 最近一次 `scan_path` 完成后、基于全量扫描树（非截断文件列表）算出的
    /// 各 scope 可清理字节缓存。`cleanup_suggestions` 优先命中这里秒回，
    /// 避免每次进盘详情页都重新全盘 jwalk 一遍（实测 C 盘 500s+）。
    /// key = 归一化后的盘根路径（如 `C:\` → `c:\`）。
    cleanup_cache: Mutex<HashMap<String, CleanupCacheEntry>>,
    /// 每盘（归一化盘根 key）最近一次扫描后的完整 Node 树。AI 上下文/总览、
    /// 清理建议秒算、USN 增量合并、tree_subtree 展开都从对应 key 的缓存树读，
    /// 避免前端把整棵树序列化回来、或每次切盘都要重新全盘遍历。
    /// key = 归一化后的盘根路径（如 `C:\` → `c:\`），与 cleanup_cache / usn_cursors 同 key。
    scan_tree: Mutex<HashMap<String, Node>>,
    /// 每盘（归一化盘根 key）最近一次扫描后的 USN Journal 游标。`scan_path_usn`
    /// 用它在增量可用时只回放变更日志并 merge 进 `scan_tree` 缓存树，把重扫
    /// 从分钟级降到亚秒级。游标与 scan_tree 生命周期一致：树被覆盖时同步更新。
    /// 非 Windows 无 journal，用 `()` 占位（`commit_scan` 泛型参数退化，行为不变）。
    #[cfg(windows)]
    usn_cursors: Mutex<HashMap<String, diskpilot_scanner::usn::UsnCursor>>,
    #[cfg(not(windows))]
    usn_cursors: Mutex<HashMap<String, ()>>,
    /// 进行中的硬件压测「用户停止」开关列表：每个测试实例注册自己的
    /// Arc<AtomicBool>，hw_stop_test 遍历全部置 true。用多槽而非单槽：
    /// 单槽全局 flag 会被后启动的测试 reset，导致先启动的测试停不下来。
    hw_stops: Mutex<Vec<Arc<AtomicBool>>>,
    /// 后端视角的 AI 权限快照（前端 sync_perms 同步，key 为权限 id 如
    /// `cleanup.execute` / `plugin.manage`）。前端权限中心改动后调 sync_perms
    /// 推一份到后端，`execute_ai_plan` / `toolbelt_run` 等写命令据此做纵深校验
    /// （防「绕过前端直调 Tauri」）。默认空 = 无 L2 授权，写操作一律拒绝。
    perm_grants: Mutex<std::collections::HashSet<String>>,
    /// 前端 WebView2 心跳：`webview_heartbeat` 命令每次调用记录当前时间戳。
    /// watchdog 线程检测到长时间无心跳（> 12s）判定 WebView 渲染进程已崩溃，
    /// 触发 reload 自愈。None = 从启动到首次心跳（或 reload 后）之间的宽限期，
    /// 期间不判定超时，避免「应用刚起来前端还没就绪」被误杀。见
    /// `spawn_webview_watchdog`。
    last_heartbeat: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// 进行中的 AI 代理请求「用户停止」开关表：key = 前端生成的 cancel_key
    /// （UUID），ai_proxy 注册自己的 Arc<AtomicBool>，ai_cancel(key) 按 key 置
    /// true，ai_proxy 的发送循环轮询到 true 即中断返回「已取消」。请求结束
    /// 无论成功失败都移除 key，防止表无限膨胀。
    ai_cancels: Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>,
    /// WebView2 重建门闩：`recreate_webview` 开始重建时置位，重建完成（或
    /// 超时放弃）后清除。watchdog 在门闩置位期间不触发新的重建/心跳 reload，
    /// 防止「重建进行中又下一 tick 触发」叠枪，以及「新进程组还没起来就被
    /// 探测为死亡 → 无限重建循环」。同时用作恢复期保护：重建成功后会短暂
    /// 保持置位（process alive 探活兜底），让新 WebView2 进程组有时间起来。
    /// 超时兜底：超过 30s 强制清除（Alive 过 8s 超时 + build 几秒 + 进程起
    /// 需要时间，正常情况下远小于 30s）。
    webview_busy: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
}

// ── 硬件检测与受控压测（hw_detect）────────────────────────────────────
// 对齐 tubatools 硬件助手设计：
//   L0 只读：hw_info / hw_disk_health / hw_report —— 零确认，随时可调
//   L1 受控：hw_run_test —— 永远要 user_confirmed=true，超时强杀 + 温度熔断
//   L2 授权：power_plan —— 每次仍需 confirmed=true，用户可在权限中心开启
// 所有工具调用只走 toolbelt 白名单解析出的 exe，路径不由 AI 提供。
// 读不到的字段一律优雅降级为 null，绝不编造数值。

/// 在 run() 开头（没有 AppHandle 时）推算 app_data_dir：Windows 上是
/// %APPDATA%\{bundle_identifier}。与 Tauri 的默认 app_data_dir 保持一致。
fn app_data_dir_for_env() -> Option<PathBuf> {
    let base = std::env::var("APPDATA")
        .ok()
        .map(PathBuf::from)
        .or_else(dirs::data_dir)?;
    Some(base.join("dev.diskpilot.app"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // 硬件加速开关：关闭时给 WebView2 加 --disable-gpu。WebView2 环境在
    // Builder.run() 创建窗口时建立，所以必须在这之前设环境变量。
    // 读不到配置/读取出错时保持默认（硬件加速开）。
    let hw_accel = std::env::var("DISKPILOT_SKIP_HW").is_err()
        && app_data_dir_for_env()
            .map(|dir| {
                std::fs::read_to_string(dir.join("general.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str::<general_tray::GeneralConfig>(&s).ok())
                    .map(|c| c.hardware_accel)
                    .unwrap_or(true)
            })
            .unwrap_or(true);
    if !hw_accel {
        let existing = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGS").unwrap_or_default();
        let merged = if existing.trim().is_empty() {
            "--disable-gpu".to_string()
        } else {
            format!("{existing} --disable-gpu")
        };
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGS", merged);
        tracing::info!("hardware acceleration: DISABLED (--disable-gpu)");
    } else {
        tracing::info!("hardware acceleration: enabled (default)");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from(".diskpilot"));
            std::fs::create_dir_all(&data_dir).ok();
            let undo_log = data_dir.join("undo.jsonl");
            let quarantine_root = data_dir.join("quarantine");

            let scaffolds = scaffold_cmds::load_all_scaffolds(app.handle());
            tracing::info!("loaded {} scaffolds", scaffolds.len());

            app.manage(AppState {
                scaffolds: Mutex::new(scaffolds),
                quarantine_root,
                undo_log,
                scan_cancel: Arc::new(AtomicBool::new(false)),
                cleanup_cache: Mutex::new(HashMap::new()),
                scan_tree: Mutex::new(HashMap::new()),
                usn_cursors: Mutex::new(HashMap::new()),
                hw_stops: Mutex::new(Vec::new()),
                perm_grants: Mutex::new(std::collections::HashSet::new()),
                last_heartbeat: Mutex::new(None),
                ai_cancels: Mutex::new(std::collections::HashMap::new()),
                webview_busy: Mutex::new(None),
            });

            // 平台支持时建托盘（取不到图标/无 appindicator 则跳过，非致命）。
            general_tray::spawn_tray(app);
            // WebView2 崩溃自愈 watchdog：后台线程周期检查前端心跳。
            webview::spawn_webview_watchdog(app.handle().clone());
            // 清理提醒后台线程：配置开启后定时算建议 + emit（只出清单，不删）。
            reminder::spawn_cleanup_reminder(app.handle().clone());
            // 插件 update/rollback 崩溃残留的 *.dp-old-tmp 目录启动清扫。
            plugin_registry::preg_misc::sweep_orphaned_tmp_dirs(app.handle());
            Ok(())
        })
        // 托盘常驻：点窗口关闭按钮时查 close_to_tray 配置，开 → 隐藏到托盘。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let close_to_tray =
                    general_tray::general_config_at(window.app_handle()).close_to_tray;
                if close_to_tray {
                    api.prevent_close();
                    window.hide().ok();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            webview::webview_heartbeat,
            scan::scan_path,
            scan::cancel_scan,
            scan::tree_subtree,
            #[cfg(windows)]
            scan::scan_path_usn,
            drives::estimate_size,
            scaffold_cmds::sync_perms,
            scaffold_cmds::list_scaffolds,
            scaffold_cmds::detect_scaffold,
            scaffold_cmds::install_scaffold,
            scaffold_cmds::parse_winapp2,
            scaffold_cmds::uninstall_scaffold,
            scaffold_cmds::scaffold_source,
            scaffold_registry::scaffold_registry_list,
            scaffold_registry::scaffold_registry_install,
            cleanup::scope_sizes_batch,
            cleanup::cleanup_suggestions,
            cleanup::chat_scan_context,
            executor::exec_scope::execute_scope,
            conda::list_conda_envs,
            conda::inspect_path,
            conda::reveal_in_explorer,
            executor::exec_glob::recycle_paths,
            executor::exec_aiplan::execute_ai_plan,
            executor::exec_undo::list_undo,
            executor::exec_undo_core::undo,
            executor::exec_undo::cleanup_preflight,
            dups::scan_duplicate_files_cmd,
            drives::volume_info,
            reminder::get_reminder_config,
            reminder::set_reminder_config,
            reminder::run_reminder_check_cmd,
            weekly_report::generate_weekly_report_cmd,
            weekly_report::get_weekly_reports,
            space_history::get_space_history,
            hw_history::get_hw_history,
            hw_history::compare_hw_snapshots,
            drives::list_drives,
            steam::list_steam_games,
            steam::list_steam_workshop_items,
            steam::fetch_workshop_titles,
            steam::translate_steam_names,
            steam::translate_steam_names_fetch,
            steam::open_steam_url,
            ai_net::web_search,
            ai::ai_proxy,
            ai::ai_cancel,
            agent::agent_list_tools,
            agent::agent_call_tool,
            agent::agent_tool_cancel,
            agent::agent_server_ready,
            mcp::mcp_probe::mcp_list_servers,
            mcp::mcp_probe::mcp_test_server,
            mcp::mcp_crud::mcp_add_server,
            mcp::mcp_crud::mcp_update_server,
            mcp::mcp_crud::mcp_remove_server,
            mcp::mcp_tools::mcp_list_tools,
            mcp::mcp_tools::mcp_call_tool,
            mcp::mcp_audit::mcp_audit_tail,
            general_tray::general_config,
            general_tray::set_general,
            general_tray::set_tools_root,
            general_tray::set_close_to_tray,
            general_tray::tray_sync,
            toolbelt::toolbelt_status,
            toolbelt::toolbelt_catalog,
            toolbelt::toolbelt_usage,
            toolbelt::toolbelt_run,
            toolbelt::toolbelt_manifests,
            toolbelt::toolbelt_launch,
            toolbelt::plugin_uninstall,
            toolbelt::plugin_install,
            toolbelt::plugin_market,
            toolbelt::plugin_activate,
            toolbelt::plugin_deactivate,
            toolbelt::plugin_export,
            plugin_remote::plugin_install_url,
            plugin_registry::preg_read::plugin_registry_list,
            plugin_registry::preg_read::plugin_registry_search,
            plugin_registry::plugin_mutate::plugin_registry_install,
            plugin_registry::preg_read::plugin_registry_verify,
            plugin_registry::preg_refresh::plugin_registry_refresh,
            plugin_registry::plugin_mutate::plugin_registry_update,
            plugin_registry::plugin_mutate::plugin_registry_rollback,
            plugin_registry::preg_read::plugin_list_installed,
            plugin_registry::preg_read::plugin_registry_config,
            plugin_registry::preg_url::plugin_set_registry_url,
            toolbelt::toolbelt_recycle,
            updates::check_update,
            hw::hw_disk::hw_info,
            hw::hw_disk::hw_disk_health,
            hw::hw_test::hw_run_test,
            hw::hw_test::hw_stop_test,
            hw::hw_report_cmd::hw_report,
            hw::hw_report_cmd::power_plan,
            hw::hw_fan::fan_curve_advice,
            hw::hw_fan::fan_control,
            hw::hw_fan::fan_status,
            hw::hw_fan::fan_curve_apply,
            system::run_system_probe,
            system::network_probe,
            startup::list_startup_items,
            startup::set_startup_item,
            startup::remove_startup_item,
            drivers::list_installed_drivers,
            drivers::check_driver_updates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
