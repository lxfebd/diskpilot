//! 全量传感器快照域（从 hw_thermal.rs 拆分，2026-10-12）。
//! collect_sensors / collect_sensors_fast + fancmd 探测与解析。

use crate::tools::v_str;
use serde_json::Value;
use std::io::Read;

use super::therm_temp::collect_temperature;

/// 非 Windows：快速传感器快照同样不可用。
#[cfg(not(windows))]
pub fn collect_sensors_fast() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

/// 全量传感器快照（AIDA64 同类：温度/风扇/电压/功耗/频率/负载），供内置 AI 体检。
///
/// 优先走 `fancmd sensors`（LibreHardwareMonitor 内核，与 FanControl 同款——能读
/// CPU 核心/主板/GPU 温度、风扇转速、电压、功耗、频率、负载；CPU 核心温度需 Ring0
/// 驱动+管理员）。若 CPU 温度传感器全 null 且当前非管理员，自动尝试
/// `fancmd elevate sensors`（ShellExecuteEx runas 弹一次 UAC，管理员子进程
/// 把输出写临时文件回传）——拿到全量 CPU 温度；用户取消 UAC 则如实降级并提示。
/// fancmd 不可用时退回系统自带通道（ACPI 热区 + SMART 磁盘温度 + nvidia-smi GPU 温度）。
///
/// **全部只读**：只开传感器快照并输出文本，不写任何寄存器。
#[cfg(windows)]
pub fn collect_sensors() -> Result<String, String> {
    // 0) 优先 HWiNFO 共享内存全量快照：普通权限即可读到 CPU/GPU/主板温度、
    //    风扇、电压、功耗、频率、负载——比 fancmd（需 Ring0+管理员）范围更全、
    //    且完全不依赖提权。HWiNFO 未运行/未启用共享内存 → 自动回退 fancmd。
    if let Ok(snapshot) = crate::tools::hwinfo::read_hwinfo_text() {
        // 读数可能高达数百条，截断到 120 条防 AI 上下文爆炸，但保留分组与完整性提示。
        let mut lines: Vec<&str> = Vec::new();
        for (i, line) in snapshot.lines().enumerate() {
            if i >= 121 {
                break;
            }
            lines.push(line);
        }
        let mut s = lines.join("\n");
        // 让 AI 知道这是来自 HWiNFO 的实时传感器，可信度高。
        let cnt = snapshot.lines().count().saturating_sub(1); // 首行是标题
        s.push_str("\n说明：数据来自 HWiNFO 共享内存（普通权限，无 Ring0/管理员依赖），\n  \
             读数界面与 HWiNFO 一致；HWiNFO 未运行时自动降级 LibreHardwareMonitor/fancmd 与 ACPI 通道。");
        if cnt > 120 {
            s.push_str(&format!(
                "\n提示：本快照截断至 120 条（共 {cnt} 条），可单独询问某组详细读数。"
            ));
        }
        return Ok(s);
    }
    // 1) 自研 SuperIO 直读全量：HWiNFO 没运行时，用 inpoutx64 驱动（已装则普通权限可用）
    //    直读 ITE/Nuvoton/Winbond 环境寄存器（温度/风扇/PWM）。
    //    HWiNFO + 驱动都没有时才走到 fancmd（LHM，需管理员）。
    if let Ok(s) = crate::tools::superio::read_text() {
        return Ok(s);
    }
    // 2) 优先 fancmd sensors（LHM 全量）
    if let Some(exe) = crate::tools::selfheal::find_fancmd() {
        let direct = run_fancmd_sensors(&exe);
        match direct {
            Ok(s) if !cpu_temps_are_all_null(&s) => return Ok(s),
            Ok(s) if !crate::tools::selfheal::is_admin() => {
                // CPU 温度全 null 且非管理员：尝试 `fancmd elevate sensors` 提权重取全量。
                // 注意：elevate 会弹 UAC（ShellExecuteEx runas）。交互式桌面用户可点，
                // 但 AI 自动调用场景 UAC 无人可点会挂住 —— 因此这里带 5s 超时，
                // 超时（或 UAC 被拒/取消）一律降级返回已有快照，不阻塞调用方。
                let started = std::time::Instant::now();
                let timeout = std::time::Duration::from_secs(5);
                let elevate = std::process::Command::new(&exe)
                    .arg("elevate")
                    .arg("sensors")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                match elevate {
                    Ok(mut c) => {
                        let mut raw = String::new();
                        // 带超时等退出 + 读 stdout
                        let mut got_raw = false;
                        loop {
                            match c.try_wait() {
                                Ok(Some(_)) => {
                                    if let Some(mut o) = c.stdout.take() {
                                        let _ = o.read_to_string(&mut raw);
                                    }
                                    got_raw = true;
                                    break;
                                }
                                Ok(None) => {}
                                Err(_) => {
                                    let _ = c.kill();
                                    let _ = c.wait();
                                    break;
                                }
                            }
                            if started.elapsed() >= timeout {
                                let _ = c.kill();
                                let _ = c.wait();
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(40));
                        }
                        if got_raw && !raw.trim().is_empty() && raw.trim().starts_with('[') {
                            return Ok(format_sensor_snapshot(&raw));
                        }
                        // 超时 / UAC 取消 / 无有效输出 → 降级
                        if started.elapsed() < timeout {
                            return collect_temperature();
                        }
                        return Ok(s); // 超时：保留非提权快照，不阻塞
                    }
                    Err(_) => return Ok(s), // elevate 启动失败：保留非提权快照
                }
            }
            Ok(s) => return Ok(s),
            Err(_) => {}
        }
    }
    // 2) 降级：系统自带通道
    collect_temperature()
}

/// 快速传感器快照（只走系统自带通道，不碰 fancmd）。
///
/// ACPI 热区 + NVIDIA GPU + SMART 磁盘温度，实测 ~3s，比 fancmd（~5-17s）
/// 快得多。给 `system_report` / `sensor_trend` / `sensor_alert` 这类「AI 对话内
/// 快速响应」的场景用——报告生成不该为细节传感器等十几秒。用户主动打开
/// 传感器页/甜甜圈时仍用 `collect_sensors`（fancmd 全量）。
#[cfg(windows)]
pub fn collect_sensors_fast() -> Result<String, String> {
    collect_temperature()
}

/// 直接跑 `fancmd sensors` 并返回原样 JSON。
///
/// **必须带超时**：fancmd 加载 LibreHardwareMonitor 内核驱动 + 枚举传感器，
/// 在非管理员/驱动加载慢的机器上可能挂住（实测 13-17s 才回），拖垮
/// `system_report` / `sensor_trend`。8s 上限，超时 kill 并报错 → 调用方
/// 降级到 ACPI 通道（`collect_temperature` 仅 ~3s），保证 AI 对话不阻塞。
/// 注意：fancmd sensors 单次实测 4.4s（i7-13700KF + Z690），5s 超时 margin
/// 仅 570ms、进程 spawn/管道抖动即误杀（2026-09-14 根因）→ 放宽到 8s。
#[cfg(windows)]
pub(crate) fn run_fancmd_sensors(exe: &str) -> Result<String, String> {
    use std::io::Read;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("sensors")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut c = cmd.spawn().map_err(|e| e.to_string())?;
    // 后台线程先读 stdout：fancmd 全量 JSON 可达数十 KB，若等退出后才读，
    // 进程写满 64KB 管道缓冲会阻塞，与「等退出」互等死锁（实测挂到超时）。
    let raw = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    if let Some(mut so) = c.stdout.take() {
        let raw2 = std::sync::Arc::clone(&raw);
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = so.read_to_string(&mut s);
            if let Ok(mut g) = raw2.lock() {
                *g = s;
            }
        });
    }
    // 有超时的等待：fancmd 正常 4-5s 出结果，8s 无回音视为卡死/过慢，直接降级
    let started = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(8);
    let status = loop {
        match c.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => {
                let _ = c.kill();
                let _ = c.wait();
                return Err(format!("等待 fancmd sensors 失败：{e}"));
            }
        }
        if started.elapsed() >= timeout {
            let _ = c.kill();
            let _ = c.wait();
            return Err(format!("fancmd sensors 执行超时（{}s）", timeout.as_secs()));
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    };
    // 等读取线程收完残留输出（kill 后 read_to_string 会立即返回 EOF）
    std::thread::sleep(std::time::Duration::from_millis(50));
    let raw = raw.lock().map(|g| g.clone()).unwrap_or_default();
    if status.success() && (raw.trim().is_empty() || !raw.trim().starts_with('[')) {
        return Err("fancmd sensors 无有效输出".into());
    }
    Ok(raw)
}

/// 解析 fancmd sensors JSON，判断 CPU 温度（含 Core/CPU 命名）是否全部无值。
#[cfg(windows)]
fn cpu_temps_are_all_null(raw: &str) -> bool {
    let Ok(groups) = serde_json::from_str::<Vec<Value>>(raw.trim()) else {
        return false;
    };
    let mut named = 0usize;
    let mut with_value = 0usize;
    for g in &groups {
        let hw_name = v_str(g, "hardware");
        if !hw_name.contains("CPU") && !hw_name.contains("Core") {
            continue;
        }
        let Some(gm) = g.get("groups").and_then(Value::as_object) else {
            continue;
        };
        if let Some(items) = gm.get("Temperature").and_then(Value::as_array) {
            named += items.len();
            with_value += items
                .iter()
                .filter(|i| i.get("value").and_then(Value::as_f64).is_some())
                .count();
        }
    }
    named > 0 && with_value == 0
}

/// 从 fancmd sensors JSON 提取 CPU 温度（Core Max / CPU Package 等有值的核心温度），
/// 返回 `(传感器名, 温度℃)` 列表；无值或无 CPU 温度组 → `None`（调用方静默跳过）。
#[cfg(windows)]
pub(crate) fn extract_cpu_temps_from_sensor_json(raw: &str) -> Option<Vec<(String, f64)>> {
    let groups: Vec<Value> = serde_json::from_str(raw.trim()).ok()?;
    let mut out: Vec<(String, f64)> = Vec::new();
    for g in &groups {
        let hw_name = v_str(g, "hardware");
        if !hw_name.contains("CPU") && !hw_name.contains("Core") {
            continue;
        }
        let Some(gm) = g.get("groups").and_then(Value::as_object) else {
            continue;
        };
        let Some(items) = gm.get("Temperature").and_then(Value::as_array) else {
            continue;
        };
        for i in items {
            let nm = v_str(i, "name");
            // 只看聚合值（Core Max / CPU Package / Core Average），单个核心的 Core #N 太碎，
            // 合并成「所有核心」一条更有用。
            if !nm.contains("Max") && !nm.contains("Package") && !nm.contains("Average") {
                continue;
            }
            let v = i.get("value").and_then(Value::as_f64)?;
            if v > 0.0 && v <= 120.0 {
                out.push((nm.clone(), v));
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 把 `fancmd sensors` 的 JSON 快照转成人类可读的短文（按硬件分组，
/// 只列有价值传感器：温度/风扇/电压/功耗/频率/负载，跳过全空组）。
/// CPU 温度全 null 时附加提权诊断（区分已提权=硬件限制 vs 未提权=需管理员）。
#[cfg(windows)]
fn format_sensor_snapshot(raw: &str) -> String {
    match serde_json::from_str::<Vec<Value>>(raw.trim()) {
        Ok(groups) if !groups.is_empty() => {
            let mut out = String::from("传感器快照（LibreHardwareMonitor）：\n");
            let mut any = false;
            let mut cpu_temp_count = 0usize;
            let mut cpu_temp_named = 0usize;
            for g in &groups {
                let hw_name = v_str(g, "hardware");
                let groups_map = g.get("groups").and_then(Value::as_object);
                let Some(gm) = groups_map else { continue };
                let mut hw_lines: Vec<String> = Vec::new();
                for (key, items) in gm {
                    let Some(items) = items.as_array() else {
                        continue;
                    };
                    let kind = key.as_str();
                    // 只看这些传感器类型；Data/SmallData/Throughput 太碎跳过
                    if !matches!(
                        kind,
                        "Temperature" | "Fan" | "Voltage" | "Power" | "Clock" | "Load"
                    ) {
                        continue;
                    }
                    // 统计 CPU 温度：硬件名含 CPU / Core 的 Temperature 条目
                    if kind == "Temperature"
                        && (hw_name.contains("CPU") || hw_name.contains("Core"))
                    {
                        cpu_temp_named += items.len();
                        cpu_temp_count += items
                            .iter()
                            .filter(|i| i.get("value").and_then(Value::as_f64).is_some())
                            .count();
                    }
                    let vals: Vec<String> = items
                        .iter()
                        .filter_map(|i| {
                            let name = v_str(i, "name");
                            let value = i.get("value").and_then(Value::as_f64)?;
                            let unit = match kind {
                                "Temperature" => "℃",
                                "Fan" => " RPM",
                                "Voltage" => " V",
                                "Power" => " W",
                                "Clock" => " MHz",
                                _ => "%",
                            };
                            Some(format!("{name}={value:.1}{unit}"))
                        })
                        .collect();
                    if !vals.is_empty() && vals.len() <= 12 {
                        hw_lines.push(format!("  {kind}: {}", vals.join(", ")));
                        any = true;
                    }
                }
                if !hw_lines.is_empty() {
                    out.push_str(&format!("[{hw_name}]\n{}\n", hw_lines.join("\n")));
                }
            }
            if !any {
                out.push_str("  （未读到有价值传感器，可能缺 Ring0 驱动/管理员权限）\n");
            }
            // CPU 温度诊断：传感器已枚举但全无值 → 区分「未提权」与「硬件限制」
            let elevated = crate::tools::selfheal::is_admin();
            if cpu_temp_named > 0 && cpu_temp_count == 0 {
                if elevated {
                    out.push_str("\n⚠️ CPU 温度传感器已枚举但读不到值：已具备管理员权限仍无数据，说明 BIOS/固件未经主板 SuperIO 暴露 CPU 测温（现代 CPU 常走 Package MSR，需 LHM 驱动通道），属硬件平台限制；GPU/内存/功耗等其余传感器不受影响。\n");
                } else {
                    out.push_str("\n⚠️ CPU 温度需要管理员权限（Ring0 驱动读 MSR）才能读到：当前进程未提权，Core Max/Average 等传感器已枚举但无值。以管理员身份运行 DiskPilot（或 fancmd elevate sensors 单独提权）后即可读到。GPU/内存等其余传感器不受影响。\n");
                }
            } else if cpu_temp_count > 0 {
                // CPU 温度有值但其余全空（极罕见）：不额外提示
            }
            out.push_str("\n说明：传感器来自 LibreHardwareMonitor（FanControl 同款内核）；全部只读，未改动任何寄存器。");
            out
        }
        _ => collect_temperature().unwrap_or_else(|e| format!("传感器解析失败：{e}")),
    }
}

#[cfg(not(windows))]
pub fn collect_sensors() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}
