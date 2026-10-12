//! 压测/温度监控域（从 hw.rs 拆分，2026-10-12）。
//! HwTestProgress / HwTestReport / hw_run_test / hw_run_test_blocking / hw_stop_test。
//! 温度采样（sample_thermal_c / read_thermal_max_c）在 `thermal` 子模块。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};

use crate::toolbelt::toolbelt_explicit_root;
use crate::AppState;

#[path = "hw_test/thermal.rs"]
mod thermal;
pub(crate) use thermal::sample_thermal_c;

#[derive(serde::Serialize, Clone)]
pub(crate) struct HwTestProgress {
    test_type: String,
    tool: String,
    elapsed_secs: u64,
    limit_secs: u64,
    temp_c: Option<f32>,
    peak_c: Option<f32>,
    /// true = 测试已结束（终态事件）。前端收到后应清除监控条。
    done: bool,
}

#[derive(serde::Serialize)]
pub(crate) struct HwTestReport {
    test_type: String,
    tool: String,
    command_line: String,
    duration_run_secs: u64,
    stop_reason: String,
    exit_code: Option<i32>,
    temp_peak_c: Option<f32>,
    temp_monitor_active: bool,
    bench_excerpt: Option<String>,
    note: String,
}

/// 运行硬件压力/基准测试。永远要求 user_confirmed=true：
/// 后端在 spawn 前就拦，AI 永远不能绕过确认门。
/// 超时强杀 + 温度熔断（能读到传感器时）+ 用户随时可停止。
#[tauri::command]
pub(crate) async fn hw_run_test(
    app: AppHandle,
    state: State<'_, AppState>,
    test_type: String,
    duration_seconds: Option<u64>,
    temp_limit_celsius: Option<f32>,
    user_confirmed: Option<bool>,
) -> Result<HwTestReport, String> {
    if user_confirmed != Some(true) {
        return Err(
            "硬件压力/基准测试必须由用户明确确认后执行（user_confirmed=true）。请先在确认面板中让用户确认。".into(),
        );
    }
    let tt = test_type.clone();
    let app2 = app.clone();
    // 每实例私有 stop flag：先登记到 AppState.hw_stops，让 hw_stop_test
    // 能按「测试实例」精准停止；结束后从列表移除，避免泄漏。
    let stop = Arc::new(AtomicBool::new(false));
    state.hw_stops.lock().unwrap().push(stop.clone());
    let result = tokio::task::spawn_blocking({
        let stop = stop.clone();
        move || hw_run_test_blocking(app2, tt, duration_seconds, temp_limit_celsius, stop)
    })
    .await
    .map_err(|e| e.to_string())?;
    state
        .hw_stops
        .lock()
        .unwrap()
        .retain(|f| !Arc::ptr_eq(f, &stop));
    result
}

fn hw_run_test_blocking(
    app: AppHandle,
    test_type: String,
    duration_seconds: Option<u64>,
    temp_limit_celsius: Option<f32>,
    stop: Arc<AtomicBool>,
) -> Result<HwTestReport, String> {
    let (tool, mut args, default_dur) = match test_type.as_str() {
        "cpu_stress" => ("Prime95", vec!["-t".to_string()], 300u64),
        "gpu_stress" => ("FurMark", vec![], 300u64),
        "gpu_benchmark" => (
            "FurMark",
            vec![
                "--demo".to_string(),
                "furmark-gl".to_string(),
                "--p1080".to_string(),
                "--no-score-box".to_string(),
            ],
            240u64,
        ),
        "cpu_benchmark" => ("AIDA64", vec![], 300u64),
        _ => {
            return Err(format!(
                "暂不支持的测试类型「{test_type}」。可用：cpu_stress / gpu_stress / gpu_benchmark / cpu_benchmark（内存专项与磁盘专项暂未开放，可改用 run_cli_tool 跑 TM5 / CrystalDiskMark）。"
            ));
        }
    };
    let dur = duration_seconds.unwrap_or(default_dur).clamp(30, 1800);
    let temp_limit = temp_limit_celsius.unwrap_or(90.0).clamp(60.0, 105.0);

    match test_type.as_str() {
        "gpu_stress" => {
            args.extend(
                [
                    "--demo",
                    "furmark-gl",
                    "--benchmark",
                    "--width",
                    "1920",
                    "--height",
                    "1080",
                ]
                .iter()
                .map(|s| s.to_string()),
            );
            args.push("--max-time".to_string());
            args.push(dur.to_string());
        }
        "cpu_benchmark" => {
            let bench_path = std::env::temp_dir()
                .join(format!(
                    "diskpilot_bench_{}.txt",
                    chrono::Utc::now().timestamp_millis()
                ))
                .to_string_lossy()
                .into_owned();
            args.extend(
                ["/R", bench_path.as_str(), "/TEXT", "/BENCH", "/SILENT"]
                    .iter()
                    .map(|s| s.to_string()),
            );
        }
        _ => {}
    }

    let explicit = toolbelt_explicit_root(&app);
    let root =
        diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())?;
    let cmd = diskpilot_toolbelt::toolbelt_command_for(tool, &args, &root, None)
        .map_err(|e| e.to_string())?;
    let command_line = format!("{} {}", cmd.program.display(), cmd.args.join(" "));

    // 不再在 spawn 前清 stop flag：flag 由 hw_run_test 以 new(false) 创建，
    // 若用户在排队期间已点停止（flag 已被 hw_stop_test 置 true），
    // 这里清掉会吞掉这次停止请求，测试会跑到自然结束。
    use std::process::Stdio;
    let mut command = std::process::Command::new(&cmd.program);
    command
        .args(&cmd.args)
        .current_dir(&cmd.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动测试工具失败：{e}（{}）", cmd.program.display()))?;

    // stdout drain 线程：压测工具（Prime95/FurMark/AIDA64）输出量大且不捕获。
    // 若不读走，管道 buffer 写满后子进程会阻塞在 write 上 → try_wait 永远 None，
    // 直到 limit_total 触发误报 stop_reason="deadline"。基准结果走文件
    // （_scores.csv / diskpilot_bench_*.txt），stdout 内容直接丢弃。
    if let Some(mut so) = child.stdout.take() {
        std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = [0u8; 8192];
            loop {
                match so.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        });
    }

    let started = std::time::Instant::now();
    let limit_total = Duration::from_secs(dur + 60);
    let mut peak: Option<f32> = None;
    let mut temp_monitor = true;
    let mut temp_checked = std::time::Instant::now();
    let mut stop_reason = "finished".to_string();
    let mut exit_code: Option<i32> = None;
    let mut bench_excerpt: Option<String> = None;

    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                exit_code = st.code();
                // 工具自己退出：exit 0 = 正常跑完；非 0 或无码 = 异常退出/崩溃，
                // 如实报告而不是伪装成「自然结束」，方便用户排查工具配置。
                if exit_code != Some(0) {
                    stop_reason = "crashed".to_string();
                }
                break;
            }
            Ok(None) => {}
            Err(_) => {
                stop_reason = "crashed".to_string();
                break;
            }
        }
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            stop_reason = "user_stopped".to_string();
            break;
        }
        if started.elapsed() >= limit_total {
            let _ = child.kill();
            let _ = child.wait();
            stop_reason = "deadline".to_string();
            break;
        }
        if temp_checked.elapsed() >= Duration::from_secs(2) {
            temp_checked = std::time::Instant::now();
            match sample_thermal_c() {
                Some(c) => {
                    if peak.is_none_or(|p| c > p) {
                        peak = Some(c);
                    }
                    if c >= temp_limit {
                        let _ = child.kill();
                        let _ = child.wait();
                        stop_reason = "temp_limit".to_string();
                        break;
                    }
                    let _ = app.emit(
                        "hw-test-progress",
                        &HwTestProgress {
                            test_type: test_type.clone(),
                            tool: cmd.tool.clone(),
                            elapsed_secs: started.elapsed().as_secs(),
                            limit_secs: dur,
                            temp_c: Some(c),
                            peak_c: peak,
                            done: false,
                        },
                    );
                }
                None => {
                    temp_monitor = false;
                    let _ = app.emit(
                        "hw-test-progress",
                        &HwTestProgress {
                            test_type: test_type.clone(),
                            tool: cmd.tool.clone(),
                            elapsed_secs: started.elapsed().as_secs(),
                            limit_secs: dur,
                            temp_c: None,
                            peak_c: peak,
                            done: false,
                        },
                    );
                }
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    let _ = child.wait();
    // 终态事件：测试结束（自然结束/停止/熔断/崩溃/超时）后必须补一发 done=true，
    // 否则前端监控条永远停在最后一条进度，无事件可让 useHwTestMonitor 清理。
    let _ = app.emit(
        "hw-test-progress",
        &HwTestProgress {
            test_type: test_type.clone(),
            tool: cmd.tool.clone(),
            elapsed_secs: started.elapsed().as_secs(),
            limit_secs: dur,
            temp_c: peak,
            peak_c: peak,
            done: true,
        },
    );

    let duration_run_secs = started.elapsed().as_secs();

    if test_type == "cpu_benchmark" {
        let mut best: Option<(i64, String)> = None;
        if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
            for ent in rd.flatten() {
                let p = ent.path();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with("diskpilot_bench_") && name.ends_with(".txt") {
                    if let Ok(md) = ent.metadata() {
                        let ts = md
                            .modified()
                            .ok()
                            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        if best.as_ref().is_none_or(|(t, _)| ts > *t) {
                            best = Some((ts, name.to_string()));
                        }
                    }
                }
            }
        }
        if let Some((_, name)) = best {
            let p = std::env::temp_dir().join(name);
            if let Ok(txt) = std::fs::read_to_string(&p) {
                bench_excerpt = Some(txt.trim().chars().take(4000).collect::<String>());
                let _ = std::fs::remove_file(&p);
            }
        }
    } else if test_type == "gpu_benchmark" {
        let scores = cmd.cwd.join("_scores.csv");
        if let Ok(txt) = std::fs::read_to_string(&scores) {
            let last = txt.lines().rev().find(|l| !l.trim().is_empty());
            if let Some(l) = last {
                bench_excerpt = Some(format!(
                    "[FurMark 基准得分] {}",
                    l.trim().chars().take(600).collect::<String>()
                ));
            }
        }
    }

    let note = match stop_reason.as_str() {
        "deadline" => {
            "已按设定时长结束测试并强制结束进程（等效工具箱 start.bat 的 taskkill）。".to_string()
        }
        "temp_limit" => format!("温度达到 {}℃ 上限，已自动停止以保护硬件。", temp_limit),
        "user_stopped" => "用户在监控条上点击了停止，已终止测试。".to_string(),
        "crashed" => match exit_code {
            Some(code) => {
                format!("测试工具异常退出（退出码 {code}）。若反复出现，请检查该工具的配置或参数。")
            }
            None => "测试工具异常退出（无退出码，可能被外部结束）。若反复出现，请检查该工具的配置或参数。"
                .to_string(),
        },
        _ => "测试自然结束（工具自行退出）。".to_string(),
    };

    Ok(HwTestReport {
        test_type,
        tool: cmd.tool.clone(),
        command_line,
        duration_run_secs,
        stop_reason,
        exit_code,
        temp_peak_c: peak,
        temp_monitor_active: temp_monitor,
        bench_excerpt,
        note,
    })
}

/// 用户随时可停止进行中的压测。遍历当前登记的所有测试实例 flag：
/// 并发启动多个测试时每个都会被置 true，不再互相覆盖。
#[tauri::command]
pub(crate) fn hw_stop_test(state: State<'_, AppState>) {
    let flags: Vec<Arc<AtomicBool>> = state.hw_stops.lock().unwrap().clone();
    for f in flags {
        f.store(true, Ordering::Relaxed);
    }
}
