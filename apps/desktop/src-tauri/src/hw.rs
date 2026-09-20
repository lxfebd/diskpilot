//! 硬件检测/受控压测/报告（Tauri 命令层）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use tauri::{AppHandle, Emitter, State};

use crate::toolbelt::toolbelt_explicit_root;
use crate::AppState;
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
/// 硬盘健康 TTL：SMART 通电时间/温度/磨损不会秒变，5 分钟缓存。
const HW_DISK_TTL: Duration = Duration::from_secs(300);
struct HwDiskSnapshot {
    at: std::time::Instant,
    value: serde_json::Value,
}
static HW_DISK_SNAPSHOT: OnceLock<Mutex<Option<HwDiskSnapshot>>> = OnceLock::new();
fn hw_disk_snapshot_lock() -> &'static Mutex<Option<HwDiskSnapshot>> {
    HW_DISK_SNAPSHOT.get_or_init(|| Mutex::new(None))
}

/// 读取硬件快照：TTL 内直接返回缓存，过期则重新跑 PowerShell 查询并刷新缓存。
/// force=true 时无视 TTL 强制重跑（前端刷新按钮），并把采集结果写回缓存。
fn hw_info_cached(force: bool) -> Result<serde_json::Value, String> {
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
fn pick_gpu_live(
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
/// 实现：PowerShell Add-Type 动态 P/Invoke（零新增 Rust 依赖），签名对齐 ADL SDK
/// 官方头文件（ADL_GetTemperature 为 2 参数，个别旧 DLL 用 4 参数变体容错）；
/// 温度单位按值域启发式（>300 视为千分之一摄氏度）。PowerShell 子进程崩了 →
/// ps_capture Err → 外层吞 None → 静默降级，绝不报错。
fn amd_adl_live() -> Result<Option<serde_json::Value>, String> {
    let raw = ps_capture(ADL_PROBE_PS, Duration::from_secs(30))?;
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

/// AMD ADL 探针脚本：atiadlxx.dll P/Invoke 读 A 卡温度/占用。
/// 签名与 `crates/agent-server/src/tools/hw.rs` 的 `ADL_PROBE_PS` 保持一致（双 EntryPoint
/// 容错 + 千分位温度换算）。DLL 不存在 → `note='NO_AMD_ADL'`，静默降级。
const ADL_PROBE_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$dll='C:\Windows\System32\atiadlxx.dll'
$result=@{ init_rc=-1; note=''; adapters=@() }
if(-not (Test-Path $dll)) {
  $result.note='NO_AMD_ADL'
  $result | ConvertTo-Json -Depth 4 -Compress
  exit 0
}
try {
  Add-Type -Namespace DiskPilot -Name ADL -TypeDefinition @'
using System.Runtime.InteropServices;
public static class ADL {
  public delegate System.IntPtr ADL_Main_Memory_Alloc(int size);
  public static System.IntPtr Alloc(int size) { return System.Runtime.InteropServices.Marshal.AllocHGlobal(size); }
  [DllImport("atiadlxx.dll", CallingConvention=CallingConvention.StdCall)]
  public static extern int ADL_Main_Control_Create(ADL_Main_Memory_Alloc cb, int iEnumConnectedAdapters);
  [DllImport("atiadlxx.dll", CallingConvention=CallingConvention.StdCall)]
  public static extern int ADL_Adapter_NumberOfAdapters_Get(out int lpNumAdapters);
  [DllImport("atiadlxx.dll", CallingConvention=CallingConvention.StdCall, EntryPoint="ADL_GetTemperature")]
  public static extern int ADL_GetTemperature2(int iAdapterIndex, out int lpTemperature);
  [DllImport("atiadlxx.dll", CallingConvention=CallingConvention.StdCall, EntryPoint="ADL_GetTemperature")]
  public static extern int ADL_GetTemperature4(int iAdapterIndex, out int lpTemperature, int flag, int unused);
  [DllImport("atiadlxx.dll", CallingConvention=CallingConvention.StdCall)]
  public static extern int ADL_GLUtilization_Get(int iAdapterIndex, out int gpu, out int mem, out int eng);
}
'@
} catch {
  $result.note='ADD_TYPE_FAILED'
  $result | ConvertTo-Json -Depth 4 -Compress
  exit 0
}
$create=[DiskPilot.ADL]::ADL_Main_Control_Create([DiskPilot.ADL+ADL_Main_Memory_Alloc][DiskPilot.ADL]::Alloc, 0)
if($create -ne 0) {
  $result.init_rc=[int]$create
  $result.note='CREATE_FAILED'
  $result | ConvertTo-Json -Depth 4 -Compress
  exit 0
}
$num=0
$rc=[DiskPilot.ADL]::ADL_Adapter_NumberOfAdapters_Get([ref]$num)
if($rc -ne 0 -or $num -le 0) {
  $result.init_rc=[int]$rc
  $result.note='ENUM_FAILED'
  $result | ConvertTo-Json -Depth 4 -Compress
  exit 0
}
$list=@()
for($i=0; $i -lt $num; $i++) {
  $o=[ordered]@{ index=[int]$i }
  $temp=0; $trc=-1
  $trc=[DiskPilot.ADL]::ADL_GetTemperature2($i,[ref]$temp)
  if($trc -ne 0) { $trc=[DiskPilot.ADL]::ADL_GetTemperature4($i,[ref]$temp,0,0) }
  if($trc -eq 0 -and $temp -ne 0) {
    if($temp -gt 300) { $o.temp_c=[int][Math]::Round($temp/1000.0) } else { $o.temp_c=[int]$temp }
  } else { $o.temp_c=$null }
  $gpu=0; $mem=0; $eng=0
  $urc=[DiskPilot.ADL]::ADL_GLUtilization_Get($i,[ref]$gpu,[ref]$mem,[ref]$eng)
  if($urc -eq 0) { $o.gpu_util=[int]$gpu; $o.mem_util=[int]$mem; $o.engine_util=[int]$eng } else { $o.gpu_util=$null; $o.mem_util=$null; $o.engine_util=$null }
  $list += $o
}
$result.init_rc=[int]$create
$result.note='OK'
$result.adapters=$list
$result | ConvertTo-Json -Depth 4 -Compress
"#;
/// 运行一段 PowerShell 脚本并捕获 stdout（CREATE_NO_WINDOW，超时强杀）。
/// 硬件查询全走 CIM，不需要管理员权限；中文系统上控制台输出是 GBK，
/// UTF-8 失败时按 GBK 解码兜底。
pub(crate) fn ps_capture(script: &str, timeout: Duration) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 PowerShell 失败：{e}"))?;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("等待 PowerShell 失败：{e}"));
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("PowerShell 查询超时（{}s）", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(60));
    };
    let mut raw = Vec::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = so.read_to_end(&mut raw);
    }
    // 进程一退出就不存在 std::process::Child::try_wait 返回 Ok(None)
    // 的可能；但 kill 过（超时/错误分支）的进程 stderr 可能带内容，
    // 这里统一把 stderr 读走，避免管道 buffer 残留导致句柄泄漏。
    if let Some(mut se) = child.stderr.take() {
        let mut sink = Vec::new();
        let _ = se.read_to_end(&mut sink);
    }
    if !status.success() && raw.is_empty() {
        return Err(format!("PowerShell 退出码 {:?}", status.code()));
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
fn run_cli_capture(exe_path: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = std::process::Command::new(exe_path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
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
fn parse_fancmd_diag(raw: &str) -> serde_json::Value {
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

const HW_DISK_PS: &str = r#"$ErrorActionPreference='SilentlyContinue'
$disks=Get-PhysicalDisk
$out=[ordered]@{
  disks=@($disks | ForEach-Object { $rc=$_ | Get-StorageReliabilityCounter; [ordered]@{ friendly=$_.FriendlyName; media=$_.MediaType; bus=$_.BusType; size=$_.Size; health=[string]$_.HealthStatus; operational=($_.OperationalStatus -join ','); is_boot=$_.IsBoot; power_on_hours=$rc.PowerOnHours; temperature_c=$rc.Temperature; wear_pct=$rc.Wear; read_errors=$rc.ReadErrorsTotal; write_errors=$rc.WriteErrorsTotal } })
  reliability=@(@($disks | Get-StorageReliabilityCounter -ErrorAction SilentlyContinue).Count -gt 0)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 读 CPU/GPU/内存/硬盘/主板/BIOS/系统/温度传感器，零确认。
/// 走 hardware_snapshot TTL 缓存（5 分钟），AI 多轮重复查询不再每次跑 PowerShell。
/// refresh=true 时无视 TTL 强制重跑（用户手动刷新）。
/// 读不到的字段一律 null，不编造。
#[tauri::command]
pub(crate) async fn hw_info(refresh: Option<bool>) -> Result<serde_json::Value, String> {
    let force = refresh.unwrap_or(false);
    tokio::task::spawn_blocking(move || hw_info_cached(force))
        .await
        .map_err(|e| e.to_string())?
}

/// 读硬盘健康（WMI + CrystalDiskInfo 补充），带 5 分钟缓存。
/// WMI 部分每次都便宜，但 CrystalDiskInfo /CopyExit 每次都要启动真实程序，
/// 切页/多轮查询时必须复用上次结果。force=true 时无视 TTL 强制重跑（手动刷新）。
fn hw_disk_cached(app: &AppHandle, force: bool) -> Result<serde_json::Value, String> {
    let now = std::time::Instant::now();
    if !force {
        if let Ok(guard) = hw_disk_snapshot_lock().lock() {
            if let Some(snap) = guard.as_ref() {
                if now.duration_since(snap.at) < HW_DISK_TTL {
                    return Ok(snap.value.clone());
                }
            }
        }
    }
    let explicit = toolbelt_explicit_root(app);
    let v = hw_disk_collect(explicit.as_deref())?;
    if let Ok(mut guard) = hw_disk_snapshot_lock().lock() {
        // at 用采集完成时刻，排队/执行耗时不计入 TTL（与 hw_info_cached 同口径）。
        *guard = Some(HwDiskSnapshot {
            at: std::time::Instant::now(),
            value: v.clone(),
        });
    }
    Ok(v)
}

fn hw_disk_collect(explicit: Option<&std::path::Path>) -> Result<serde_json::Value, String> {
    let raw = ps_capture(HW_DISK_PS, Duration::from_secs(30))?;
    let raw = raw.trim().trim_start_matches('\u{feff}');
    let mut v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("硬盘健康解析失败：{e}"))?;
    // 补一份 CrystalDiskInfo /CopyExit 产出的 DiskInfo.txt 原始字段，
    // WMI 拿不到 SMART 时（无管理员权限）可作为补充参考。
    let mut cdi: Vec<serde_json::Value> = Vec::new();
    if let Ok(root) = diskpilot_toolbelt::find_tools_root(explicit) {
        if let Some(t) = diskpilot_toolbelt::find("crystaldiskinfo") {
            if let Some(rel) = t.exe_rel.as_deref() {
                let exe = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
                if exe.is_file() {
                    let cmd = diskpilot_toolbelt::ResolvedCommand {
                        tool: t.name.clone(),
                        category: t.category.clone(),
                        program: exe.clone(),
                        args: vec!["/CopyExit".to_string()],
                        cwd: exe.parent().unwrap_or(&root).to_path_buf(),
                        risk: diskpilot_toolbelt::Risk::Low,
                        timeout_secs: 90,
                        usage: String::new(),
                    };
                    let _ = diskpilot_toolbelt::run(&cmd);
                    let txt_path = exe.parent().unwrap_or(&root).join("DiskInfo.txt");
                    if let Ok(txt) = std::fs::read_to_string(txt_path) {
                        cdi = parse_cdi_report(&txt);
                    }
                }
            }
        }
    }
    v["cdi_report"] = serde_json::json!(cdi);
    v["note"] = serde_json::json!("SMART 字段（通电时间/温度/磨损/读写错误）在非管理员权限下可能为 null；CrystalDiskInfo 补充字段同样可能不全。");
    Ok(v)
}

/// 硬盘健康：WMI Get-PhysicalDisk + Get-StorageReliabilityCounter
/// （SMART 通电时间/温度/磨损/读写错误），管理员权限不可用时字段降级为 null；
/// 另附 CrystalDiskInfo（若已装）生成的 DiskInfo.txt 原始字段，供补充参考。
/// 5 分钟 TTL 缓存：切页/多轮查询不再重跑 PowerShell 和 CrystalDiskInfo。
/// refresh=true 时无视 TTL 强制重跑（用户手动刷新）。
#[tauri::command]
pub(crate) async fn hw_disk_health(
    app: AppHandle,
    refresh: Option<bool>,
) -> Result<serde_json::Value, String> {
    let force = refresh.unwrap_or(false);
    tokio::task::spawn_blocking(move || hw_disk_cached(&app, force))
        .await
        .map_err(|e| e.to_string())?
}

/// 解析 CrystalDiskInfo /CopyExit 产出的 DiskInfo.txt，按磁盘抽取
/// 健康状态 / 通电时间 / 温度 / 磨损度。标签中英都认，认不出的字段留 null。
fn parse_cdi_report(txt: &str) -> Vec<serde_json::Value> {
    let lines: Vec<&str> = txt.lines().collect();
    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut cur: Option<serde_json::Map<String, serde_json::Value>> = None;
    let mut i = 0usize;
    while i < lines.len() {
        let t = lines[i].trim();
        let is_sep = t.len() >= 10 && t.chars().all(|c| c == '-' || c == '=');
        if is_sep {
            let nxt = lines.get(i + 1).map(|s| s.trim()).unwrap_or("");
            let looks_name = !nxt.is_empty()
                && !nxt.contains(':')
                && !nxt.contains('：')
                && !(nxt.len() >= 10 && nxt.chars().all(|c| c == '-' || c == '='));
            if looks_name {
                if let Some(m) = cur.take() {
                    out.push(serde_json::Value::Object(m));
                }
                let mut m = serde_json::Map::new();
                m.insert(
                    "device".to_string(),
                    serde_json::Value::String(nxt.to_string()),
                );
                cur = Some(m);
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(pos) = t.find(':').or_else(|| t.find('：')) {
            let key = t[..pos].trim().to_lowercase();
            let val = t[pos + 1..].trim().to_string();
            if let Some(m) = cur.as_mut() {
                let target = [
                    "健康状态",
                    "health status",
                    "通电时间",
                    "power on hours",
                    "温度",
                    "temperature",
                    "磨损",
                    "wear leveling count",
                    "剩余寿命",
                    "ssd 剩余寿命",
                ];
                if target.iter().any(|t| t == &key) {
                    m.insert(key, serde_json::Value::String(val.replace(',', "")));
                }
            }
        }
        i += 1;
    }
    if let Some(m) = cur.take() {
        out.push(serde_json::Value::Object(m));
    }
    out
}

/// 温度采样 TTL 缓存：传感器读数秒级内不会突变，10 秒内直接复用，
/// 避免压测循环每 2 秒 spawn 一个 PowerShell（长测试最多 ~900 次进程风暴）。
/// None（传感器不可读）也缓存：不可读的机器不会每轮都白 spawn 一遍。
const THERMAL_TTL: Duration = Duration::from_secs(10);
static THERMAL_CACHE: OnceLock<Mutex<Option<(std::time::Instant, Option<f32>)>>> = OnceLock::new();
fn thermal_cache_lock() -> &'static Mutex<Option<(std::time::Instant, Option<f32>)>> {
    THERMAL_CACHE.get_or_init(|| Mutex::new(None))
}

/// 采样当前温度（带 TTL 缓存），读不到返回 None。
fn sample_thermal_c() -> Option<f32> {
    let now = std::time::Instant::now();
    if let Ok(guard) = thermal_cache_lock().lock() {
        if let Some((at, v)) = guard.as_ref() {
            if now.duration_since(*at) < THERMAL_TTL {
                return *v;
            }
        }
    }
    let v = read_thermal_max_c();
    if let Ok(mut guard) = thermal_cache_lock().lock() {
        *guard = Some((std::time::Instant::now(), v));
    }
    v
}

/// 读本机 CPU/GPU 温度传感器的最大值（0.1K 单位），失败返回 None。
/// 真正的 PowerShell spawn 点；调用方应走 sample_thermal_c() 走缓存。
fn read_thermal_max_c() -> Option<f32> {
    let script = "(Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue | Measure-Object -Property CurrentTemperature -Maximum).Maximum";
    let s = ps_capture(script, Duration::from_secs(8)).ok()?;
    let v: f64 = s.trim().parse().ok()?;
    let c = v / 10.0 - 273.15;
    // 热区未就绪报标记值 2731（=27.31℃，恰好落入 0~120 区间）——必须单独排除，
    // 否则把「未就绪假值」当成真实温度显示（电脑温度=室温 的假象根源）。
    if (0.0..=120.0).contains(&c) && !(27.0..=27.6).contains(&c) {
        Some(c as f32)
    } else {
        None
    }
}

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

/// 生成硬件检测报告（markdown / json / html），可选保存到用户指定路径。
/// 只读：不修改任何硬件设置。保存路径白名单 .md/.html/.json/.txt，
/// 拒绝覆盖已有文件、拒绝写入系统目录。
#[tauri::command]
pub(crate) async fn hw_report(
    app: AppHandle,
    format: Option<String>,
    save_path: Option<String>,
) -> Result<HwReportOut, String> {
    tokio::task::spawn_blocking(move || {
        // 复用 TTL 缓存：5 分钟内再次导出不重跑 30-60s PowerShell，也不重复
        // 启动 CrystalDiskInfo；冷启动时才各自采集一次并落缓存。
        let info = hw_info_cached(false)?;
        // 磁盘健康读取失败时如实带进报告正文（错误原因），而不是静默当 None。
        let (health, health_note) = match hw_disk_cached(&app, false) {
            Ok(v) => (Some(v), String::new()),
            Err(e) => (None, format!("（硬盘健康读取失败：{e}）")),
        };

        let fmt = format.unwrap_or_else(|| "markdown".to_string());
        let (content, _ext) = match fmt.as_str() {
            "json" => (
                serde_json::to_string_pretty(
                    &serde_json::json!({ "info": info, "health": health, "note": health_note }),
                )
                .unwrap_or_default(),
                "json",
            ),
            "html" => {
                let md = build_hw_markdown(&info, health.as_ref(), &health_note);
                (html_wrap(&md), "html")
            }
            _ => (
                build_hw_markdown(&info, health.as_ref(), &health_note),
                "md",
            ),
        };

        // 报告生成成功后自动归档一份快照（增强功能：失败不影响报告返回）。
        // 归档走独立模块，绝不重跑采集、绝不阻塞主流程。
        crate::hw_history::archive_snapshot(&app, &info, &health, &health_note);

        let mut saved: Option<String> = None;
        let note;
        if let Some(sp) = save_path.filter(|s| !s.trim().is_empty()) {
            let p = std::path::PathBuf::from(sp.trim());
            if !p.is_absolute() {
                return Err("保存路径必须是绝对路径".into());
            }
            let ext_ok = matches!(
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .as_deref(),
                Some("md") | Some("markdown") | Some("html") | Some("json") | Some("txt"),
            );
            if !ext_ok {
                return Err("报告只能保存为 .md / .html / .json / .txt".into());
            }
            let lower = p.to_string_lossy().to_lowercase();
            let sys_root = std::env::var("SystemRoot")
                .unwrap_or_else(|_| "C:\\Windows".to_string())
                .to_lowercase();
            if lower.starts_with(&sys_root)
                || lower.contains("system32")
                || lower.contains("windows\\system")
            {
                return Err("拒绝写入系统目录".into());
            }
            let parent = p.parent().ok_or_else(|| "保存路径没有父目录".to_string())?;
            if !parent.is_dir() {
                return Err(format!("目录不存在：{}", parent.display()));
            }
            if p.exists() {
                return Err(format!(
                    "文件已存在：{} —— 报告不会覆盖已有文件，请换个文件名。",
                    p.display()
                ));
            }
            std::fs::write(&p, &content).map_err(|e| format!("写入失败：{e}"))?;
            saved = Some(p.display().to_string());
            note = format!("报告已保存到 {}", p.display());
        } else {
            note = "未指定保存路径，仅返回内容预览。".to_string();
        }

        Ok(HwReportOut {
            format: fmt,
            markdown: content,
            saved_path: saved,
            note,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize)]
pub(crate) struct HwReportOut {
    format: String,
    markdown: String,
    saved_path: Option<String>,
    note: String,
}

/// 调整电源计划（L2 授权操作）：每次仍需 user_confirmed=true。
/// 只支持 balanced / high_performance / power_saver 三个已知方案，
/// 其余按 GUID 原样透传。
#[tauri::command]
pub(crate) async fn power_plan(
    set_scheme: Option<String>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if let Some(scheme) = set_scheme.as_deref() {
        if confirmed != Some(true) {
            return Err(
                "调整电源计划需要用户明确确认（confirmed=true）。请先在确认面板中让用户确认。"
                    .into(),
            );
        }
        let scheme_lower = scheme.to_ascii_lowercase();
        // GUID 透传只放宽给「标准 8-4-4-4-12 十六进制格式」：
        // 长度 36 + 连字符位置 + 全十六进制，杜绝把任意字符串塞进 powercfg。
        let is_std_guid = |g: &str| {
            if g.len() != 36 {
                return false;
            }
            let bytes = g.as_bytes();
            for (i, &b) in bytes.iter().enumerate() {
                if [8, 13, 18, 23].contains(&i) {
                    if b != b'-' {
                        return false;
                    }
                } else if !(b.is_ascii_hexdigit()) {
                    return false;
                }
            }
            true
        };
        let guid = match scheme_lower.as_str() {
            "balanced" | "平衡" => "381b4222-f694-41f0-9685-ff5bb260df2e",
            "high_performance" | "高性能" => "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c",
            "power_saver" | "节能" => "a1841308-3541-4fab-bc81-f71556f20b4a",
            g if is_std_guid(g) => g,
            _ => {
                return Err(format!(
                    "未知的电源计划：{scheme}。可用 balanced / high_performance / power_saver"
                ))
            }
        };
        ps_capture(
            &format!("powercfg /setactive {guid}"),
            Duration::from_secs(15),
        )
        .map_err(|e| e.to_string())?;
    }
    let list = ps_capture("powercfg /list", Duration::from_secs(15)).map_err(|e| e.to_string())?;
    let mut schemes: Vec<String> = Vec::new();
    let mut active: Option<String> = None;
    for line in list.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with('*') {
            active = Some(t.trim_start_matches('*').trim().to_string());
        }
        if t.contains("GUID") {
            schemes.push(t.to_string());
        }
    }
    Ok(serde_json::json!({ "active": active, "schemes": schemes }))
}

// ── 报告格式化（纯函数，便于测试）─────────────────────────────────────

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn html_wrap(md: &str) -> String {
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>DiskPilot 硬件检测报告</title>\n<style>body{{font:14px/1.6 system-ui,-apple-system,Segoe UI,sans-serif;max-width:860px;margin:40px auto;padding:0 16px;color:#222}}h1,h2,h3{{border-bottom:1px solid #ddd;padding-bottom:4px}}code{{background:#f4f4f4;padding:1px 4px;border-radius:3px}}pre{{background:#f6f8fa;padding:12px;border-radius:6px;overflow:auto}}table{{border-collapse:collapse;margin:8px 0}}td,th{{border:1px solid #ccc;padding:4px 8px}}</style>\n<pre>{}</pre>",
        html_escape(md)
    )
}

fn fmt_gb(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.*} GB", 1, b / 1024.0 / 1024.0 / 1024.0)
    } else if b >= 1024.0 * 1024.0 {
        format!("{:.*} MB", 1, b / 1024.0 / 1024.0)
    } else if b >= 1024.0 {
        format!("{:.*} KB", 0, b / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn v_str(v: &serde_json::Value, k: &str) -> String {
    match v.get(k) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

fn arr<'a>(v: &'a serde_json::Value, k: &str) -> Vec<&'a serde_json::Value> {
    v.get(k)
        .and_then(|x| x.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

fn build_hw_markdown(
    info: &serde_json::Value,
    health: Option<&serde_json::Value>,
    health_note: &str,
) -> String {
    let mut out = String::new();
    out.push_str("# DiskPilot 硬件检测报告\n\n");
    out.push_str(&format!(
        "- 生成时间：{}\n\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    ));

    let os = arr(info, "os")
        .first()
        .copied()
        .unwrap_or(&serde_json::Value::Null);
    let system = arr(info, "system")
        .first()
        .copied()
        .unwrap_or(&serde_json::Value::Null);
    out.push_str("## 系统\n\n");
    out.push_str(&format!("- 系统：{}\n", v_str(os, "Caption")));
    out.push_str(&format!(
        "- 版本：{} / Build {}\n",
        v_str(os, "Version"),
        v_str(os, "BuildNumber")
    ));
    out.push_str(&format!("- 制造商：{}\n", v_str(system, "Manufacturer")));
    out.push_str(&format!("- 机型：{}\n\n", v_str(system, "Model")));

    let cpus = arr(info, "cpu");
    if let Some(c) = cpus.first() {
        out.push_str("## CPU\n\n");
        out.push_str(&format!("- 型号：{}\n", v_str(c, "Name")));
        out.push_str(&format!(
            "- 核心/线程：{} / {}\n",
            v_str(c, "NumberOfCores"),
            v_str(c, "NumberOfLogicalProcessors")
        ));
        let maxmhz: i64 = v_str(c, "MaxClockSpeed").parse().unwrap_or(0);
        let curmhz: i64 = v_str(c, "CurrentClockSpeed").parse().unwrap_or(0);
        out.push_str(&format!(
            "- 频率：标称 {} MHz，当前 {} MHz\n",
            maxmhz, curmhz
        ));
        out.push_str(&format!(
            "- 缓存：L2 {} KB / L3 {} KB\n",
            v_str(c, "L2CacheSize"),
            v_str(c, "L3CacheSize")
        ));
        out.push_str(&format!("- 制造商：{}\n", v_str(c, "Manufacturer")));
        let virt = v_str(c, "VirtualizationFirmwareEnabled");
        out.push_str(&format!(
            "- 虚拟化：{}\n\n",
            if virt == "True" {
                "已开启"
            } else if virt == "False" {
                "未开启"
            } else {
                "未知"
            }
        ));
    }

    let gpus = arr(info, "gpu");
    if !gpus.is_empty() {
        out.push_str("## 显卡\n\n");
        for g in gpus {
            let vram: i64 = v_str(g, "AdapterRAM").parse().unwrap_or(0);
            let vram_str = if vram > 0 {
                if vram >= 4_294_967_296 || vram <= 4_293_918_720 {
                    "≥4 GB（WMI 32 位溢出，实际可能更大）".to_string()
                } else {
                    fmt_gb(vram as u64)
                }
            } else {
                "未知".to_string()
            };
            out.push_str(&format!("- {} · 显存 {}\n", v_str(g, "Name"), vram_str));
            out.push_str(&format!(
                "  - 驱动 {} · {} · {}x{}\n",
                v_str(g, "DriverVersion"),
                v_str(g, "DriverDate"),
                v_str(g, "CurrentHorizontalResolution"),
                v_str(g, "CurrentVerticalResolution")
            ));
            out.push_str(&format!(
                "  - 刷新率 {} Hz\n",
                v_str(g, "CurrentRefreshRate")
            ));
        }
        out.push('\n');
    }

    let mems = arr(info, "memory");
    let total: i64 = mems
        .iter()
        .filter_map(|m| v_str(m, "Capacity").parse::<i64>().ok())
        .sum();
    if total > 0 || !mems.is_empty() {
        out.push_str("## 内存\n\n");
        out.push_str(&format!("- 总容量：{}\n", fmt_gb(total as u64)));
        let mut speeds: Vec<i64> = Vec::new();
        for m in &mems {
            let cap = v_str(m, "Capacity");
            let speed = v_str(m, "Speed");
            speeds.push(speed.parse().unwrap_or(0));
            out.push_str(&format!(
                "- {} {} · {} · {} · {}\n",
                v_str(m, "DeviceLocator"),
                fmt_gb(cap.parse::<i64>().unwrap_or(0) as u64),
                speed,
                v_str(m, "Manufacturer"),
                v_str(m, "PartNumber")
            ));
        }
        if let Some(&s) = speeds.iter().max() {
            let confs: Vec<i64> = mems
                .iter()
                .filter_map(|m| v_str(m, "ConfiguredClockSpeed").parse().ok())
                .collect();
            let max_conf = confs.iter().max().copied().unwrap_or(0);
            if max_conf > 0 {
                out.push_str(&format!(
                    "- 频率：标称 {} MHz，当前运行 {} MHz{}\n",
                    s,
                    max_conf,
                    if max_conf < s {
                        "（XMP/EXPO 可能未生效）"
                    } else {
                        "（XMP/EXPO 已生效）"
                    }
                ));
            }
        }
        out.push('\n');
    }

    let board = arr(info, "board")
        .first()
        .copied()
        .unwrap_or(&serde_json::Value::Null);
    let bios = arr(info, "bios")
        .first()
        .copied()
        .unwrap_or(&serde_json::Value::Null);
    out.push_str("## 主板 / BIOS\n\n");
    out.push_str(&format!(
        "- 主板：{} {}\n",
        v_str(board, "Manufacturer"),
        v_str(board, "Product")
    ));
    out.push_str(&format!(
        "- BIOS：{} · {}\n\n",
        v_str(bios, "SMBIOSBIOSVersion"),
        v_str(bios, "ReleaseDate")
    ));

    let disks = arr(info, "disk");
    if !disks.is_empty() {
        out.push_str("## 磁盘\n\n");
        for d in disks {
            let size: i64 = v_str(d, "Size").parse().unwrap_or(0);
            out.push_str(&format!(
                "- {} · {} · {} · {}\n",
                v_str(d, "Model"),
                fmt_gb(size as u64),
                v_str(d, "InterfaceType"),
                v_str(d, "Status")
            ));
        }
        out.push('\n');
    }

    // 硬盘健康小节：数据读得到时列明细；读不到但带失败原因时也要可见，
    // 不能静默消失（health_note 非空时单独出小节）。
    if health.is_some() || !health_note.is_empty() {
        let smart = health
            .and_then(|h| h.get("reliability"))
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        out.push_str(&format!(
            "## 硬盘健康（SMART 可用：{smart}）{health_note}\n\n"
        ));
    }
    if let Some(h) = health {
        if let Some(disks_h) = h.get("disks").and_then(|x| x.as_array()) {
            for d in disks_h {
                out.push_str(&format!(
                    "- {} · {} · {}\n",
                    v_str(d, "friendly"),
                    v_str(d, "media"),
                    v_str(d, "bus")
                ));
                out.push_str(&format!(
                    "  - 健康：{} · 状态：{}\n",
                    v_str(d, "health"),
                    v_str(d, "operational")
                ));
                if let Some(ph) = v_str(d, "power_on_hours")
                    .parse::<i64>()
                    .ok()
                    .filter(|v| *v > 0)
                {
                    out.push_str(&format!("  - 通电时间：{} 小时\n", ph));
                }
                if let Some(tc) = v_str(d, "temperature_c")
                    .parse::<f64>()
                    .ok()
                    .filter(|v| *v > 0.0)
                {
                    let c = if tc > 60.0 {
                        (tc - 32.0) * 5.0 / 9.0
                    } else {
                        tc
                    };
                    out.push_str(&format!("  - 温度：{:.*} ℃\n", 0, c));
                }
                if let Some(w) = v_str(d, "wear_pct")
                    .parse::<f64>()
                    .ok()
                    .filter(|v| *v >= 0.0)
                {
                    let life = 100.0 - w;
                    out.push_str(&format!(
                        "  - SSD 寿命剩余：{:.*}%（磨损度 {:.*}%）\n",
                        0, life, 0, w
                    ));
                }
                let re = v_str(d, "read_errors");
                if !re.is_empty() {
                    out.push_str(&format!(
                        "  - 读错误：{} · 写错误：{}\n",
                        re,
                        v_str(d, "write_errors")
                    ));
                }
            }
        }
        if let Some(cdis) = h.get("cdi_report").and_then(|x| x.as_array()) {
            if !cdis.is_empty() {
                out.push_str("\n### CrystalDiskInfo 补充字段（原始标签）\n\n");
                for c in cdis {
                    out.push_str(&format!("- {}\n", v_str(c, "device")));
                    if let Some(obj) = c.as_object() {
                        for (k, _val) in obj {
                            if k == "device" {
                                continue;
                            }
                            out.push_str(&format!("  - {k}：{}\n", v_str(c, k)));
                        }
                    }
                }
            }
        }
    }

    let thermal = arr(info, "thermal");
    if !thermal.is_empty() {
        out.push_str("## 温度传感器\n\n");
        for t in thermal {
            let raw: f64 = v_str(t, "CurrentTemperature").parse().unwrap_or(0.0);
            let c = raw / 10.0 - 273.15;
            if (0.0..=120.0).contains(&c) {
                out.push_str(&format!("- {}：{:.0} ℃\n", v_str(t, "InstanceName"), c));
            }
        }
        out.push('\n');
    }

    out.push_str("---\n*数据来源：WMI CIM 查询 + 图吧工具箱（CrystalDiskInfo /CopyExit）。读不到的字段留空，不编造。*\n");
    out
}

/// 风扇转速（WMI 支持有限，读不到为 null）。
const FAN_PS: &str = r#"$ErrorActionPreference='SilentlyContinue'
$out=[ordered]@{}
$fans=@()
Get-CimInstance -Namespace root/cimv2 -ClassName Win32_Fan -ErrorAction SilentlyContinue | ForEach-Object {
  $fans += [ordered]@{ name=[string]$_.Name; desired_rpm=$_.DesiredSpeed; actual_rpm=$null; status=[string]$_.Status; active=($_.ActiveCooling -eq $true) }
}
try {
  Get-CimInstance -Namespace root/wmi -ClassName mssctree_faninformation -ErrorAction SilentlyContinue | ForEach-Object {
    $fans += [ordered]@{ name=[string]$_.InstanceName; desired_rpm=$_.DesiredValue; actual_rpm=$null; status=''; active=$true }
  }
} catch {}
$out.fans=$fans
$out.thermal=@(Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue | Select-Object InstanceName,CurrentTemperature)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 从温度/负载推导建议档位（纯规则，可单测）：
/// info = hw_info 缓存结果（cpu[].LoadPercentage + thermal[].CurrentTemperature）。
/// 返回 { level, level_label, reason, curve: [{load_pct, fan_pct}], sensors, note }。
/// 这里是「建议」层：只读传感器推导档位；落地调速走 fan_control（L2 权限，多通道探测）。
fn build_fan_advice(info: &serde_json::Value) -> serde_json::Value {
    // 最高温度传感器（MSAcpi 单位：十分之一开尔文）。
    let mut temps: Vec<f32> = Vec::new();
    if let Some(arr) = info.get("thermal").and_then(|v| v.as_array()) {
        for t in arr {
            if let Some(raw) = t.get("CurrentTemperature").and_then(|v| v.as_f64()) {
                let c = (raw / 10.0 - 273.15) as f32;
                if (0.0..=120.0).contains(&c) {
                    temps.push(c);
                }
            }
        }
    }
    let temp_max = temps.iter().cloned().fold(f32::MIN, f32::max);
    let temp_max = if temp_max == f32::MIN {
        None
    } else {
        Some(temp_max)
    };

    // CPU 负载（LoadPercentage 可能缺失）。
    let mut load: Option<f64> = None;
    if let Some(arr) = info.get("cpu").and_then(|v| v.as_array()) {
        if let Some(first) = arr.first() {
            load = first.get("LoadPercentage").and_then(|v| v.as_f64());
        }
    }

    // 档位规则：温度优先（散热是结果），负载其次（负载是原因）。
    let (level, level_label, reason) = match (temp_max, load) {
        (Some(t), _) if t >= 90.0 => (
            "urgent",
            "过热风险",
            format!("温度已达 {t:.0}℃，接近降频阈值。建议立即清灰/改善进风，并把风扇拉满跑几分钟压温。"),
        ),
        (Some(t), _) if t >= 80.0 => (
            "high",
            "偏高",
            format!("温度 {t:.0}℃，持续高负载下偏热。建议把曲线中段（60-80% 负载）抬高 10-15%。"),
        ),
        (Some(t), Some(l)) if t >= 70.0 && l >= 60.0 => (
            "balanced_plus",
            "略偏高",
            format!("负载 {l:.0}% 时温度 {t:.0}℃，正常偏高。建议中段风扇 +10%，或改用「高性能」电源计划让风扇更早介入。"),
        ),
        (Some(t), Some(l)) if t < 55.0 && l < 25.0 => (
            "quiet_ok",
            "安静档可行",
            format!("负载 {l:.0}%、温度 {t:.0}℃，散热余量大。低负载段（<30%）可把风扇压到 30-40% 换安静。"),
        ),
        (Some(t), Some(l)) => (
            "balanced",
            "均衡",
            format!("负载 {l:.0}%、温度 {t:.0}℃，散热正常。保持默认曲线即可。"),
        ),
        (Some(t), None) => (
            "temp_only",
            "温度可读",
            format!("温度 {t:.0}℃，CPU 负载传感器读不到。按温度给建议：{}{}。",
                if t >= 80.0 { "偏高，建议清灰/改善散热" } else { "散热正常" },
                if t >= 90.0 { "，并立即检查散热" } else { "" }
            ),
        ),
        (None, Some(l)) => (
            "unknown_temp",
            "温度不可读",
            format!("温度传感器读不到（ACPI 热区未暴露），只能按负载 {l:.0}% 给通用建议：低负载安静、高负载均衡。"),
        ),
        (None, None) => (
            "unknown",
            "数据不足",
            "温度与负载传感器都读不到，无法给出针对性建议。常见原因：主板未暴露 ACPI 热区/需要管理员权限。".to_string(),
        ),
    };

    // 建议曲线锚点（负载% → 风扇%），按档位微调默认 OEM 曲线形态。
    let curve: Vec<serde_json::Value> = match level {
        "urgent" | "high" => [(20u32, 55u32), (40, 70), (60, 85), (80, 100), (100, 100)],
        "balanced_plus" => [(20, 40), (40, 55), (60, 70), (80, 85), (100, 100)],
        "quiet_ok" => [(20, 30), (40, 40), (60, 60), (80, 75), (100, 90)],
        _ => [(20, 35), (40, 50), (60, 65), (80, 80), (100, 100)],
    }
    .iter()
    .map(|(l, f)| serde_json::json!({ "load_pct": l, "fan_pct": f }))
    .collect();

    serde_json::json!({
        "level": level,
        "level_label": level_label,
        "reason": reason,
        "temp_max_c": temp_max,
        "cpu_load_pct": load,
        "curve": curve,
        "note": "这是基于温度/负载的建议曲线。落地调速由「风扇控制」（fan.control 权限，多通道探测：内置 WMI + 厂商/第三方 CLI 通道）执行；本建议的锚点可直接作为控制面板与自定义曲线编辑的起点。"
    })
}

/// 风扇曲线建议（L2）：读温度传感器 + CPU 负载 + 风扇信息（WMI 能读多少算多少），
/// 生成「当前散热状态 + 建议曲线锚点」。只出建议；落地调速走 fan_control（L2，多通道探测）。
#[tauri::command]
pub(crate) async fn fan_curve_advice(refresh: Option<bool>) -> Result<serde_json::Value, String> {
    let force = refresh.unwrap_or(false);
    tokio::task::spawn_blocking(move || {
        let info = hw_info_cached(force)?;
        let mut advice = build_fan_advice(&info);
        // 风扇列表单独查（hw_info 不含）；读失败不致命——建议主体来自温度/负载。
        let fans_json = ps_capture(FAN_PS, Duration::from_secs(20))
            .ok()
            .and_then(|raw| {
                let raw = raw.trim().trim_start_matches('\u{feff}');
                serde_json::from_str::<serde_json::Value>(raw).ok()
            });
        if let Some(fj) = fans_json {
            advice["fans"] = fj.get("fans").cloned().unwrap_or(serde_json::json!([]));
        } else {
            advice["fans"] = serde_json::json!([]);
        }
        Ok(advice)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── 风扇控制（多通道探测 + 温度熔断回退）─────────────────────────────────

/// 温度熔断阈值：应用自定义曲线后，后台守护采样到该温度即自动回退原转速。
/// 对齐 hw_run_test 的 temp_limit（默认 90℃），下限保护到 70℃。
const FAN_TEMP_BREAKER_C: f32 = 90.0;

/// 风扇应用后的温度守护采样周期。
const FAN_GUARD_POLL: Duration = Duration::from_secs(5);

/// 原转速快照 + 守护运行标志（进程级静态，与 THERMAL_CACHE 同款）。
static FAN_SNAPSHOT: OnceLock<Mutex<Option<u32>>> = OnceLock::new();
static FAN_GUARD_RUNNING: OnceLock<AtomicBool> = OnceLock::new();
fn fan_snapshot_lock() -> &'static Mutex<Option<u32>> {
    FAN_SNAPSHOT.get_or_init(|| Mutex::new(None))
}
fn fan_guard_running() -> bool {
    FAN_GUARD_RUNNING
        .get_or_init(|| AtomicBool::new(false))
        .load(Ordering::Relaxed)
}
fn fan_guard_set(v: bool) {
    FAN_GUARD_RUNNING
        .get_or_init(|| AtomicBool::new(false))
        .store(v, Ordering::Relaxed);
}

/// 内置 WMI 通道探测：Win32_Fan / mssctree_faninformation 只读属性。
/// 标准 WMI 类没有通用的「写风扇」接口——如实报告不可写，不伪装成可调。
fn wmi_fan_probe() -> serde_json::Value {
    let mut out = serde_json::json!({
        "channel": "wmi",
        "writable": false,
        "mode": "readonly",
        "reason": "WMI 标准类（Win32_Fan / mssctree_faninformation）只暴露只读属性（DesiredSpeed / DesiredValue），没有通用的写入接口。",
        "fans": []
    });
    if let Some(raw) = ps_capture(FAN_PS, Duration::from_secs(20)).ok() {
        let raw = raw.trim().trim_start_matches('\u{feff}');
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(fans) = v.get("fans").and_then(|x| x.as_array()) {
                out["fans"] = serde_json::json!(fans);
            }
        }
    }
    out
}

/// 厂商/第三方 CLI 外挂通道：在本机 PATH 与常见安装目录找可写调速 CLI。
/// 探测顺序：
///   1. fancmd（DiskPilot 自带 ITE SuperIO 写控桥，tools/fancmd —— 真实可写，无需第三方软件）
///   2. nbfc（NoteBook FanControl：`nbfc set -s <0-100>` 写转速）
/// 找不到就不编造，如实降级。
fn cli_fan_probe() -> serde_json::Value {
    // (显示名, exe, 子命令模板, 是否按空格拆分参数, 是否有独立 reset 子命令)
    let candidates: &[(&str, &str, &str, bool, bool)] = &[
        (
            "DiskPilot fancmd（ITE SuperIO 写控桥）",
            "fancmd.exe",
            "write 0 {speed}",
            true,
            true,
        ),
        (
            "NoteBook FanControl",
            "nbfc.exe",
            "set -s {speed}",
            false,
            false,
        ),
    ];
    for (name, exe, subcmd_tpl, arg_split, has_reset) in candidates {
        if let Some(exe_path) = find_cli_in_paths(exe) {
            // fancmd 额外跑 `diag json` 拿结构化诊断（缺驱动/芯片不可识别等，
            // 供内置 AI 自修复参考）。只读、超时兜底，失败不致命。
            let mut diagnostics = serde_json::json!({});
            let mut writable = true;
            if *exe == "fancmd.exe" {
                match run_cli_capture(&exe_path, &["diag", "json"], Duration::from_secs(10)) {
                    Ok(raw) => {
                        let d = parse_fancmd_diag(&raw);
                        diagnostics = d.clone();
                        // 驱动没加载 / 芯片没识别 / diag 输出没法解析（旧版 fancmd 不认
                        // `diag json`、或非管理员下驱动不可用输出人类可读文本）→
                        // fancmd 存在但写通道不可确证，如实降级，绝不拿假可写下结论。
                        if d.get("parsed").and_then(|x| x.as_bool()) == Some(false) {
                            writable = false;
                        }
                        if let Some(drv) = d.get("driver") {
                            if drv.get("loaded").and_then(|x| x.as_bool()) == Some(false) {
                                writable = false;
                            }
                        }
                        if let Some(sio) = d.get("sio") {
                            if sio.get("found").and_then(|x| x.as_bool()) == Some(false) {
                                writable = false;
                            }
                        }
                    }
                    // diag 命令本身执行失败（如旧版 exe 直接 usage 退出）→ 不可写。
                    Err(_) => writable = false,
                }
            }
            return serde_json::json!({
                "channel": "cli",
                "writable": writable,
                "mode": if writable { "cli" } else { "degraded" },
                "cli": {
                    "name": name,
                    "exe": exe,
                    "exe_path": exe_path,
                    "subcmd_tpl": subcmd_tpl,
                    "arg_split": arg_split,
                    "has_reset": has_reset,
                },
                "diagnostics": diagnostics,
                "reason": if writable {
                    format!("检测到可写调速 CLI：{name}（{exe_path}）。")
                } else {
                    format!(
                        "检测到 {name}（{exe_path}），但其自诊断显示当前不可写（缺驱动或 SuperIO 芯片不可识别）。驱动装在非标准路径时请让内置 AI 自动安装/加载端口驱动后重试。"
                    )
                },
                "fans": diagnostics.get("fans").cloned().unwrap_or(serde_json::json!([]))
            });
        }
    }
    serde_json::json!({
        "channel": "cli",
        "writable": false,
        "mode": "readonly",
        "reason": "未检测到可写调速 CLI（fancmd / NoteBook FanControl 均未找到）。如需落地调速，请先安装可写通道，或让内置 AI 自动安装 fancmd + inpoutx64 后重试。",
        "fans": []
    })
}

/// 在 PATH 和常见安装目录里找 exe 的完整路径（找到第一个即停）。
/// 额外探测两个本地路径：
///   - 开发环境：仓库内 tools/fancmd/bin/fancmd.exe（从当前 exe 相对上溯）
///   - 打包环境：resources/fancmd/fancmd.exe（与主程序同目录）
fn find_cli_in_paths(exe: &str) -> Option<String> {
    let is_file = |p: &std::path::Path| p.is_file();
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let full = dir.join(exe);
            if is_file(&full) {
                return Some(full.to_string_lossy().into_owned());
            }
        }
    }
    // 常见安装目录兜底（32/64 位 Program Files + LOCALAPPDATA）。
    let bases = [
        std::env::var("ProgramFiles").unwrap_or_default(),
        std::env::var("ProgramFiles(x86)").unwrap_or_default(),
        std::env::var("LOCALAPPDATA").unwrap_or_default(),
    ];
    for base in bases {
        if base.is_empty() {
            continue;
        }
        let candidates = [
            std::path::Path::new(&base)
                .join("NoteBook FanControl")
                .join(exe),
            std::path::Path::new(&base).join("nbfc").join(exe),
        ];
        for c in candidates {
            if is_file(&c) {
                return Some(c.to_string_lossy().into_owned());
            }
        }
    }
    // 开发环境：仓库 tools/fancmd/bin（从当前 exe 上溯到仓库根）。
    if let Ok(cur) = std::env::current_exe() {
        let mut probe = cur.clone();
        for _ in 0..6 {
            if !probe.pop() {
                break;
            }
            let fancmd = probe.join("tools").join("fancmd").join("bin").join(exe);
            if is_file(&fancmd) {
                return Some(fancmd.to_string_lossy().into_owned());
            }
            if probe.join("tools").join("fancmd").is_dir() {
                break;
            }
        }
    }
    // 打包环境：与主程序同目录的 resources/fancmd。
    if let Ok(cur) = std::env::current_exe() {
        if let Some(dir) = cur.parent() {
            let res = dir.join("resources").join("fancmd").join(exe);
            if is_file(&res) {
                return Some(res.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// 多通道探测汇总：内置 WMI（只读）+ 外挂 CLI（可写时写入）。
/// 返回 { wmi, cli, writable, best_channel, note }。
/// 注意：`cli` 字段**直接**是 CLI 通道内容（name/exe/exe_path/subcmd_tpl/…），
/// 不是 cli_fan_probe 的整块——fan_control / fan_curve_apply 把 `probe["cli"]`
/// 原样传给 cli_set_fan_speed / cli_restore_fan_speed，套一层会导致
/// 「调速 CLI 信息不完整（缺 exe_path / subcmd_tpl）」。
fn fan_probe_all() -> serde_json::Value {
    let wmi = wmi_fan_probe();
    let probe = cli_fan_probe();
    let writable = probe["writable"].as_bool().unwrap_or(false);
    let best_channel = if writable { "cli" } else { "wmi" };
    serde_json::json!({
        "wmi": wmi,
        "cli": if writable {
            // 可写：平铺通道内容（probe.cli 里层）。
            probe["cli"].clone()
        } else {
            // 只读/降级：cli 字段给个空对象，保持键存在；writable=false 已足够区分。
            serde_json::json!({})
        },
        "cli_probe": probe,
        "writable": writable,
        "best_channel": best_channel,
        "note": if writable {
            "检测到可写调速通道（CLI 外挂）。应用曲线会写入该通道。"
        } else {
            "当前无可写调速通道（WMI 标准类只读 + 未检测到调速 CLI）。只能给出建议，不能落地调速。"
        }
    })
}

/// 用 CLI 通道写入转速百分比（0-100）。写失败返回 Err。
/// nbfc 把子命令模板当单个参数；fancmd 需要按空格拆成多个参数（arg_split=true）。
fn cli_set_fan_speed(cli: &serde_json::Value, speed_pct: u32) -> Result<(), String> {
    let speed_pct = speed_pct.clamp(0, 100);
    let exe_path = cli["exe_path"].as_str().unwrap_or("").to_string();
    let tpl = cli["subcmd_tpl"].as_str().unwrap_or("").to_string();
    if exe_path.is_empty() || tpl.is_empty() {
        return Err("调速 CLI 信息不完整（缺 exe_path / subcmd_tpl）。".into());
    }
    let arg_split = cli["arg_split"].as_bool().unwrap_or(false);
    let sub = tpl.replace("{speed}", &speed_pct.to_string());
    let mut cmd = std::process::Command::new(&exe_path);
    if arg_split {
        for part in sub.split_whitespace() {
            cmd.arg(part);
        }
    } else {
        cmd.arg(&sub);
    }
    let result = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();
    match result {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format!(
            "调速 CLI 执行失败（退出码 {:?}）：{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => Err(format!("启动调速 CLI 失败：{e}（{exe_path}）")),
    }
}

/// 恢复主板自动控制（fancmd 走 `write reset <idx>`；nbfc 无 reset 走写 100%）。
fn cli_restore_fan_speed(cli: &serde_json::Value) -> Result<(), String> {
    let exe_path = cli["exe_path"].as_str().unwrap_or("").to_string();
    let tpl = cli["subcmd_tpl"].as_str().unwrap_or("").to_string();
    if exe_path.is_empty() || tpl.is_empty() {
        return Err("调速 CLI 信息不完整（缺 exe_path / subcmd_tpl）。".into());
    }
    if cli["has_reset"].as_bool().unwrap_or(false) {
        // fancmd: 提取 fan idx 然后 `write reset <idx>`。tpl 形如 "write 0 {speed}"。
        let fan_idx = tpl.split_whitespace().nth(1).unwrap_or("0").to_string();
        let result = std::process::Command::new(&exe_path)
            .args(["write", "reset", &fan_idx])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output();
        match result {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => Err(format!(
                "恢复调速 CLI 执行失败（退出码 {:?}）：{}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            )),
            Err(e) => Err(format!("启动恢复调速 CLI 失败：{e}（{exe_path}）")),
        }
    } else {
        // nbfc 没有独立 reset：写 100% 近似主板接管。
        cli_set_fan_speed(cli, 100)
    }
}

/// 从档位/曲线算目标转速百分比（纯函数，可单测）。
fn speed_pct_for(level: Option<&str>, curve: Option<&[serde_json::Value]>) -> Result<u32, String> {
    match level.unwrap_or("balanced") {
        "quiet" => Ok(35),
        "balanced" => Ok(55),
        "balanced_plus" => Ok(70),
        "high" => Ok(85),
        "urgent" => Ok(100),
        "custom" => {
            let anchors =
                curve.ok_or_else(|| "custom 档位需要提供 curve 锚点（非空）。".to_string())?;
            if anchors.is_empty() {
                return Err("custom 档位需要提供 curve 锚点（非空）。".into());
            }
            let mut sum = 0u64;
            let mut n = 0u64;
            for a in anchors {
                if let Some(p) = a["fan_pct"].as_u64() {
                    sum += p;
                    n += 1;
                }
            }
            if n == 0 {
                return Err("custom 档位的 curve 锚点缺少 fan_pct 字段。".into());
            }
            Ok(((sum / n) as u32).clamp(20, 100))
        }
        other => Err(format!(
            "未知档位：{other}。可用 quiet / balanced / balanced_plus / high / urgent / custom"
        )),
    }
}

/// 风扇控制（L2 写操作）：确认 + 权限中心双重校验后，按档位/自定义曲线写入调速通道。
/// 写前把原转速快照存到进程级静态（OnceLock<Mutex>，与 THERMAL_CACHE 同款），
/// 然后 spawn 后台温度守护线程：5s 采样一次，超阈值自动回退原转速并退出。
/// 返回应用结果。前端另有 5s visibility 门控轮询读 fan_status 长期展示状态。
#[tauri::command]
pub(crate) async fn fan_control(
    app: AppHandle,
    state: State<'_, AppState>,
    level: Option<String>,
    curve: Option<Vec<serde_json::Value>>,
    temp_breaker_c: Option<f32>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    // 纵深防御：confirmed + perm_grants 双重校验（对齐 toolbelt_run / plugin_* 模板）。
    if confirmed != Some(true) {
        return Err(
            "fan:confirm: 风扇控制会写入调速通道，必须由用户明确确认（confirmed=true）。请先在确认面板中让用户确认。"
                .into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("fan.control") {
        return Err(
            "fan:denied: 未开启「风扇控制」权限（fan.control）。请先在权限中心开启后重试。".into(),
        );
    }
    let breaker_c = temp_breaker_c
        .unwrap_or(FAN_TEMP_BREAKER_C)
        .clamp(70.0, 105.0);
    let app2 = app.clone();
    tokio::task::spawn_blocking(move || fan_control_blocking(app2, level, curve, breaker_c))
        .await
        .map_err(|e| e.to_string())?
}

fn fan_control_blocking(
    app: AppHandle,
    level: Option<String>,
    curve: Option<Vec<serde_json::Value>>,
    breaker_c: f32,
) -> Result<serde_json::Value, String> {
    let probe = fan_probe_all();
    if !probe["writable"].as_bool().unwrap_or(false) {
        return Ok(serde_json::json!({
            "ok": false,
            "applied": false,
            "probe": probe,
            "note": "当前无可写调速通道，未执行任何写入。这是如实降级：DiskPilot 不会在没有可用通道时伪造控制结果。",
            "recommend": "可安装 NoteBook FanControl（nbfc）获得可写 CLI 通道后重试。"
        }));
    }
    let speed_pct = speed_pct_for(level.as_deref(), curve.as_deref())?;

    // 读取当前转速作为「原转速」快照：Win32_Fan 只有 DesiredSpeed（-1 表自动），
    // 读不到就用 100（主板自动接管的手感）。仅首次应用保存；后续应用不覆盖，
    // 熔断始终回退到最早一次应用前的转速。
    let cli = probe["cli"].clone();
    let current_pct: u32 = probe["wmi"]["fans"]
        .as_array()
        .and_then(|fans| fans.first())
        .and_then(|f| f["desired_rpm"].as_i64())
        .and_then(|v| if v >= 0 { Some(v as u32) } else { None })
        .unwrap_or(100)
        .clamp(0, 100);
    {
        let mut snap = fan_snapshot_lock().lock().unwrap();
        if snap.is_none() {
            *snap = Some(current_pct);
        }
    }
    let original_pct = fan_snapshot_lock()
        .lock()
        .unwrap()
        .unwrap_or(100)
        .clamp(0, 100);

    // 写入。
    cli_set_fan_speed(&cli, speed_pct).map_err(|e| format!("写入调速通道失败：{e}"))?;

    // 后台温度熔断守护：只 spawn 一次（避免多次应用叠出多个守护线程互踩）。
    // 守护线程 5s 采样一次温度；读到 ≥ 阈值就自动回退到原转速，emit 事件，退出。
    if !fan_guard_running() {
        fan_guard_set(true);
        std::thread::spawn(move || loop {
            std::thread::sleep(FAN_GUARD_POLL);
            if let Some(c) = sample_thermal_c() {
                if c >= breaker_c {
                    let reverted = cli_set_fan_speed(&cli, original_pct).is_ok();
                    let note = format!(
                        "温度达到 {c:.0}℃（≥ 熔断阈值 {breaker_c:.0}℃），已自动回退原转速 {}%。",
                        original_pct
                    );
                    let _ = app.emit(
                        "fan-temp-breaker",
                        &serde_json::json!({
                            "temp_c": c,
                            "breaker_c": breaker_c,
                            "reverted": reverted,
                            "original_speed_pct": original_pct,
                            "note": note,
                        }),
                    );
                    fan_guard_set(false);
                    break;
                }
            }
        });
    }

    Ok(serde_json::json!({
        "ok": true,
        "applied": true,
        "channel": probe["best_channel"],
        "speed_pct": speed_pct,
        "original_speed_pct": original_pct,
        "temp_breaker_c": breaker_c,
        "guard_running": true,
        "probe": probe,
        "note": format!("已通过 {} 通道应用转速 {}%；后台温度守护已启动（阈值 {breaker_c:.0}℃，超限自动回退 {}%）。", probe["best_channel"], speed_pct, original_pct)
    }))
}

/// 风扇状态（只读 L0）：通道探测 + 当前温度 + 是否有快照/守护在跑。
/// 供前端面板轮询与 AI 只读查询。
#[tauri::command]
pub(crate) async fn fan_status() -> Result<serde_json::Value, String> {
    let has_snapshot = fan_snapshot_lock().lock().unwrap().is_some();
    let guard_running = fan_guard_running();
    tokio::task::spawn_blocking(move || {
        let probe = fan_probe_all();
        let temp_c = sample_thermal_c();
        Ok(serde_json::json!({
            "probe": probe,
            "temp_c": temp_c,
            "writable": probe["writable"],
            "has_snapshot": has_snapshot,
            "guard_running": guard_running,
            "note": probe["note"],
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 恢复默认（L2 写操作）：确认 + 权限中心双重校验后，把转速写回 100% 让主板
/// 自动接管，并清空原转速快照（后续应用再存新快照）。
#[tauri::command]
pub(crate) async fn fan_curve_apply(
    state: State<'_, AppState>,
    restore_default: Option<bool>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "fan:confirm: 恢复默认风扇转速需要用户明确确认（confirmed=true）。请先在确认面板中让用户确认。"
                .into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("fan.control") {
        return Err(
            "fan:denied: 未开启「风扇控制」权限（fan.control）。请先在权限中心开启后重试。".into(),
        );
    }
    let do_restore = restore_default.unwrap_or(false);
    tokio::task::spawn_blocking(move || {
        let probe = fan_probe_all();
        if !do_restore {
            return Ok(serde_json::json!({
                "ok": true,
                "applied": false,
                "probe": probe,
                "note": "未执行操作（restore_default 未开启）。"
            }));
        }
        if !probe["writable"].as_bool().unwrap_or(false) {
            return Ok(serde_json::json!({
                "ok": false,
                "applied": false,
                "probe": probe,
                "note": "当前无可写调速通道，未执行任何写入。如实降级：没有可用通道时不伪造控制结果。"
            }));
        }
        let cli = probe["cli"].clone();
        cli_restore_fan_speed(&cli).map_err(|e| format!("恢复默认转速失败：{e}"))?;
        // 清空快照：主板已接管，后续应用重新存原转速。
        *fan_snapshot_lock().lock().unwrap() = None;
        fan_guard_set(false);
        Ok(serde_json::json!({
            "ok": true,
            "applied": true,
            "note": "已恢复主板自动控制，主板将接管风扇策略。"
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fmt_gb_handles_binary_units() {
        assert_eq!(fmt_gb(0), "0 B");
        assert_eq!(fmt_gb(1023), "1023 B");
        assert_eq!(fmt_gb(1536), "2 KB"); // 1.5 KB 四舍五入到 0 位小数
        assert_eq!(fmt_gb(1024 * 1024), "1.0 MB");
        assert_eq!(fmt_gb(1024 * 1024 * 1024), "1.0 GB");
        // 恰好 4 GiB 走 GB 分支
        let gb = 4u64 * 1024 * 1024 * 1024;
        assert_eq!(fmt_gb(gb), "4.0 GB");
    }

    #[test]
    fn v_str_reads_string_number_bool_and_missing() {
        let v = json!({ "s": "abc", "n": 42, "b": true });
        assert_eq!(v_str(&v, "s"), "abc");
        assert_eq!(v_str(&v, "n"), "42");
        assert_eq!(v_str(&v, "b"), "true");
        assert_eq!(v_str(&v, "missing"), "");
        assert_eq!(v_str(&serde_json::Value::Null, "x"), "");
    }

    #[test]
    fn arr_extracts_array_or_empty() {
        let v = json!({ "list": [1, 2, 3], "nope": "x" });
        assert_eq!(arr(&v, "list").len(), 3);
        assert_eq!(arr(&v, "nope").len(), 0);
        assert_eq!(arr(&v, "missing").len(), 0);
    }

    #[test]
    fn build_hw_markdown_includes_health_failure_note() {
        let info = json!({
            "os": [{ "Caption": "Windows 11 Pro", "Version": "10.0.22631", "BuildNumber": "22631" }],
            "system": [{ "Manufacturer": "LENOVO", "Model": "Y9000P" }],
            "cpu": [], "gpu": [], "memory": [], "board": [], "bios": [], "disk": [], "thermal": []
        });
        let md = build_hw_markdown(&info, None, "（硬盘健康读取失败：WMI 拒绝访问）");
        assert!(md.contains("Windows 11 Pro"));
        assert!(md.contains("硬盘健康（SMART 可用：false）"));
        assert!(md.contains("WMI 拒绝访问"));
    }

    #[test]
    fn build_hw_markdown_lists_health_details_when_available() {
        let info = json!({
            "os": [], "system": [], "cpu": [], "gpu": [], "memory": [], "board": [],
            "bios": [], "disk": [], "thermal": []
        });
        let health = json!({
            "reliability": true,
            "disks": [{
                "friendly": "NVMe SSD", "media": "SSD", "bus": "NVMe",
                "health": "Healthy", "operational": "OK", "power_on_hours": 1234,
                "temperature_c": 45.0, "wear_pct": 3.0,
                "read_errors": 0, "write_errors": 0
            }]
        });
        let md = build_hw_markdown(&info, Some(&health), "");
        assert!(md.contains("SMART 可用：true"));
        assert!(md.contains("NVMe SSD"));
        assert!(md.contains("通电时间：1234 小时"));
        assert!(md.contains("SSD 寿命剩余：97%"));
        assert!(md.contains("读错误：0 · 写错误：0"));
    }

    // GPU 实时双通道决策（pick_gpu_live）：NVIDIA 优先，AMD ADL 兜底。
    // 抽纯函数规避真 spawn nvidia-smi / PowerShell——这儿 mock 两通道结果锁矩阵。
    #[test]
    fn pick_gpu_live_nvidia_wins_when_both_available() {
        let nv = json!({ "name": "NVIDIA GeForce RTX 5090 D v2", "temperature_c": 38 });
        let adl = json!({ "name": "AMD 显卡（ADL 适配器 1）", "temperature_c": 58 });
        let got =
            pick_gpu_live(Ok(Some(nv.clone())), Ok(Some(adl.clone()))).expect("应取到 NVIDIA");
        assert_eq!(got["name"], "NVIDIA GeForce RTX 5090 D v2");
    }

    #[test]
    fn pick_gpu_live_amd_when_nvidia_absent() {
        // 无 N 卡（nvidia-smi 返回空行 → Ok(None)）→ 退 AMD ADL 兜底
        let adl = json!({ "name": "AMD 显卡（ADL 适配器 1）", "temperature_c": 58 });
        let got = pick_gpu_live(Ok(None), Ok(Some(adl.clone()))).expect("应取到 AMD ADL");
        assert!(got["name"].as_str().unwrap().contains("AMD"));
    }

    #[test]
    fn pick_gpu_live_amd_when_nvidia_spawn_fails() {
        // nvidia-smi 不存在 → Err → AMD 兜底照常生效
        let adl = json!({ "name": "AMD 显卡（ADL 适配器 1）", "temperature_c": 58 });
        let got = pick_gpu_live(
            Err("启动 nvidia-smi 失败：NotFound".into()),
            Ok(Some(adl.clone())),
        )
        .expect("应取到 AMD ADL");
        assert_eq!(got["temperature_c"], 58);
    }

    #[test]
    fn pick_gpu_live_none_when_both_channels_dead() {
        assert!(pick_gpu_live(Ok(None), Ok(None)).is_none());
        assert!(pick_gpu_live(Err("nvidia-smi 失败".into()), Ok(None)).is_none());
        assert!(pick_gpu_live(Ok(None), Err("ADL 不可用".into())).is_none());
        assert!(pick_gpu_live(Err("a".into()), Err("b".into())).is_none());
    }

    #[test]
    fn build_hw_markdown_omits_health_section_when_no_health_and_no_note() {
        let info = json!({
            "os": [], "system": [], "cpu": [], "gpu": [], "memory": [], "board": [],
            "bios": [], "disk": [], "thermal": []
        });
        let md = build_hw_markdown(&info, None, "");
        assert!(!md.contains("硬盘健康"));
    }

    #[test]
    fn build_hw_markdown_formats_thermal_sensor() {
        let info = json!({
            "os": [], "system": [], "cpu": [], "gpu": [], "memory": [], "board": [],
            "bios": [], "disk": [],
            "thermal": [{ "InstanceName": "ACPI\\ThermalZone\\TZ00_0", "CurrentTemperature": 2992 }]
        });
        let md = build_hw_markdown(&info, None, "");
        assert!(md.contains("## 温度传感器"));
        assert!(md.contains("26 ℃")); // 2992/10 - 273.15 = 26.05 → "26 ℃"
    }

    #[test]
    fn fan_advice_urgent_when_temp_high() {
        let info = json!({
            "cpu": [{ "LoadPercentage": 40 }],
            "thermal": [{ "InstanceName": "ACPI\\ThermalZone\\TZ00_0", "CurrentTemperature": 3652 }]
        });
        let advice = build_fan_advice(&info);
        assert_eq!(advice["level"], "urgent"); // 3652/10-273.15 ≈ 92℃
        let t = advice["temp_max_c"].as_f64().unwrap();
        assert!((t - 92.05).abs() < 0.01);
        // 紧急档曲线拉满
        let curve = advice["curve"].as_array().unwrap();
        assert_eq!(curve.last().unwrap()["fan_pct"], 100);
    }

    #[test]
    fn fan_advice_quiet_when_cool_and_idle() {
        let info = json!({
            "cpu": [{ "LoadPercentage": 5 }],
            "thermal": [{ "InstanceName": "ACPI\\ThermalZone\\TZ00_0", "CurrentTemperature": 2982 }]
        });
        let advice = build_fan_advice(&info);
        assert_eq!(advice["level"], "quiet_ok"); // 2982/10-273.15 ≈ 25℃
        assert!(advice["reason"].as_str().unwrap().contains("安静"));
    }

    #[test]
    fn fan_advice_data_missing_degrades_gracefully() {
        let advice = build_fan_advice(&json!({ "cpu": [], "thermal": [] }));
        assert_eq!(advice["level"], "unknown");
        assert!(advice["temp_max_c"].is_null());
        assert!(advice["cpu_load_pct"].is_null());
    }

    #[test]
    fn html_wrap_escapes_user_content() {
        let wrapped = html_wrap("<script>alert(1)</script>");
        assert!(wrapped.contains("&lt;script&gt;"));
        assert!(!wrapped.contains("<script>alert"));
    }

    #[test]
    fn speed_pct_for_levels_and_custom() {
        assert_eq!(speed_pct_for(Some("quiet"), None), Ok(35));
        assert_eq!(speed_pct_for(Some("balanced"), None), Ok(55));
        assert_eq!(speed_pct_for(Some("balanced_plus"), None), Ok(70));
        assert_eq!(speed_pct_for(Some("high"), None), Ok(85));
        assert_eq!(speed_pct_for(Some("urgent"), None), Ok(100));
        // 默认（无档位）走 balanced。
        assert_eq!(speed_pct_for(None, None), Ok(55));
        // custom：锚点 fan_pct 取均值。
        let anchors = json!([
            { "load_pct": 20, "fan_pct": 30 },
            { "load_pct": 60, "fan_pct": 70 },
            { "load_pct": 100, "fan_pct": 90 },
        ]);
        let arr = anchors.as_array().unwrap();
        assert_eq!(speed_pct_for(Some("custom"), Some(arr)), Ok(63)); // (30+70+90)/3
                                                                      // custom 空锚点报错。
        assert!(speed_pct_for(Some("custom"), Some(&[])).is_err());
        // 未知档位报错。
        assert!(speed_pct_for(Some("turbo"), None).is_err());
    }
}
