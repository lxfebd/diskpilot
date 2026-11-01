//! WebView2 崩溃自愈：心跳 / watchdog / 窗口重建。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：WebView2 渲染进程死亡或前端 JS 卡死时
//! 自动重建/重载窗口。仅依赖 AppState 与 tauri，便于独立演化。

use std::time::Duration;

use tauri::AppHandle;
use tauri::Manager;

use crate::AppState;

#[tauri::command]
pub(crate) fn webview_heartbeat(app: AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.last_heartbeat.lock().unwrap() = Some(chrono::Utc::now());
    }
}

/// 前台 watchdog：周期检查 WebView 心跳 + 渲染进程存活。
/// 在 run() 的 setup 里 spawn。每个 tick（2s）：
///  1. 渲染进程真死了（枚举子进程：msedgewebview2.exe 且父进程 = 本进程，
///     一个都不剩）→ 不依赖心跳，直接重建窗口——WebView2 浏览器进程死亡后
///     reload 只是给已死的 controller 发命令，永远不生效（实测黑屏不恢复）；
///     正确动作是 destroy 旧窗口 + 按配置重建（根治「UI 假死、主进程空壳」）。
///  2. 心跳超时（>12s）→ 前端 JS 卡死（浏览器进程还活着）→ reload 自愈。
///
/// 重建后前端恢复心跳则自动解除；若重建也无用（进程起不来），主进程
/// 不自杀，但日志会明确提示，避免「无声假死」。
pub(crate) fn spawn_webview_watchdog(app: AppHandle) {
    use chrono::Utc;
    #[cfg(windows)]
    use std::sync::atomic::{AtomicBool, Ordering};
    const TICK: Duration = Duration::from_secs(2);
    const STALE_AFTER: chrono::Duration = chrono::Duration::seconds(12);
    // 重建/重载必须发生在主线程（WebView2 的窗口操作线程模型），
    // 这里用标志位避免后台线程每 2s 狂投递，也避免上一次还没做完又叠一次。
    #[cfg(windows)]
    let recreate_pending = std::sync::Arc::new(AtomicBool::new(false));

    std::thread::spawn(move || loop {
        std::thread::sleep(TICK);
        let Some(state) = app.try_state::<AppState>() else {
            continue;
        };

        // 0) 重建门闩检查：重建进行中（或恢复期内）不重复触发，防止叠枪和
        //    「新进程组还没起来就被判死亡 → 无限重建循环」。超时兜底 30s。
        {
            let busy = *state.webview_busy.lock().unwrap();
            if let Some(t) = busy {
                if Utc::now() - t <= chrono::Duration::seconds(30) {
                    continue;
                }
                // 超时：清闩，允许下一 tick 重新尝试
                *state.webview_busy.lock().unwrap() = None;
            }
        }

        // 1) 窗口存活探测：main 窗口没了（重建失败后只剩 holder 撑进程）
        //    或渲染进程全灭 → 重建整个窗口（reload 无效）。
        //    注意：不能只看渲染子进程——holder 自己的 WebView2 子进程会被
        //    has_webview_renderer_child 误判为「活着」，导致 main 崩了不重建。
        #[cfg(windows)]
        {
            use tauri::Manager;
            let has_main = app.get_webview_window("main").is_some();
            let procs_alive = has_webview_renderer_child();
            if !has_main || !procs_alive {
                tracing::error!(
                    "webview main window gone (has_main={has_main}, procs_alive={procs_alive}) — recreating"
                );
                // 重置心跳基线，避免重建后旧心跳时间戳导致下一 tick 误判
                *state.last_heartbeat.lock().unwrap() = None;
                // 置闩：重建期间 watchdog 不再触发新重建（直到重建完成/超时）
                *state.webview_busy.lock().unwrap() = Some(Utc::now());
                if !recreate_pending.swap(true, Ordering::SeqCst) {
                    let app2 = app.clone();
                    let flag = recreate_pending.clone();
                    // run_on_main_thread：窗口/WebView2 操作必须在事件循环线程，
                    // 后台线程直接 build 会停在 about:blank（实测三次一致）。
                    let _ = app.run_on_main_thread(move || {
                        recreate_webview(&app2);
                        flag.store(false, Ordering::SeqCst);
                    });
                }
                continue;
            }
        }

        // 2) 心跳超时判定（前端 JS 卡死但进程还活着）：reload 自愈。
        let last = *state.last_heartbeat.lock().unwrap();
        let Some(last) = last else { continue }; // 宽限期：还没收到首次心跳
        let age = Utc::now() - last;
        if age <= STALE_AFTER {
            continue;
        }
        tracing::warn!(
            "webview heartbeat stale for {:.1}s — reloading webview",
            age.num_milliseconds() as f64 / 1000.0
        );
        // 重置基线：即使 reload 失败也避免每 2s 狂 reload。
        *state.last_heartbeat.lock().unwrap() = None;
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            for (_, w) in app2.webview_windows() {
                let _ = w.reload();
            }
        });
    });
}

/// Windows: 渲染进程全灭后的重建动作。
/// 顺序陷阱（2026-09-15 实测）：直接 destroy 唯一窗口 → Tauri run loop 检测到
/// 窗口清空 → 主进程退出，「UI 假死」变「整个应用死」，这比黑屏更糟。
/// 正确顺序：先建一个隐藏占位窗口（保证 destroy 后仍 ≥1 窗口，run loop 不退出）
/// → destroy 崩溃窗口（label 释放）→ 按 tauri.conf.json 配置重建同 label 的真窗口
/// （from_config 自动继承 dev/prod URL 与几何属性）→ 销毁占位窗口。
/// 重建失败不 panic：记日志即可，下一 tick 由存活探测再次触发。
#[cfg(windows)]
fn recreate_webview(app: &tauri::AppHandle) {
    use std::time::Instant;
    use tauri::Manager;
    use tauri::WebviewUrl;
    use tauri::WebviewWindowBuilder;

    // 重建全路径诊断（阶段标记 → 文件，供黑屏/重建失败排障）
    fn diag(msg: String) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(r"C:\temp\dp-rebuild.log")
        {
            let _ = writeln!(f, "[{}] {msg}", chrono::Utc::now().format("%H:%M:%S"));
        }
    }

    diag("== recreate_webview enter ==".into());
    // 1) placeholder：已有则复用（上一次重建失败可能留下 holder，重复 build
    //    同 label 必然 already exists）；没有才创建。holder 只在真窗口全灭时
    //    短暂撑主进程不退出。
    let holder: Option<tauri::WebviewWindow> = match app.get_webview_window("watchdog-holder") {
        Some(h) => {
            diag("holder reused (already exists)".into());
            Some(h)
        }
        None => {
            match WebviewWindowBuilder::new(
                app,
                "watchdog-holder",
                WebviewUrl::App("index.html".into()),
            )
            .title("")
            .inner_size(1.0, 1.0)
            .visible(false)
            .build()
            {
                Ok(h) => {
                    diag("holder created".into());
                    Some(h)
                }
                Err(e) => {
                    diag(format!("holder build failed: {e}"));
                    tracing::error!("failed to create watchdog holder window: {e}");
                    None
                }
            }
        }
    };

    // 2) 要销毁的 = 当前存活窗口 − holder（崩溃/残留的真窗口）。
    //    注意：main 可能已经没了，这里为空是正常情形，不代表没东西要重建。
    let destroy_labels: Vec<String> = app
        .webview_windows()
        .values()
        .map(|w| w.label().to_string())
        .filter(|l| l != "watchdog-holder")
        .collect();
    diag(format!("destroying {:?}", destroy_labels));
    for label in &destroy_labels {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.destroy();
        }
    }

    // 3) 重建目标 = config 里定义的所有真窗口（与当前存活无关！）。
    //    之前错用「存活窗口」当重建目标：main 崩了之后 labels=[]，
    //    重建循环 continue 全跳过 → rebuilt_any=false → 每 2s 死循环
    //    空转，main 窗口永远回不来（dp-rebuild.log 实测铁证）。
    let rebuild_labels: Vec<String> = app
        .config()
        .app
        .windows
        .iter()
        .map(|w| w.label.clone())
        .filter(|l| l != "watchdog-holder")
        .collect();
    diag(format!("rebuild targets: {:?}", rebuild_labels));
    // destroy 是异步的：label 不会立即释放（实测 2s 都不够，WebView2 controller
    // 销毁要等渲染进程退出）。必须等 label 真正释放才能同 label 重建——
    // 否则 build 报「a webview with label `main` already exists」失败。
    // 这里不阻塞主线程：后台线程轮询 label 释放（最多 8s），释放后投递 build
    // 回主线程；到点仍未释放则本轮放弃，下一 tick 存活探测会再次触发。
    let app2 = app.clone();
    let destroy2 = destroy_labels.clone();
    let rebuild2 = rebuild_labels.clone();
    let holder2 = holder.clone();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let all_gone = destroy2
                .iter()
                .all(|l| app2.get_webview_window(l).is_none());
            if all_gone || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let still_held: Vec<&String> = destroy2
            .iter()
            .filter(|l| app2.get_webview_window(l).is_some())
            .collect();
        if !still_held.is_empty() {
            diag(format!(
                "destroy timeout, labels still held: {:?}",
                still_held
            ));
            tracing::error!(
                "webview destroy timeout, labels still held: {:?} — deferring rebuild to next tick",
                still_held
            );
            return;
        }
        diag("destroy settled; labels all released".to_string());
        // 投递 build 回主线程（WebView2 窗口创建必须在事件循环线程）
        let app3 = app2.clone();
        let rebuild3 = rebuild2.clone();
        let _ = app2.run_on_main_thread(move || {
            let mut rebuilt_any = false;
            for cfg in &app3.config().app.windows {
                if !rebuild3.contains(&cfg.label) {
                    continue;
                }
                // 竞态守卫：destroy 等待期间若有别的路径已把该 label 建回来
                // （如用户手动/恢复流程），再 build 会撞 already exists，跳过。
                if app3.get_webview_window(&cfg.label).is_some() {
                    diag(format!("label {:?} already exists before rebuild, skip", cfg.label));
                    continue;
                }
                match WebviewWindowBuilder::from_config(&app3, cfg).and_then(|b| b.build()) {
                    Ok(w) => {
                        rebuilt_any = true;
                        diag(format!("rebuilt window {:?}", w.label()));
                        let label = w.label().to_string();
                        let win = w.clone();
                        // 记录重建窗口加载的真实 URL（诊断：from_config 重建后可能停在 about:blank）。
                        // build() 刚返回时导航是异步的，立即读多半是创建瞬间的初始值，
                        // 这里延迟轮询（≤3s）读终态；若仍是 about:blank，则显式导航到
                        // 生产首页兜底，确保恢复后不是白屏。
                        std::thread::spawn(move || {
                            use std::io::Write;
                            let mut last_url = String::new();
                            for _ in 0..6 {
                                if let Ok(u) = win.url() {
                                    last_url = u.to_string();
                                    if !last_url.starts_with("about:") {
                                        break;
                                    }
                                }
                                std::thread::sleep(Duration::from_millis(500));
                            }
                            if let Ok(mut f) = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(r"C:\temp\dp-urllog.txt")
                            {
                                let _ = writeln!(
                                    f,
                                    "[{}] recreated {label} url={last_url}",
                                    chrono::Utc::now().format("%H:%M:%S")
                                );
                            }
                            if last_url.starts_with("about:") {
                                tracing::warn!(
                                    "webview {label} still on about:blank after recreate, navigating to app url"
                                );
                                // 生产构建下应用首页 = http://tauri.localhost（tauri 自定义协议）
                                if let Ok(u) = tauri::Url::parse("http://tauri.localhost/index.html") {
                                    let _ = win.navigate(u);
                                }
                            }
                        });
                        tracing::info!("webview window {} recreated after renderer crash", w.label());
                        // 自动获得焦点，让用户立刻看到恢复的界面
                        let _ = w.set_focus();
                    }
                    Err(e) => {
                        diag(format!("rebuild window {:?} failed: {e}", cfg.label));
                        tracing::error!("failed to recreate webview window {}: {e}", cfg.label)
                    }
                }
            }
            // 4) 清理占位窗口——仅当真窗口重建成功才关 holder，否则保留 holder
            //    撑住主进程（避免「重建失败 + 关 holder → 窗口清空 → 主进程退出」）。
            diag(format!("rebuilt_any={rebuilt_any}"));
            if rebuilt_any {
                if let Some(h) = &holder2 {
                    diag("destroying holder".into());
                    let _ = h.destroy();
                }
            }
            // 5) 重建流程结束，清闩。即使重建失败（rebuilt_any=false）也清闩——
            //    下一 tick 会重新探测并重试；若闩一直不放，holder 撑着但没人
            //    再触发重建，就真死了。
            if let Some(st) = app3.try_state::<AppState>() {
                *st.webview_busy.lock().unwrap() = None;
            }
            diag("== recreate_webview exit ==".into());
        });
    });
}

/// Windows: 本进程是否还有存活的 WebView2 渲染子进程。
/// 用 Toolhelp32Snapshot 枚举 `msedgewebview2.exe` 且 ParentProcessID = 自身 PID
/// 的进程；有一个就算渲染进程组活着。不依赖 tauri/wry 的 COM 接口，稳。
#[cfg(windows)]
fn has_webview_renderer_child() -> bool {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let me = std::process::id();
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        // 拿不到快照：保守认为活着（避免误杀）
        return true;
    }
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut found = false;
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) } != 0;
    while ok {
        // 名称比较：msedgewebview2.exe（不区分大小写，Windows 文件系统不敏感）
        let name = String::from_utf16_lossy(
            &entry.szExeFile[..entry.szExeFile.iter().take_while(|&&c| c != 0).count()],
        );
        if entry.th32ParentProcessID == me && name.eq_ignore_ascii_case("msedgewebview2.exe") {
            found = true;
            break;
        }
        ok = unsafe { Process32NextW(snap, &mut entry) } != 0;
    }
    let _ = unsafe { windows_sys::Win32::Foundation::CloseHandle(snap) };
    found
}
