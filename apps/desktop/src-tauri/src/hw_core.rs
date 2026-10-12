//! 硬件采集内核域（从 hw.rs 拆分，2026-10-12）。
//! ps_capture / TTL 快照 / GPU 实时指标 / run_cli_capture / parse_fancmd_diag / hw_info。

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// PowerShell 执行器（psutil 共享实现，带 UTF-8 首行注入 + GBK 兜底）。
/// desktop 历史签名是 `(script, timeout)` 两参；共享 crate 提供一参版
/// `ps_capture(script)`（默认 30s）与 `ps_capture_timeout`。这里薄封装保持
/// 本文件全部调用点不变。
pub(crate) fn ps_capture(script: &str, timeout: Duration) -> Result<String, String> {
    diskpilot_psutil::ps_capture_timeout(script, timeout)
}
/// 硬件静态信息 TTL：CPU/内存/主板/BIOS/显卡等基本不变，5 分钟内直接复用，
/// 切页/多轮 AI 查询不再每次重跑 PowerShell。
const HW_SNAPSHOT_TTL: Duration = Duration::from_secs(300);
struct HwSnapshot {
    at: std::time::Instant,
    value: serde_json::Value,
}
static HW_SNAPSHOT: OnceLock<Mutex<Option<HwSnapshot>>> = OnceLock::new();
fn hw_snapshot_lock() -> &'static Mutex<Option<HwSnapshot>> {
    HW_SNAPSHOT.get_or_init(|| Mutex::new(None))
}

/// 读取硬件快照：TTL 内直接返回缓存，过期则重新跑 PowerShell 查询并刷新缓存。
/// force=true 时无视 TTL 强制重跑（前端刷新按钮），并把采集结果写回缓存。
pub(crate) fn hw_info_cached(force: bool) -> Result<serde_json::Value, String> {
    let now = std::time::Instant::now();
    if !force {
        if let Ok(guard) = hw_snapshot_lock().lock() {
            if let Some(snap) = guard.as_ref() {
                if now.duration_since(snap.at) < HW_SNAPSHOT_TTL {
                    return Ok(snap.value.clone());
                }
            }
        }
    }
    let raw = ps_capture(HW_INFO_PS, Duration::from_secs(30))?;
    let raw = raw.trim().trim_start_matches('\u{feff}');
    let mut v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("硬件信息解析失败：{e}"))?;
    attach_gpu_live(&mut v);
    if let Ok(mut guard) = hw_snapshot_lock().lock() {
        // at 用采集完成时刻而非函数开头时刻：spawn_blocking 排队/执行耗时
        // 不计入 TTL，5 分钟缓存才是真正的 5 分钟。
        *guard = Some(HwSnapshot {
            at: std::time::Instant::now(),
            value: v.clone(),
        });
    }
    Ok(v)
}

/// 给硬件信息 JSON 附 GPU 实时指标（普通权限可读），CIM 拿不到实时占用/温度：
/// N 卡走 nvidia-smi；N 卡缺席时退 AMD atiadlxx.dll ADL（同结构 JSON，前端零改动）；
/// 两通道都不可用 → 不附字段（前端回退「N 个传感器」）。
fn attach_gpu_live(v: &mut serde_json::Value) {
    if let Some(live) = pick_gpu_live(nvidia_smi_live(), amd_adl_live()) {
        v["gpu_live"] = live;
    }
}

/// GPU 实时指标选择决策（纯函数）：NVIDIA 优先，AMD 兜底；两通道都是 Err/None → None。
/// 抽出来锁「厂商双通道」决策矩阵，避免单测真的 spawn nvidia-smi / PowerShell。
pub(crate) fn pick_gpu_live(
    nv: Result<Option<serde_json::Value>, String>,
    adl: Result<Option<serde_json::Value>, String>,
) -> Option<serde_json::Value> {
    if let Ok(Some(g)) = nv {
        return Some(g);
    }
    adl.ok().flatten()
}

/// nvidia-smi 实时 GPU 指标（无 N 卡 / 未安装 nvidia-smi 时返回 None）。
/// 普通权限即可读取利用率/温度/显存，弥补 WMI 拿不到实时占用的缺口。
fn nvidia_smi_live() -> Result<Option<serde_json::Value>, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=name,utilization.gpu,temperature.gpu,memory.used,memory.total",
        "--format=csv,noheader,nounits",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 nvidia-smi 失败：{e}"))?;
    let mut raw = Vec::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = so.read_to_end(&mut raw);
    }
    let _ = child.wait();
    let line = String::from_utf8_lossy(&raw)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if line.is_empty() {
        return Ok(None);
    }
    let mut parts = line.split(',');
    let name = parts.next().unwrap_or("").trim();
    let util = parts
        .next()
        .unwrap_or("")
        .trim()
        .parse::<u64>()
        .unwrap_or(0);
    let temp = parts
        .next()
        .unwrap_or("")
        .trim()
        .parse::<u64>()
        .unwrap_or(0);
    let mem_used = parts
        .next()
        .unwrap_or("")
        .trim()
        .parse::<u64>()
        .unwrap_or(0);
    let mem_total = parts
        .next()
        .unwrap_or("")
        .trim()
        .parse::<u64>()
        .unwrap_or(0);
    Ok(Some(serde_json::json!({
        "name": name,
        "utilization_pct": util,
        "temperature_c": temp,
        "memory_used_bytes": mem_used.saturating_mul(1024 * 1024),
        "memory_total_bytes": mem_total.saturating_mul(1024 * 1024),
    })))
}

/// AMD atiadlxx.dll ADL 实时 GPU 指标（DLL 缺失 / 初始化失败 / 无 A 卡 → None）。
/// 与 `nvidia_smi_live()` 同结构的 JSON：N 卡缺席时用它顶上 `gpu_live`。
/// 探针脚本统一在共享 crate `diskpilot-psutil`（desktop 与 agent-server 共用，
/// 原双份 2927 字符完全相同，迁入防漂移）。PowerShell 子进程崩了 →
/// ps_capture Err → 外层吞 None → 静默降级，绝不报错。
fn amd_adl_live() -> Result<Option<serde_json::Value>, String> {
    let raw = ps_capture(diskpilot_psutil::ADL_PROBE_PS, Duration::from_secs(30))?;
    let raw = raw.trim().trim_start_matches('\u{feff}');
    let v: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let note = v
        .get("note")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if note != "OK" {
        return Ok(None);
    }
    let adapters = v.get("adapters").and_then(serde_json::Value::as_array);
    let Some(adapters) = adapters else {
        return Ok(None);
    };
    // 取第一个有实际读数的适配器（temp 或 util > 0），与 nvidia_smi_live 单值结构一致。
    for a in adapters {
        let temp = a
            .get("temp_c")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let util = a
            .get("gpu_util")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        if temp > 0 || util > 0 {
            return Ok(Some(serde_json::json!({
                "name": format!("AMD 显卡（ADL 适配器 {}）", a.get("index").and_then(serde_json::Value::as_u64).unwrap_or(0)),
                "utilization_pct": util,
                "temperature_c": temp,
                "memory_used_bytes": 0,
                "memory_total_bytes": 0,
            })));
        }
    }
    Ok(None)
}

const HW_INFO_PS: &str = r#"$ErrorActionPreference='SilentlyContinue'
$out=[ordered]@{}
$out.cpu=@(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors,MaxClockSpeed,CurrentClockSpeed,L2CacheSize,L3CacheSize,Manufacturer,VirtualizationFirmwareEnabled,LoadPercentage)
$perf=Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor | Where-Object { $_.Name -eq '_Total' }
$out.cpu_usage=@($perf | Select-Object PercentProcessorTime)
$out.gpu=@(Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion,DriverDate,CurrentRefreshRate,CurrentHorizontalResolution,CurrentVerticalResolution)
$out.memory=@(Get-CimInstance Win32_PhysicalMemory | Select-Object Capacity,Speed,ConfiguredClockSpeed,Manufacturer,PartNumber,DeviceLocator)
$out.system=@(Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer,Model,TotalPhysicalMemory,HypervisorPresent)
$out.bios=@(Get-CimInstance Win32_BIOS | Select-Object Manufacturer,SMBIOSBIOSVersion,ReleaseDate)
$out.board=@(Get-CimInstance Win32_BaseBoard | Select-Object Manufacturer,Product,SerialNumber)
$out.os=@(Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber,LastBootUpTime)
$out.disk=@(Get-CimInstance Win32_DiskDrive | Select-Object Model,Size,InterfaceType,Status)
$out.thermal=@(Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature | Select-Object InstanceName,CurrentTemperature)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 运行一个外部可执行文件并捕获 stdout（超时强杀，无窗口）。fancmd/nbfc 等 CLI 专用。
pub(crate) fn run_cli_capture(
    exe_path: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut cmd = std::process::Command::new(exe_path);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 {exe_path} 失败：{e}"))?;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("等待 {exe_path} 失败：{e}"));
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{exe_path} 执行超时（{}s）", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    let mut raw = Vec::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = so.read_to_end(&mut raw);
    }
    if let Some(mut se) = child.stderr.take() {
        let mut sink = Vec::new();
        let _ = se.read_to_end(&mut sink);
    }
    if !status.success() && raw.is_empty() {
        return Err(format!("{exe_path} 退出码 {:?}", status.code()));
    }
    match std::str::from_utf8(&raw) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => {
            #[cfg(windows)]
            {
                let (cow, _, _) = encoding_rs::GBK.decode(&raw);
                Ok(cow.into_owned())
            }
            #[cfg(not(windows))]
            Ok(String::from_utf8_lossy(&raw).into_owned())
        }
    }
}

/// 解析 fancmd diag json 的关键字段，供降级诊断与 AI 自修复参考。
pub(crate) fn parse_fancmd_diag(raw: &str) -> serde_json::Value {
    let raw = raw.trim().trim_start_matches('\u{feff}');
    let doc: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return serde_json::json!({ "parsed": false }),
    };
    let mut out = serde_json::json!({ "parsed": true });
    if let Some(drv) = doc.get("driver") {
        out["driver"] = drv.clone();
    }
    if let Some(rtc) = doc.get("rtc_ok") {
        out["rtc_ok"] = rtc.clone();
    }
    if let Some(sio) = doc.get("sio") {
        out["sio"] = sio.clone();
    }
    if let Some(fans) = doc.get("fans") {
        out["fans"] = fans.clone();
    }
    if let Some(w) = doc.get("writable") {
        out["writable"] = w.clone();
    }
    out
}
