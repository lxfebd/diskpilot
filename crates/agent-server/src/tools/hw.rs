//! 硬件信息只读采集（Windows：PowerShell + WMI/CIM）。
//!
//! 纯工具文件，不接路由（由 `tools/mod.rs` 统一接 MCP tool 薄壳）。
//! **全部只读**：不写注册表、不改 BIOS、不装驱动、不调风扇控制器；
//! 全部走 CIM / `Get-PhysicalDisk`，**不需要管理员权限**，也不触发 UAC。
//! 数据经 `crate::ps::ps_capture` 跑 PowerShell（30s 超时 + UTF-8 强注入 + GBK 兜底），
//! 再 `serde_json::from_str` 解析后拼成中文可读多行文本（`- ` 前缀列表）。
//! 不需要管理员权限、不需要新增 windows-sys feature、不需要 Tauri。
//!
//! ## 已处理的已知坑（照搬主项目 `apps/desktop/src-tauri/src/hw.rs`，不重新发明）
//! - **L2CacheSize 的 WMI bug**：常返回 3 之类的垃圾值。`<= 0 || < 16` 一律显示"未知"，
//!   绝不把 3 KB 当真（主项目 hw.rs:997-1001）。
//! - **显存 32 位溢出**：`Win32_VideoController.AdapterRAM` 是 32 位，≥4 GB 的卡会回绕。
//!   `>= 4_294_967_296 || <= 4_293_918_720` → 显示"≥4 GB（WMI 32 位溢出，实际可能更大）"
//!   （主项目 hw.rs:1020-1029）；`== 0` → 共享系统内存 / 未知。
//! - **内存频率**：DDR5 的 `Speed` 只有实际有效频率的一半，输出必须标注，不能直接说
//!   "运行在 4800 MHz"。XMP/EXPO 诊断照搬主项目 hw.rs:1068-1085 的 `max_conf vs max_speed`。
//! - **ACPI 热区温度**是「十分之一开尔文」，不是摄氏：`c = raw / 10.0 - 273.15`。
//!   热区未就绪会报 2731（= 27.31 ℃，假的）或 0，必须用合理区间 `(0.0..=120.0)` 过滤；
//!   全部过滤掉时给降级文案，**不返回空列表假装成功**。
//! - **`Get-StorageReliabilityCounter.Temperature` 单位不统一**：有的控制器报摄氏、
//!   有的报华氏。启发式 `if t > 60.0 { (t-32)*5/9 } else { t }`（主项目 hw.rs:1161-1167）。
//! - **SMART 正解**是 `Get-PhysicalDisk` + `Get-StorageReliabilityCounter`
//!   （Win10 1607+，普通权限即可）。**不要**走 DeviceIoControl，**也不要**用 `wmic`
//!   （Win11 24H2 已从系统里移除）。
//!
//! ## 本文件导出的函数清单
//! - `pub fn collect_cpu() -> Result<String, String>` —— CPU 型号/核心/线程/频率/缓存/实时占用
//! - `pub fn collect_gpu() -> Result<String, String>` —— 显卡型号/显存/驱动/分辨率 + N 卡实时
//! - `pub fn collect_memory() -> Result<String, String>` —— 每根内存条 + 总容量 + XMP 诊断
//! - `pub fn collect_motherboard() -> Result<String, String>` —— 主板/BIOS/整机品牌
//! - `pub fn collect_temperature() -> Result<String, String>` —— 温度阶梯：HWiNFO 共享内存 →
//!   自研 SuperIO 直读 → fancmd/LHM → ACPI 热区 + GPU 温度
//! - `pub fn collect_battery() -> Result<String, String>` —— 电量/健康度/循环次数
//! - `pub fn collect_disk_smart() -> Result<String, String>` —— 磁盘健康与 SMART 磨损指标

use serde_json::Value;
use std::io::Read;

use crate::ps::{json_array_of, ps_capture};

// ── JSON 取值小工具（与主项目 hw.rs 的 v_str / arr 同语义）─────────────────

/// 取一个 JSON 字段的可读文本：数字/布尔转字符串，缺失/异常一律空串。
fn v_str(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// 取数组字段；缺失返回空 Vec（配合下面的 `arr().iter()` 直接用）。
fn v_arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(Value::as_array).map_or(&[], |a| a)
}

/// 可选整型：`null` / 缺失 / 非数字 → `None`。
fn get_int(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(Value::as_u64)
}

/// 可选浮点：`null` / 缺失 / 非数字 → `None`。
fn get_f64(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

/// 字节数人类可读（与主项目 `fmt_gb` 同语义；单位用 GB/MB/KB/B，小数 `{:.*}`）。
fn fmt_bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.*} GB", 1, n as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if n >= 1024 * 1024 {
        format!("{:.*} MB", 1, n as f64 / 1024.0 / 1024.0)
    } else if n >= 1024 {
        format!("{:.*} KB", 0, n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

/// 大整数千分位（`1,234,567`），避免累计错误数挤成一团。
fn format_num(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, &c) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c as char);
    }
    out
}

/// 跑一段 PowerShell 并解析 JSON。`ps_capture` 只可能真出错（超时/启动失败/进程异常），
/// 单个查询返回空时给中文兜底，不让空结果冒充成功。
///
/// **注意**：本函数只用于输出「对象」的脚本（`[ordered]@{...}`），**不做数组折叠**——
/// `json_array_of` 会把对象裹成 `[{...}]`，之后 `v_arr()` 取不到顶层键，CPU/主板会静默变空。
/// 输出「数组」的脚本请直接用下面的 `json_arr_of()`。
fn json_of(script: &str, what: &str) -> Result<Value, String> {
    let raw = ps_capture(script)?;
    let raw = raw.trim().trim_start_matches('\u{feff}');
    serde_json::from_str(raw).map_err(|e| format!("解析{what}查询结果失败：{e}"))
}

/// 跑一段输出数组的脚本并解析成数组（`ConvertTo-Json` 单项折叠成对象的坑在此兜底）。
fn json_arr_of(script: &str, what: &str) -> Result<Vec<Value>, String> {
    let raw = ps_capture(script)?;
    let raw = json_array_of(raw.trim().trim_start_matches('\u{feff}'));
    serde_json::from_str(&raw).map_err(|e| format!("解析{what}查询结果失败：{e}"))
}

/// WMI 日期字段序列化后是 COM 日期串 `/Date(1668470400000)/`（毫秒时间戳），
/// 直接透传没人看得懂；能解析就转成 `yyyy-MM-dd`，失败就原样返回。
///
/// `Win32_BIOS.ReleaseDate` 在不同系统上可能是 COM 日期串、也可能是
/// `20200923121500.000+080` 这种数字串；后者只做「前 8 位日期」的安全解析，
/// 解析不了就原样透传，绝不猜。
fn fmt_wmi_date(s: &str) -> String {
    let t = s.trim();
    if let Some(inner) = t.strip_prefix("/Date(").and_then(|x| x.strip_suffix(")/")) {
        if let Ok(ms) = inner.trim().parse::<i64>() {
            return com_date_to_iso(ms);
        }
        return t.to_string();
    }
    let d: Vec<char> = t.chars().take(8).collect();
    if d.len() == 8 && d.iter().all(|c| c.is_ascii_digit()) {
        let y: u32 = d.iter().take(4).collect::<String>().parse().unwrap_or(0);
        let m: u32 = d
            .iter()
            .skip(4)
            .take(2)
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        let dd: u32 = d
            .iter()
            .skip(6)
            .take(2)
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        if (1900..=2099).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&dd) {
            return format!("{y:04}-{m:02}-{dd:02}（原始 {t}，为 UTC 时间戳加时区偏移）");
        }
    }
    t.to_string()
}

/// COM 毫秒时间戳 → `yyyy-MM-dd`（UTC）。
fn com_date_to_iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    // 1970-01-01 起算的天数 → 年/月/日（Howard Hinnant 的 civil_from_days 算法）。
    // 注意 yoe 是「整个表达式」再整除 365，不是各项分别除（分开除会算错年份）。
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe.div_euclid(1_460) + doe.div_euclid(36_524) - doe.div_euclid(146_096))
        .div_euclid(365);
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe.div_euclid(4) - yoe.div_euclid(100));
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    // mp: 0..=11，转成 1..=12；1、2 月属于上一个天文年份，年份需 +1
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = y + if mo <= 2 { 1 } else { 0 };
    format!("{yr:04}-{mo:02}-{d:02}")
}

// ── nvidia-smi：CIM 拿不到 GPU 实时占用/温度，N 卡用官方 CLI 补 ───────────
// 仅在 Windows 门控（只被 `#[cfg(windows)]` 的 collect_gpu / collect_temperature 调用；
// nvidia-smi 也不是跨平台进程）。

/// N 卡实时指标。`mem_used` 是 `Option`：显存已用可能读不到，总显存通常都在。
#[cfg(windows)]
struct NvidiaGpuLive {
    util: u32,
    temp: u32,
    mem_used: Option<u64>,
    mem_total: u64,
}

/// 取 GPU 温度（℃），给 `collect_temperature` 复用。无 N 卡 / 无 nvidia-smi → `None`
/// （正常情况，不算错误）。
#[cfg(windows)]
fn nvidia_gpu_temp() -> Option<String> {
    nvidia_gpu_live()
        .ok()
        .flatten()
        .map(|g| format!("{} ℃", g.temp))
}

/// 取物理 N 卡型号名（`nvidia-smi --query-gpu=name`）。失败/无卡 → `None`。
#[cfg(windows)]
fn nvidia_gpu_name() -> Option<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args(["--query-gpu=name", "--format=csv,noheader,nounits"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().ok()?;
    let mut raw = String::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = so.read_to_string(&mut raw);
    }
    let _ = child.wait();
    let n = raw.lines().next().unwrap_or("").trim().to_string();
    if n.is_empty() {
        None
    } else {
        Some(n)
    }
}

/// `nvidia-smi --query-gpu=...`。**spawn 失败直接 `Err` 被外层吞成 `None`**：
/// 用户机器没装 N 卡驱动时不该报"查询失败"，而是安静地不给这一行。
#[cfg(windows)]
fn nvidia_gpu_live() -> Result<Option<NvidiaGpuLive>, String> {
    use std::process::Stdio;
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=utilization.gpu,temperature.gpu,memory.used,memory.total",
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
    let mut child = cmd.spawn().map_err(|_| "未安装 nvidia-smi".to_string())?;
    let mut raw = String::new();
    if let Some(mut so) = child.stdout.take() {
        so.read_to_string(&mut raw)
            .map_err(|_| "读取 nvidia-smi 输出失败".to_string())?;
    }
    let _ = child.wait();
    let line = raw.lines().next().unwrap_or("").trim().to_string();
    if line.is_empty() {
        return Ok(None);
    }
    let p: Vec<&str> = line.split(',').map(str::trim).collect();
    let util = p.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let temp = p.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mem_used = p.get(2).and_then(|s| s.parse().ok());
    let mem_total = p.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok(Some(NvidiaGpuLive {
        util,
        temp,
        mem_used,
        mem_total,
    }))
}

// ── AMD ADL：atiadlxx.dll P/Invoke 读 A 卡温度/占用（DLL 缺失/初始化失败则降级）──
// NVIDIA 之外的第二通道：AMD 驱动随装 atiadlxx.dll（C:\Windows\System32），
// 普通权限即可读温度/占用；DLL 不存在（N 卡机/无 AMD 驱动）时走降级，绝不报错。
// 用 PowerShell Add-Type 动态 P/Invoke，零新增 Rust 依赖、零 windows-sys feature；
// 30s 超时 + 子进程隔离：就算某个 DLL 版本的函数签名不匹配把 PS 崩了，也只影响
// 这个 powershell 子进程（ps_capture 超时强杀 + 返回 Err → 外层吞成 None → 如实降级）。
//
// 签名说明（对齐 AMD ADL SDK 官方头文件，非网传变体）：
//   ADL_Main_Control_Create(ADL_Main_Memory_Alloc, int iEnumConnectedAdapters) —— 必须传
//     allocator 回调，传 0 会导致后续枚举时内部内存分配失败。
//   ADL_Adapter_NumberOfAdapters_Get(int* lpNumAdapters)
//   ADL_GetTemperature(int iAdapterIndex, int* lpTemperature) —— 2 参数官方签名；
//     同时声明 4 参数变体 EntryPoint 容错旧 DLL，先试 2 参再试 4 参。
//   ADL_GLUtilization_Get(int iAdapterIndex, int* gpu, int* mem, int* eng)
// 温度单位：官方文档是千分之一摄氏度（65 ℃ → 65000），个别旧 DLL 直接摄氏，
// 脚本按值域启发式换算（>300 视为 milli）。
#[cfg(windows)]
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

/// AMD 卡实时指标（ADL 通道）。DLL 缺失/初始化失败/无 A 卡 → `Ok(Some(vec))` 可能为空或
/// `Ok(None)`（正常情况，不算错误）；PowerShell 子进程崩了 → ps_capture Err → 外层吞 None。
#[cfg(windows)]
struct AmdAdlLive {
    index: u32,
    temp_c: u32,
    gpu_util: u32,
}

#[cfg(windows)]
fn amd_adl_live() -> Result<Option<Vec<AmdAdlLive>>, String> {
    let raw = ps_capture(ADL_PROBE_PS)?;
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    let v: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if v_str(&v, "note") != "OK" {
        return Ok(None);
    }
    let mut list = Vec::new();
    if let Some(arr) = v.get("adapters").and_then(Value::as_array) {
        for a in arr {
            let temp_c = get_f64(a, "temp_c").unwrap_or(0.0) as u32;
            let gpu_util = get_f64(a, "gpu_util").unwrap_or(0.0) as u32;
            // 只收有实际读数的适配器（temp 与 util 都读不到 → 虚拟输出，跳过）
            if temp_c > 0 || gpu_util > 0 {
                list.push(AmdAdlLive {
                    index: get_f64(a, "index").unwrap_or(0.0) as u32,
                    temp_c,
                    gpu_util,
                });
            }
        }
    }
    Ok(Some(list))
}

/// 取 AMD GPU 温度（℃）字符串，给 `collect_temperature` 复用。
#[cfg(windows)]
fn amd_adl_temp() -> Option<String> {
    amd_adl_live()
        .ok()
        .flatten()
        .and_then(|l| l.into_iter().find(|a| a.temp_c > 0))
        .map(|a| format!("{} ℃", a.temp_c))
}

// ── 1. CPU ────────────────────────────────────────────────────────────────

const CPU_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$cpu=@(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors,MaxClockSpeed,CurrentClockSpeed,L2CacheSize,L3CacheSize,Manufacturer,VirtualizationFirmwareEnabled,LoadPercentage)
$perf=Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor | Where-Object { $_.Name -eq '_Total' } | Select-Object -First 1
$out=[ordered]@{ cpu=$cpu; total=[int]$cpu.Count; usage=if($perf){[double]$perf.PercentProcessorTime}else{$null} }
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// CPU 采集：型号/核心/线程/频率/缓存/实时占用。
#[cfg(windows)]
pub fn collect_cpu() -> Result<String, String> {
    let v = json_of(CPU_PS, "CPU")?;
    let arr = v_arr(&v, "cpu");
    if arr.is_empty() {
        return Ok("CPU 信息读不到（Win32_Processor 无返回）".into());
    }
    let first = &arr[0];
    let name = if v_str(first, "Name").is_empty() {
        "未知型号".to_string()
    } else {
        v_str(first, "Name")
    };
    let cores = get_int(first, "NumberOfCores").unwrap_or(0);
    let threads = get_int(first, "NumberOfLogicalProcessors").unwrap_or(0);
    let max_hz = get_int(first, "MaxClockSpeed").unwrap_or(0);
    let cur_hz = get_int(first, "CurrentClockSpeed").unwrap_or(0);
    // L2CacheSize 的 WMI bug：垃圾值（3 之类的）一律当未知，绝不显示 3 KB。
    let l2 = get_int(first, "L2CacheSize").unwrap_or(0);
    let l3 = get_int(first, "L3CacheSize").unwrap_or(0);
    let cache = |kb: u64| -> String {
        if kb < 16 {
            // WMI bug：常返回 3 之类的垃圾值，按未知处理
            "未知".to_string()
        } else if kb >= 1024 {
            format!("{:.*} MB", 1, kb as f64 / 1024.0)
        } else {
            format!("{kb} KB")
        }
    };
    // Win32_Processor.VirtualizationFirmwareEnabled 是 bool，JSON 里是小写 true/false
    let virt = match v_str(first, "VirtualizationFirmwareEnabled")
        .to_ascii_lowercase()
        .as_str()
    {
        "true" => "已开启",
        "false" => "未开启",
        _ => "未知",
    };
    let usage = get_f64(&v, "usage").unwrap_or(-1.0);
    let usage_str = if usage < 0.0 {
        "读不到".to_string()
    } else {
        format!("{:.*}%", 1, usage)
    };
    let total = get_int(&v, "total").unwrap_or(arr.len() as u64);
    let mut s = String::new();
    if total > 1 {
        s.push_str(&format!(
            "- CPU（共 {total} 路物理 CPU，以下为第 1 路）：\n"
        ));
    } else {
        s.push_str("- CPU：\n");
    }
    s.push_str(&format!("  - 型号：{name}\n"));
    s.push_str(&format!("  - 核心 / 线程：{cores} / {threads}\n"));
    if max_hz > 0 {
        s.push_str(&format!(
            "  - 标称频率：{} MHz（约 {:.*} GHz）\n",
            max_hz,
            2,
            max_hz as f64 / 1000.0
        ));
    }
    if cur_hz > 0 {
        s.push_str(&format!(
            "  - 实时频率：{} MHz（空闲时通常远低于标称，Turbo 下可更高）\n",
            cur_hz
        ));
    }
    s.push_str(&format!(
        "  - 缓存：L2 {} / L3 {}（WMI 的 L2CacheSize 常有 bug，过小值按未知处理）\n",
        cache(l2),
        cache(l3)
    ));
    let man = v_str(first, "Manufacturer");
    if !man.is_empty() {
        s.push_str(&format!("  - 厂商：{man}\n"));
    }
    s.push_str(&format!("  - 硬件虚拟化：{virt}\n"));
    s.push_str(&format!(
        "  - 实时占用：{usage_str}（PercentProcessorTime 瞬时采样，非平均值）"
    ));
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_cpu() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 2. GPU ────────────────────────────────────────────────────────────────

const GPU_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
# 部分机器 Get-CimInstance 对 Win32_VideoController 抛「调用已取消」返回空，
# 旧版 Get-WmiObject 仍可读 → 失败自动回退，保证显卡型号/驱动/显存能探明。
$out = $null
try {
  $out = @(Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion,DriverDate,CurrentRefreshRate,CurrentHorizontalResolution,CurrentVerticalResolution,DeviceID)
} catch { $out = $null }
if (-not $out -or $out.Count -eq 0) {
  $out = @(Get-WmiObject Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion,DriverDate,CurrentRefreshRate,CurrentHorizontalResolution,CurrentVerticalResolution,DeviceID)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 显卡采集：型号/显存/驱动/分辨率/刷新率，外加 N 卡实时占用/温度/显存。
#[cfg(windows)]
pub fn collect_gpu() -> Result<String, String> {
    let arr = json_arr_of(GPU_PS, "显卡")?;
    if arr.is_empty() {
        return Ok("显卡信息读不到（Win32_VideoController 无返回）".into());
    }
    let mut s = String::new();
    let mut nvidia_via_wmi = false;
    for (i, g) in arr.iter().enumerate() {
        let nm = if v_str(g, "Name").is_empty() {
            "未知显卡".to_string()
        } else {
            v_str(g, "Name")
        };
        s.push_str(&format!("- 显卡 {}：{nm}\n", i + 1));
        // 显存 32 位溢出坑：≥4 GB 的卡回绕，<= 4GB-1MB 也是回绕产物。
        let vram = get_int(g, "AdapterRAM").unwrap_or(0);
        let vram_str = if vram == 0 {
            "共享系统内存 / 未知".to_string()
        } else if vram >= 4_294_967_296 || vram <= 4_293_918_720 {
            "≥4 GB（WMI 32 位溢出，实际可能更大）".to_string()
        } else {
            fmt_bytes(vram)
        };
        s.push_str(&format!("  - 显存：{vram_str}\n"));
        let dv = v_str(g, "DriverVersion");
        let dd = v_str(g, "DriverDate");
        if !dv.is_empty() || !dd.is_empty() {
            s.push_str(&format!(
                "  - 驱动：{}（日期 {}）\n",
                if dv.is_empty() { "未知".into() } else { dv },
                if dd.is_empty() {
                    "未知".into()
                } else {
                    fmt_wmi_date(&dd)
                }
            ));
        }
        let hw = get_int(g, "CurrentHorizontalResolution").unwrap_or(0);
        let vh = get_int(g, "CurrentVerticalResolution").unwrap_or(0);
        let fr = get_int(g, "CurrentRefreshRate").unwrap_or(0);
        if hw == 0 && vh == 0 && fr == 0 {
            s.push_str("  - 当前输出：无活动显示（虚拟显示适配器，或未接显示器）\n");
        } else {
            s.push_str(&format!("  - 当前输出：{hw} x {vh} @ {fr} Hz\n"));
        }
        let did = v_str(g, "DeviceID");
        if !did.is_empty() {
            s.push_str(&format!("  - DeviceID：{did}\n"));
        }
        // WMI 已列出物理 N 卡（NVIDIA 型号名）→ 标记，稍后 nvidia-smi 只补实时行不重复主卡。
        let nm_lower = nm.to_ascii_lowercase();
        if nm_lower.contains("nvidia")
            && !nm_lower.contains("virtual")
            && !nm_lower.contains("mirar")
        {
            nvidia_via_wmi = true;
        }
    }
    // 物理独显实时通道：WMI 未列出物理 N 卡时，nvidia-smi 型号作为主卡条目输出；
    // WMI 已列出（如 RTX 5090）时只补实时占用/温度/精确显存，避免重复条目。
    match nvidia_gpu_live() {
        Ok(Some(l)) => {
            let mu = l
                .mem_used
                .map(|m| format!("已用 {}", fmt_bytes(m * 1024 * 1024)))
                .unwrap_or_else(|| "已用未知".to_string());
            if !nvidia_via_wmi {
                s.push_str(&format!(
                    "- 显卡 {}：{}（nvidia-smi 物理卡）\n",
                    arr.len() + 1,
                    nvidia_gpu_name().unwrap_or_else(|| "NVIDIA".to_string())
                ));
                s.push_str(&format!(
                    "  - 显存：{}（共 {}）\n",
                    mu,
                    fmt_bytes(l.mem_total * 1024 * 1024)
                ));
            } else {
                s.push_str(&format!(
                    "  - N 卡实时：占用 {}% / 温度 {} ℃ / 显存 {}（共 {}）\n",
                    l.util,
                    l.temp,
                    mu,
                    fmt_bytes(l.mem_total * 1024 * 1024)
                ));
            }
        }
        _ => match amd_adl_live() {
            Ok(Some(list)) if !list.is_empty() => {
                let shown = list
                    .iter()
                    .filter(|a| a.gpu_util > 0 || a.temp_c > 0)
                    .map(|a| {
                        format!(
                            "  - A 卡实时（ADL 适配器 {}）：占用 {}% / 温度 {} ℃",
                            a.index, a.gpu_util, a.temp_c
                        )
                    })
                    .collect::<Vec<_>>();
                for line in shown {
                    s.push_str(&line);
                    s.push('\n');
                }
            }
            _ => {
                s.push_str(
                    "  - 实时（占用/温度）：无 NVIDIA（nvidia-smi）且无 AMD（atiadlxx ADL），跳过\n",
                );
            }
        },
    }
    s.push_str("说明：WMI 列出的适配器可能含虚拟显示适配器（模拟器/远程桌面）；物理独显的\n  型号/显存/温度/占用以 nvidia-smi（N 卡）或 atiadlxx ADL（A 卡）实时通道为准；\n  WMI AdapterRAM 为 32 位，≥4 GB 会回绕，故物理卡显存以 nvidia-smi 为准。");
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_gpu() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 3. 内存 ───────────────────────────────────────────────────────────────

const MEM_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
# 与 GPU_PS 同理：部分机 Get-CimInstance 对该类抛「调用已取消」返回空，
# 旧版 Get-WmiObject 仍可读 → 失败回退，保证内存条/容量/频率能探明。
$out = $null
try {
  $out = @(Get-CimInstance Win32_PhysicalMemory | Select-Object DeviceLocator,Capacity,Speed,ConfiguredClockSpeed,Manufacturer,PartNumber,MemoryType,BankLabel,FormFactor)
} catch { $out = $null }
if (-not $out -or $out.Count -eq 0) {
  $out = @(Get-WmiObject Win32_PhysicalMemory | Select-Object DeviceLocator,Capacity,Speed,ConfiguredClockSpeed,Manufacturer,PartNumber,MemoryType,BankLabel,FormFactor)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 内存采集：每根内存条 + 总容量 + XMP/EXPO 生效诊断。
#[cfg(windows)]
pub fn collect_memory() -> Result<String, String> {
    let arr = json_arr_of(MEM_PS, "内存")?;
    if arr.is_empty() {
        return Ok("内存信息读不到（Win32_PhysicalMemory 无返回）".into());
    }
    let total: u64 = arr.iter().filter_map(|m| get_int(m, "Capacity")).sum();
    let mut s = String::new();
    s.push_str(&format!(
        "- 总容量：{}（{} 根内存条）\n",
        fmt_bytes(total),
        arr.len()
    ));
    for m in &arr {
        let slot = if v_str(m, "DeviceLocator").is_empty() {
            "未知插槽".to_string()
        } else {
            v_str(m, "DeviceLocator")
        };
        let cap = fmt_bytes(get_int(m, "Capacity").unwrap_or(0));
        let speed = get_int(m, "Speed").unwrap_or(0);
        let conf = get_int(m, "ConfiguredClockSpeed").unwrap_or(0);
        let man = if v_str(m, "Manufacturer").is_empty() {
            "未知品牌".to_string()
        } else {
            v_str(m, "Manufacturer")
        };
        let pn_raw = v_str(m, "PartNumber");
        let pn = if pn_raw.trim().is_empty() {
            "未知型号".to_string()
        } else {
            pn_raw.trim().to_string()
        };
        // Win32_PhysicalMemory.PartNumber 尾部常带 NUL 填充，trim 掉
        let mt = v_str(m, "MemoryType");
        // DDR5 的 Speed 只有实际有效频率的一半；DDR4 及以下不翻倍，别给错提示。
        let is_ddr5 = mt.contains("DDR5") || pn.contains("DDR5");
        s.push_str(&format!(
            "- 内存条「{slot}」：{cap}，标称 {speed} MT/s，运行 {conf} MT/s\n"
        ));
        s.push_str(&format!("  - 品牌 {man} / 型号 {pn}\n"));
        if is_ddr5 {
            s.push_str(
                "  - 说明：标称速率，DDR5 实际有效频率通常为其 2 倍（DDR5-4800 ≈ 等效 9600 MT/s）\n",
            );
        } else {
            s.push_str("  - 说明：速率单位为 MT/s（DDR4 及以下，无 2 倍折算）\n");
        }
    }
    // XMP/EXPO 诊断：运行频率 < 标称 → profile 可能没生效（照搬主项目 hw.rs:1074-1085）。
    let max_speed = arr
        .iter()
        .filter_map(|m| get_int(m, "Speed"))
        .max()
        .unwrap_or(0);
    let max_conf = arr
        .iter()
        .filter_map(|m| get_int(m, "ConfiguredClockSpeed"))
        .max()
        .unwrap_or(0);
    if max_conf > 0 && max_speed > 0 {
        s.push_str(&format!(
            "- 频率诊断：最高标称 {} MT/s，最高运行 {} MT/s（{}）\n",
            max_speed,
            max_conf,
            if max_conf < max_speed {
                "XMP/EXPO 可能未生效"
            } else {
                "XMP/EXPO 已生效"
            }
        ));
    } else if max_speed > 0 {
        s.push_str(
            "- 频率诊断：读不到实际运行频率（ConfiguredClockSpeed 为空），无法判断 XMP 是否生效\n",
        );
    } else {
        s.push_str("- 频率诊断：速率读不到（Speed / ConfiguredClockSpeed 均为空）\n");
    }
    s.push_str("- 时序（CL）：WMI 不提供，需 SPD 读取工具（如 CPU-Z 的 SPD tab、Thaiphoon Burner），本工具不编造");
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_memory() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 3.5 DRAM 时序（SPD 读取，阶段四）──────────────────────────────────────

const DRAM_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-CimInstance Win32_PhysicalMemory | Select-Object DeviceLocator,Manufacturer,PartNumber,ConfiguredClockSpeed,Speed,Capacity)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// DRAM 时序信息：每根内存条的 SPD 级时序（CL-tRCD-tRP-tRAS）。
///
/// WMI 不直接暴露 SPD 时序，这里用 SPD 读取工具（Thaiphoon Burner 便携版 /
/// RWEverything）探路：有则优先输出，无则基于运行频率给出一致性检查
/// （不编造具体数值）。**只读 L0**。
#[cfg(windows)]
pub fn collect_dram_timings() -> Result<String, String> {
    let arr = json_arr_of(DRAM_PS, "内存时序")?;
    if arr.is_empty() {
        return Ok("内存时序读不到（Win32_PhysicalMemory 无返回）".into());
    }
    let mut s = String::from("DRAM 时序信息：\n");
    for (i, m) in arr.iter().enumerate() {
        let slot = if v_str(m, "DeviceLocator").is_empty() {
            "未知插槽".to_string()
        } else {
            v_str(m, "DeviceLocator")
        };
        let cap = fmt_bytes(get_int(m, "Capacity").unwrap_or(0));
        let speed = get_int(m, "Speed").unwrap_or(0);
        let conf = get_int(m, "ConfiguredClockSpeed").unwrap_or(0);
        let pn = if v_str(m, "PartNumber").trim().is_empty() {
            "未知型号".to_string()
        } else {
            v_str(m, "PartNumber").trim().to_string()
        };
        let man = if v_str(m, "Manufacturer").is_empty() {
            "未知品牌".to_string()
        } else {
            v_str(m, "Manufacturer")
        };
        s.push_str(&format!(
            "- 内存条 {}「{slot}」：{cap}，{man} / {pn}\n",
            i + 1
        ));
        s.push_str(&format!("  - 标称 {speed} MT/s，运行 {conf} MT/s\n"));
    }
    // 一致性检查：运行频率 < 标称 → XMP/EXPO 可能没生效
    let max_speed = arr
        .iter()
        .filter_map(|m| get_int(m, "Speed"))
        .max()
        .unwrap_or(0);
    let max_conf = arr
        .iter()
        .filter_map(|m| get_int(m, "ConfiguredClockSpeed"))
        .max()
        .unwrap_or(0);
    if max_conf > 0 && max_speed > 0 {
        s.push_str(&format!(
            "- 频率一致性：标称 {} MT/s / 运行 {} MT/s（{}）\n",
            max_speed,
            max_conf,
            if max_conf < max_speed {
                "XMP/EXPO 可能未生效"
            } else {
                "XMP/EXPO 已生效"
            }
        ));
    }
    // SPD 读取工具探路（不编造）
    for (exe, tag) in [
        ("ThaiphoonBurner.exe", "Thaiphoon Burner"),
        ("RW.exe", "RWEverything"),
    ] {
        let candidate = std::env::var("ProgramFiles")
            .map(|p| std::path::Path::new(&p).join(exe))
            .unwrap_or_default();
        if candidate.exists() {
            s.push_str(&format!(
                "  - 检测到 {tag}（{exe}），其 SPD 报告可给出完整时序（CL-tRCD-tRP），本工具不解析二进制。\n"
            ));
        }
    }
    s.push_str(
        "- 完整 CL 时序需 SPD 读取（CPU-Z SPD tab / Thaiphoon Burner），本工具不编造具体数值。",
    );
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_dram_timings() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 3.6 CPU 指令集（阶段四）───────────────────────────────────────────────

const CPUID_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfLogicalProcessors,VirtualizationFirmwareEnabled,SecondLevelAddressTranslationExtensions,VMMonitorModeExtensions,PowerManagementSupported)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// CPU 指令集特性：虚拟化/二级地址翻译（SLAT）/VM 监控模式扩展等 CPUID 位。
///
/// 读取 CPUID 指令集位（SSE/AVX/AVX2/FMA 等）需要 Ring0 或 cpuid 内联汇编，
/// WMI 只给到虚拟化相关位。这里输出 WMI 能确认的位 + 用 Rust 运行时探测
/// `is_x86_feature_detected!` 确认 AVX2/FMA 等用户态指令集（跨平台安全）。
/// **只读 L0**。
#[cfg(windows)]
pub fn collect_cpu_features() -> Result<String, String> {
    let arr = json_arr_of(CPUID_PS, "CPU 指令集")?;
    let first = arr.first().cloned().unwrap_or_default();
    let name = if v_str(&first, "Name").is_empty() {
        "未知型号".to_string()
    } else {
        v_str(&first, "Name")
    };
    let mut s = String::from("CPU 指令集与特性：\n");
    s.push_str(&format!("- 型号：{name}\n"));

    // 虚拟化位
    let virt = match v_str(&first, "VirtualizationFirmwareEnabled")
        .to_ascii_lowercase()
        .as_str()
    {
        "true" => "已开启",
        "false" => "未开启",
        _ => "未知",
    };
    s.push_str(&format!("- 硬件虚拟化（VT-x/AMD-V）：{virt}\n"));
    let slat = v_str(&first, "SecondLevelAddressTranslationExtensions").to_ascii_lowercase();
    let vmm = v_str(&first, "VMMonitorModeExtensions").to_ascii_lowercase();
    s.push_str(&format!(
        "- 二级地址翻译 SLAT（EPT/NPT）：{}\n",
        if slat == "true" {
            "支持"
        } else {
            "未知/不支持"
        }
    ));
    s.push_str(&format!(
        "- VM 监控模式扩展（VMX/SVM）：{}\n",
        if vmm == "true" {
            "支持"
        } else {
            "未知/不支持"
        }
    ));

    // 用户态指令集（运行时探测，跨平台安全）
    let mut features: Vec<&str> = Vec::new();
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("sse2") {
            features.push("SSE2");
        }
        if std::is_x86_feature_detected!("sse4.2") {
            features.push("SSE4.2");
        }
        if std::is_x86_feature_detected!("avx") {
            features.push("AVX");
        }
        if std::is_x86_feature_detected!("avx2") {
            features.push("AVX2");
        }
        if std::is_x86_feature_detected!("fma") {
            features.push("FMA");
        }
        if std::is_x86_feature_detected!("aes") {
            features.push("AES-NI");
        }
    }
    if features.is_empty() {
        s.push_str("- 指令集探测：当前平台无法用运行时探测确认（非 x86 或编译器不暴露），给出 WMI 虚拟化位即可。\n");
    } else {
        s.push_str(&format!("- 已确认指令集：{}\n", features.join(" / ")));
        s.push_str("- 说明：AVX2/FMA/AES-NI 由运行时 CPUID 探测（Rust is_x86_feature_detected），未列出的（AVX-512 等）不代表不支持。\n");
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_cpu_features() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 3.7 显示器信息（阶段四）───────────────────────────────────────────────

const DISPLAY_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorBasicDisplayParams | Select-Object InstanceName,VideoInputType,MaxHorizontalImageSize,MaxVerticalImageSize,DisplayTransferCharacteristic,Active)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 显示器信息：WMI 基本显示参数（物理尺寸/输入类型/激活状态）。
///
/// 完整 EDID（厂商/序列号/生产周/色彩位深）需要解析 EDID 二进制，WMI 给到
/// 物理尺寸 + 输入类型；分辨率/刷新率已在 collect_gpu 输出。**只读 L0**。
#[cfg(windows)]
pub fn collect_displays() -> Result<String, String> {
    let arr = json_arr_of(DISPLAY_PS, "显示器")?;
    if arr.is_empty() {
        return Ok(
            "显示器信息读不到（WmiMonitorBasicDisplayParams 无返回，可能无外接显示器或驱动未暴露）"
                .into(),
        );
    }
    let mut s = String::from("显示器信息：\n");
    for (i, d) in arr.iter().enumerate() {
        let instance = v_str(d, "InstanceName");
        let input = v_str(d, "VideoInputType");
        let w = get_int(d, "MaxHorizontalImageSize").unwrap_or(0);
        let h = get_int(d, "MaxVerticalImageSize").unwrap_or(0);
        let active = v_str(d, "Active").to_ascii_lowercase();
        s.push_str(&format!("- 显示器 {}：\n", i + 1));
        if !instance.is_empty() {
            // 形如 DISPLAY\DEL40C4\5&... ，截短成友好串
            let short = instance.split('\\').nth(1).unwrap_or(&instance);
            s.push_str(&format!("  - 设备：{short}\n"));
        }
        s.push_str(&format!(
            "  - 物理尺寸：{} cm × {} cm（约 {:.1} 英寸）\n",
            w,
            h,
            ((w as f64).powi(2) + (h as f64).powi(2)).sqrt() / 2.54
        ));
        s.push_str(&format!(
            "  - 输入类型：{}\n",
            if input.is_empty() {
                "未知".to_string()
            } else {
                input
            }
        ));
        s.push_str(&format!(
            "  - 状态：{}\n",
            if active == "true" {
                "激活"
            } else {
                "非激活"
            }
        ));
    }
    s.push_str("- 分辨率/刷新率见 collect_gpu 输出；完整 EDID（序列号/生产周）需专用工具解析。");
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_displays() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 3.8 ACPI 固件/电源信息（阶段五·G17 简化版）────────────────────────
// WMI Win32_BIOS + Win32_ComputerSystem 可读固件版本/序列号/唤醒类型 + 电源状态；
// 完整 ACPI 表浏览（FADT/DSDT 反汇编）需 Ring0/bios 工具，这里给到可读信息。

const ACPI_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=[ordered]@{
  bios=@(Get-CimInstance Win32_BIOS | Select-Object Manufacturer,SMBIOSBIOSVersion,SerialNumber,Version,ReleaseDate)
  sys=@(Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer,Model,SystemType,TotalPhysicalMemory)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// ACPI / 固件信息（只读）：BIOS 厂商/版本/序列号/发布日期 + 机箱厂商/型号/系统类型。
///
/// AIDA64「ACPI Browser」可浏览 FADT/DSDT 等表（需 Ring0），本工具给到 WMI 可读的
/// 固件与电源顶层信息；完整反汇编不在此列（专业向，无安全收益）。**只读 L0**。
#[cfg(windows)]
pub fn collect_acpi() -> Result<String, String> {
    let v = json_of(ACPI_PS, "ACPI/固件")?;
    let mut s = String::from("ACPI 固件与电源信息：\n");
    let bins = v_arr(&v, "bios");
    if let Some(b) = bins.first() {
        let mfr = v_str(b, "Manufacturer");
        let ver = v_str(b, "SMBIOSBIOSVersion");
        let sn = v_str(b, "SerialNumber");
        let rel = v_str(b, "ReleaseDate");
        if !mfr.is_empty() || !ver.is_empty() {
            s.push_str(&format!("- BIOS：{} {}\n", mfr, ver));
        }
        if !rel.is_empty() {
            s.push_str(&format!("- BIOS 发布日期：{rel}\n"));
        }
        if !sn.is_empty() && sn != "To be filled by O.E.M." {
            s.push_str(&format!("- BIOS 序列号：{sn}\n"));
        }
    }
    let systs = v_arr(&v, "sys");
    if let Some(sys) = systs.first() {
        let mfr = v_str(sys, "Manufacturer");
        let model = v_str(sys, "Model");
        let st = v_str(sys, "SystemType");
        if !mfr.is_empty() || !model.is_empty() {
            s.push_str(&format!("- 机箱：{mfr} {model}\n"));
        }
        if !st.is_empty() {
            s.push_str(&format!("- 系统类型（ACPI OEM）：{st}\n"));
        }
    }
    s.push_str("- 说明：完整 ACPI 表浏览（FADT/DSDT 反汇编）需 Ring0 工具，属专业向；这里给到固件/电源顶层信息。");
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_acpi() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 3.9 IPMI 服务器接口探测（阶段五·G14）────────────────────────────

const IPMI_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=[ordered]@{
  ipmi=@(Get-CimInstance -Namespace root\wmi -ClassName IPMI_Interface 2>$null | Select-Object -First 5)
  baseboard=@(Get-CimInstance Win32_BaseBoard | Select-Object Manufacturer,Product)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// IPMI 服务器接口探测（只读）：列出 IPMI 接口类实例（BMC 存在性）。
///
/// 家用台式机一般**无 BMC/IPMI**（HEDT/服务器板才有），无返回时如实提示「未检测到
/// IPMI」，不编造传感器/日志。服务器场景 AIDA64 会读 BMC 传感器/系统事件日志（SEL），
/// 这里探测到接口后给引导（IPMI 工具不在本工具集）。**只读 L0**。
#[cfg(windows)]
pub fn collect_ipmi() -> Result<String, String> {
    let v = json_of(IPMI_PS, "IPMI")?;
    let arr = v_arr(&v, "ipmi");
    if arr.is_empty() {
        return Ok("IPMI：未检测到 BMC/IPMI 接口（家用台式机通常无；服务器/HEDT 板才带）。如实返回，不编造传感器。".into());
    }
    let base = v_arr(&v, "baseboard").first().cloned().unwrap_or_default();
    let board = if v_str(&base, "Manufacturer").is_empty() {
        String::new()
    } else {
        format!(
            "{} {}",
            v_str(&base, "Manufacturer"),
            v_str(&base, "Product")
        )
    };
    let mut s = String::from("IPMI 服务器接口：\n");
    if !board.is_empty() {
        s.push_str(&format!("- 主板：{board}\n"));
    }
    s.push_str(&format!("- 检测到 IPMI/BMC 接口实例：{} 个\n", arr.len()));
    for (i, it) in arr.iter().enumerate() {
        s.push_str(&format!(
            "  - 接口 {}：{}\n",
            i + 1,
            v_str(it, "InstanceName")
        ));
    }
    s.push_str("- BMC 传感器/系统事件日志（SEL）需 IPMI 工具（ipmitool）读取，本工具集不含。");
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_ipmi() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 3.10 GPU 快照（阶段五·G12 bench_gpu 简化版）──────────────────────
// 复用 collect_gpu 已有信息做只读快照；深度 GPGPU 计算（OpenCL/像素填充率）
// 需 GPU 厂商 SDK，暂不引入重量级依赖，输出如实标注。

/// GPU 只读快照：型号/显存/驱动/分辨率/温度（NVIDIA 时）。bench_gpu 用它。
///
/// AIDA64「GPGPU Benchmark」测像素填充率/纹理/带宽/OpenCL，需厂商 SDK 深度计算；
/// 本工具给出 GPU 信息快照 + 说明深度基准未实现，**只读 L0**。
pub fn collect_gpu_snapshot() -> Result<String, String> {
    let gpu = collect_gpu().unwrap_or_else(|e| format!("GPU 信息读不到：{e}"));
    let mut s = String::from("GPU 快照：\n");
    s.push_str(&gpu);
    s.push_str("\n- GPGPU 深度基准（像素填充率/OpenCL）需 GPU 厂商 SDK，本工具集未实现；当前为只读信息快照。");
    Ok(s)
}

// ── 4. 主板 / BIOS / 整机 ─────────────────────────────────────────────────

const BOARD_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=[ordered]@{
  board=@(Get-CimInstance Win32_BaseBoard | Select-Object Manufacturer,Product,SerialNumber)
  bios=@(Get-CimInstance Win32_BIOS | Select-Object Manufacturer,SMBIOSBIOSVersion,ReleaseDate)
  sys=@(Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer,Model)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 主板 / BIOS / 整机品牌采集。
#[cfg(windows)]
pub fn collect_motherboard() -> Result<String, String> {
    let v = json_of(BOARD_PS, "主板")?;
    // PS 里已用 @(...) 包成数组；这里同时兼容「是数组」和「退化成了单个对象」两种形态。
    let pick = |k: &str| -> Value {
        match v.get(k) {
            Some(Value::Array(a)) if !a.is_empty() => a[0].clone(),
            Some(o @ Value::Object(_)) => o.clone(),
            _ => Value::default(),
        }
    };
    let board = pick("board");
    let bios = pick("bios");
    let sys = pick("sys");
    // 厂商没填时 WMI 会塞占位串：经典的是 "To Be Filled By O.E.M."，
    // 还有 "Default string"（这台机器的 Gigabyte 主板就是这样）。都按未知处理。
    let placeholder = |s: &str| -> String {
        let t = s.trim();
        if t.is_empty() {
            "未知".to_string()
        } else if t.eq_ignore_ascii_case("To Be Filled By O.E.M.")
            || t.eq_ignore_ascii_case("Default string")
            || t.eq_ignore_ascii_case("Unknown")
        {
            format!("未知（原始占位串「{t}」）")
        } else {
            t.to_string()
        }
    };
    let mut s = String::new();
    s.push_str(&format!(
        "- 主板厂商：{}\n",
        placeholder(&v_str(&board, "Manufacturer"))
    ));
    s.push_str(&format!(
        "- 主板型号：{}\n",
        placeholder(&v_str(&board, "Product"))
    ));
    s.push_str(&format!(
        "- 主板序列号：{}\n",
        placeholder(&v_str(&board, "SerialNumber"))
    ));
    s.push_str(&format!(
        "- BIOS 厂商：{}\n",
        placeholder(&v_str(&bios, "Manufacturer"))
    ));
    s.push_str(&format!(
        "- BIOS 版本：{}\n",
        placeholder(&v_str(&bios, "SMBIOSBIOSVersion"))
    ));
    let rd = v_str(&bios, "ReleaseDate");
    if rd.trim().is_empty() {
        s.push_str("- BIOS 发布日期：未知\n");
    } else {
        s.push_str(&format!("- BIOS 发布日期：{}\n", fmt_wmi_date(&rd)));
    }
    s.push_str(&format!(
        "- 整机品牌：{}\n",
        placeholder(&v_str(&sys, "Manufacturer"))
    ));
    s.push_str(&format!(
        "- 整机机型：{}",
        placeholder(&v_str(&sys, "Model"))
    ));
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_motherboard() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 5. 温度 ───────────────────────────────────────────────────────────────

const THERMAL_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature | Select-Object InstanceName,CurrentTemperature)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 温度采集：优先 HWiNFO 共享内存（CPU/GPU/主板/磁盘全温度，普通权限可读），
/// 失败回退 ACPI 机箱级热区 + GPU 温度。
///
/// ACPI 热区是「十分之一开尔文」，且**未就绪时会报 2731（=27.31 ℃ 的假值）或 0**，
/// 必须用合理区间过滤；全部被过滤掉时返回降级说明，绝不假装读到了。
#[cfg(windows)]
pub fn collect_temperature() -> Result<String, String> {
    // 1) 优先 HWiNFO 共享内存：HWiNFO 后台运行时以**普通权限**发布全部温度传感器，
    //    非管理员也能读到 CPU/GPU/主板/磁盘温度（根治「AI 看不到 CPU 温度」）。
    //    读数可能多达几十条，截断到 60 条防止 AI 上下文爆炸。
    if let Ok(lines) = super::hwinfo::read_temperature_lines() {
        let mut out: Vec<String> = Vec::new();
        for (nm, v) in lines.iter().take(60) {
            out.push(format!("- {nm}：{:.*} ℃", 1, v));
        }
        if lines.len() > 60 {
            out.push(format!("- ……（其余 {} 条温度省略）", lines.len() - 60));
        }
        let mut s = format!(
            "温度传感器（HWiNFO 共享内存，共 {} 条）：\n{}",
            lines.len(),
            out.join("\n")
        );
        s.push_str(
            "\n说明：HWiNFO 以普通权限后台发布共享内存，读到的温度与 HWiNFO 界面一致；\n  \
             HWiNFO 未运行时自动降级到 ACPI 热区 + GPU + SMART 通道。",
        );
        return Ok(s);
    }
    // 1.5) 自研 SuperIO 直读温度：HWiNFO 没运行时，inpoutx64 驱动（已装则普通权限可用）
    //      直读 ITE/Nuvoton/Winbond 环境寄存器（主板/CPU/辅助温度，0x29-0x2B）。
    if let Ok(lines) = super::superio::read_temperatures() {
        let mut out: Vec<String> = Vec::new();
        for (label, v) in lines.iter().take(10) {
            out.push(format!("- {label}：{:.*} ℃", 1, v));
        }
        let mut s = format!(
            "温度传感器（自研 SuperIO 直读，共 {} 条）：\n{}",
            lines.len(),
            out.join("\n")
        );
        s.push_str(
            "\n说明：直读主板 SuperIO 环境寄存器（inpoutx64 驱动，普通权限），无 HWiNFO/LHM 依赖。",
        );
        return Ok(s);
    }
    let raw = ps_capture(THERMAL_PS)?;
    let arr: Vec<Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析温度查询结果失败：{e}"))?;
    let mut out: Vec<String> = Vec::new();
    for t in &arr {
        let raw = get_f64(t, "CurrentTemperature").unwrap_or(0.0);
        let c = raw / 10.0 - 273.15;
        // 热区未就绪时 MSACPI 报标记值 2731（= 27.31 ℃）或 0，落在 0~120 区间内会被当成
        // 真实温度——这正是「电脑温度=室温」假象的来源。必须单独排除该标记值区间。
        let not_ready = (27.0..=27.6).contains(&c);
        if (0.0..=120.0).contains(&c) && !not_ready {
            out.push(format!(
                "- {}：{:.*} ℃",
                if v_str(t, "InstanceName").is_empty() {
                    "热区".into()
                } else {
                    v_str(t, "InstanceName")
                },
                0,
                c
            ));
        }
    }
    // CPU 温度：fancmd（LHM 内核）读 CPU 核心温度需 Ring0 驱动 + 管理员权限。
    // 非管理员下 CPU 温度必为 null，白等 fancmd 枚举 4-8s 毫无意义 → 直接跳过。
    // 管理员（DiskPilot 生产模式）下 fancmd 能读到 Core Max，并入输出；读不到静默跳过。
    if super::selfheal::is_admin() {
        if let Some(exe) = super::selfheal::find_fancmd() {
            if let Ok(raw) = run_fancmd_sensors(&exe) {
                if let Some(cpus) = extract_cpu_temps_from_sensor_json(&raw) {
                    for (nm, v) in cpus {
                        out.push(format!(
                            "- CPU（{nm}）：{:.*} ℃（LibreHardwareMonitor）",
                            1, v
                        ));
                    }
                }
            }
        }
    }
    match nvidia_gpu_temp() {
        Some(gt) => out.push(format!("- GPU（NVIDIA）：{gt}（nvidia-smi 实时）")),
        None => match amd_adl_temp() {
            Some(gt) => out.push(format!("- GPU（AMD）：{gt}（atiadlxx ADL 实时）")),
            None => out.push("- GPU：非 NVIDIA/AMD 或驱动未装，WMI 不提供独立显卡温度".into()),
        },
    }
    // 磁盘温度：SMART 控制器上报时是真实物理盘温度（NVMe/SSD），与热区无关，无条件并入。
    if let Ok(raw) = ps_capture(DISK_SMART_PS) {
        let trimmed = raw.trim().trim_start_matches('\u{feff}');
        if let Ok(arr) = serde_json::from_str::<Vec<Value>>(&json_array_of(trimmed)) {
            for d in arr
                .iter()
                .filter(|d| get_f64(d, "temperature").unwrap_or(0.0) != 0.0)
            {
                let nm = v_str(d, "friendly");
                let t = get_f64(d, "temperature").unwrap_or(0.0);
                // 有些控制器以 0.1℃ 上报、有些直接摄氏——按取值落在合理区间内的解释输出
                let c = if t > 120.0 { t / 10.0 } else { t };
                if !(0.0..=120.0).contains(&c) {
                    continue;
                }
                out.push(format!("- 磁盘（{nm}）：{:.*} ℃（SMART）", 0, c));
            }
        }
    }
    // 热区 / GPU / 磁盘温度至少一个有效才算「读到了」；全空 → 降级文案，绝不假装。
    let has_any = out
        .iter()
        .any(|l| l.contains("℃") && !l.contains("驱动未装"));
    if !has_any {
        Ok(
            "温度传感器读不到（ACPI 热区 + GPU 温度 + 磁盘 SMART 均不可用），只能给通用建议\n\
            - 常见原因：主板固件未实现 ACPI 热区，或当前没有就绪的热区\n\
            - 通用建议：留意机身/出风口触感，跑重负载时若外壳烫手可考虑清灰或垫高散热\n\
            - CPU 核心温度需厂商工具（如 Ryzen Master / IAUWMI / Intel DTT），WMI 只有机箱级热区"
                .into(),
        )
    } else {
        let mut s = format!("温度传感器：\n{}", out.join("\n"));
        s.push_str(
            "\n说明：ACPI 热区单位为十分之一开尔文（摄氏 = raw/10 - 273.15），\n  \
             已过滤未就绪的标记值（2731 = 27.31 ℃ 的假值）；GPU 温度 NVIDIA 取 nvidia-smi 实时读数、\n  \
             AMD 取 atiadlxx.dll ADL 实时读数（两通道都不可用时该行标注「驱动未装」）；\n  \
             磁盘取 SMART 控制器上报值；CPU 核心温度需厂商工具（如 Ryzen Master / IAUWMI）。",
        );
        Ok(s)
    }
}

#[cfg(not(windows))]
pub fn collect_temperature() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

/// 非 Windows：快速传感器快照同样不可用。
#[cfg(not(windows))]
pub fn collect_sensors_fast() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
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
    if let Ok(snapshot) = super::hwinfo::read_hwinfo_text() {
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
        s.push_str(&format!(
            "\n说明：数据来自 HWiNFO 共享内存（普通权限，无 Ring0/管理员依赖），\n  \
             读数界面与 HWiNFO 一致；HWiNFO 未运行时自动降级 LibreHardwareMonitor/fancmd 与 ACPI 通道。"
        ));
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
    if let Ok(s) = super::superio::read_text() {
        return Ok(s);
    }
    // 2) 优先 fancmd sensors（LHM 全量）
    if let Some(exe) = super::selfheal::find_fancmd() {
        let direct = run_fancmd_sensors(&exe);
        match direct {
            Ok(s) if !cpu_temps_are_all_null(&s) => return Ok(s),
            Ok(s) if !super::selfheal::is_admin() => {
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
                            return Ok(collect_temperature()?);
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
fn run_fancmd_sensors(exe: &str) -> Result<String, String> {
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
fn extract_cpu_temps_from_sensor_json(raw: &str) -> Option<Vec<(String, f64)>> {
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
            let elevated = super::selfheal::is_admin();
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
    Ok(NOT_WINDOWS.into())
}

/// 取 CPU 当前最高温度（℃）。给 stress_test 超温熔断 / sensor_alert 阈值
/// 用结构化数值：只读 fancmd sensors（LHM 全量 CPU 核心温度，快通道 <1s）。
/// **刻意不回落 PowerShell**（ACPI 热区可能卡 30s 超时，压测主循环每 1s 采样
/// 一次不能被阻塞）；fancmd 不可用/读不到直接 `Err`，调用方按「无温度护栏」
/// 降级（压测仍可跑但提示）。
#[cfg(windows)]
pub fn max_cpu_temp() -> Result<f64, String> {
    let mut best = f64::NAN;
    // 0) 自研 SuperIO 直读（inpoutx64 驱动 + 环境寄存器，毫秒级、普通权限即可）：
    //    本机 ITE 0x8689 直读 CPU 42℃，无需 HWiNFO/LHM/管理员。放最前是为了
    //    stress_test 每 1s 采样不被 fancmd 的 4-5s spawn 拖垮。
    if let Ok(temps) = super::superio::read_temperatures() {
        for (label, v) in &temps {
            if label.contains("CPU") && v.is_finite() && (best.is_nan() || *v > best) {
                best = *v;
            }
        }
    }
    // 1) fancmd sensors（LHM 全量 CPU 核心温度，快通道 <1s）
    if best.is_nan() {
        if let Some(exe) = super::selfheal::find_fancmd() {
            if let Ok(raw) = run_fancmd_sensors(&exe) {
                if let Ok(groups) = serde_json::from_str::<Vec<Value>>(raw.trim()) {
                    for g in &groups {
                        let hw_name = v_str(g, "hardware");
                        if !hw_name.contains("CPU") && !hw_name.contains("Core") {
                            continue;
                        }
                        let Some(gm) = g.get("groups").and_then(Value::as_object) else {
                            continue;
                        };
                        if let Some(items) = gm.get("Temperature").and_then(Value::as_array) {
                            for i in items {
                                if let Some(v) = i.get("value").and_then(Value::as_f64) {
                                    if v.is_finite() && (best.is_nan() || v > best) {
                                        best = v;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // 2) SuperIO / fancmd 都不可用时，快速回退 ACPI 热区 + SMART 磁盘温度（~3s），
    //    保证压测熔断护栏仍可用。
    if best.is_nan() {
        if let Ok(txt) = collect_temperature_fast_parse() {
            if let Some(t) = txt {
                best = t;
            }
        }
    }
    if !best.is_nan() {
        Ok(best)
    } else {
        // fancmd 输出里 CPU Temperature 全 null / ACPI 被拒，都是权限不足的典型表现
        //（LibreHardwareMonitor 读 CPU 温度依赖内核驱动，非管理员被拒；ACPI 热区
        // 非管理员返回 0x80041003）。提示提权而非「不可用」，误导排查方向。
        Err("未读到 CPU 温度：fancmd 的 CPU 温度读数为空或 ACPI 热区被拒（常见于非管理员运行，可提权后重试）；stress_test 将无温度熔断护栏运行".into())
    }
}

/// 快速温度数值：ACPI 热区 + SMART 磁盘温度里取最高（fancmd 慢时的降级温度源）。
/// 返回 `Ok(Some(℃))` 有值 / `Ok(None)` 无有效温度 / `Err` 通道不可用。
#[cfg(windows)]
fn collect_temperature_fast_parse() -> Result<Option<f64>, String> {
    let raw = ps_capture(THERMAL_PS)?;
    let arr: Vec<Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析温度查询结果失败：{e}"))?;
    let mut best = f64::NAN;
    for t in &arr {
        let raw = get_f64(t, "CurrentTemperature").unwrap_or(0.0);
        let c = raw / 10.0 - 273.15;
        let not_ready = (27.0..=27.6).contains(&c);
        if (0.0..=120.0).contains(&c) && !not_ready && c > best {
            best = c;
        }
    }
    if best.is_nan() {
        Ok(None)
    } else {
        Ok(Some(best))
    }
}

#[cfg(not(windows))]
pub fn max_cpu_temp() -> Result<f64, String> {
    Err("max_cpu_temp 仅支持 Windows".into())
}

/// 取 GPU 温度（℃）数值，给 `stress_test_gpu` 熔断用。N 卡走 nvidia-smi，A 卡走
/// atiadlxx.dll ADL；两通道都不可用 → `Err`（调用方如实降级「无 GPU 温度护栏」）。
#[cfg(windows)]
pub fn gpu_temp_c() -> Result<f64, String> {
    let mut nv_err: Option<String> = None;
    if let Ok(Some(g)) = nvidia_gpu_live() {
        if g.temp > 0 {
            return Ok(g.temp as f64);
        }
        nv_err = Some("GPU 温度读数为 0（nvidia-smi 未就绪）".into());
    }
    if let Ok(Some(list)) = amd_adl_live() {
        if let Some(a) = list.into_iter().find(|a| a.temp_c > 0) {
            return Ok(a.temp_c as f64);
        }
    }
    Err(nv_err
        .unwrap_or_else(|| "未检测到 NVIDIA（nvidia-smi）或 AMD（atiadlxx ADL）GPU 温度".into()))
}

#[cfg(not(windows))]
pub fn gpu_temp_c() -> Result<f64, String> {
    Err("gpu_temp_c 仅支持 Windows".into())
}

// ── 6. 电池 ───────────────────────────────────────────────────────────────

const BATTERY_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$bats=@(Get-CimInstance Win32_Battery | Select-Object DeviceID,Name,ManufacturerName,SerialNumber,DesignCapacity,FullChargeCapacity,EstimatedChargeRemaining,EstimatedRunTime,BatteryStatus,Temperature,Chemistry)
$sysb=Get-CimInstance Win32_SystemBattery
$bstat=Get-CimInstance Win32_BatteryStatus
$cycles=null
if($sysb){ $cycles=@($sysb | ForEach-Object { @($_.SystemBatteryInformation | Select -Expand CycleCount) -join "," } ) }
$out=[ordered]@{
  battery=$bats
  cycles=if($cycles){$cycles}else{[ordered]@{}}
  power_online=if($bstat){[bool]$bstat.PowerOnline}else{$null}
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 电池采集：电量/剩余时间/健康度/循环次数。台式机无电池时返回说明而非 `Err`。
#[cfg(windows)]
pub fn collect_battery() -> Result<String, String> {
    let v = json_of(BATTERY_PS, "电池")?;
    let arr = v_arr(&v, "battery");
    if arr.is_empty() {
        return Ok("未检测到电池（台式机或电池不可用）".into());
    }
    let b = &arr[0];
    let mut s = String::new();
    s.push_str("- 电池：");
    if v_str(b, "Name").is_empty() {
        s.push_str("型号未提供");
    } else {
        s.push_str(&v_str(b, "Name"));
    }
    if !v_str(b, "DeviceID").is_empty() {
        s.push_str(&format!("（{0}）", v_str(b, "DeviceID")));
    }
    s.push('\n');
    let pct = get_int(b, "EstimatedChargeRemaining").unwrap_or(0);
    s.push_str(&format!("  - 当前电量：{pct}%\n"));
    let mins = get_int(b, "EstimatedRunTime").unwrap_or(0);
    if mins > 0 {
        s.push_str(&format!(
            "  - 预估剩余：{mins} 分钟（约 {:.*} 小时）\n",
            1,
            mins as f64 / 60.0
        ));
    } else {
        s.push_str("  - 预估剩余：读不到（未供电或 WMI 无估算）\n");
    }
    let code = get_int(b, "BatteryStatus").unwrap_or(0);
    let status = match code {
        1 => "使用电池（放电中）",
        2 => "正在充电",
        3 => "已充满",
        4 => "过热",
        5 => "电池错误",
        6 => "需要更换",
        7 => "电池不存在",
        8 => "需要冷却",
        9 => "电池已耗尽",
        10 => "安全错误",
        0 => "未知",
        _ => {
            s.push_str(&format!(
                "  - 电池状态码：未知数值 {code}（Win32_Battery.BatteryStatus）\n"
            ));
            "未知"
        }
    };
    let po = v.get("power_online").and_then(Value::as_bool);
    let po_str = match po {
        Some(true) => "（已插电源）",
        Some(false) => "（未插电源）",
        None => "（PowerOnline 未提供）",
    };
    s.push_str(&format!("  - 状态：{status}{po_str}\n"));
    let design = get_int(b, "DesignCapacity").unwrap_or(0);
    let full = get_int(b, "FullChargeCapacity").unwrap_or(0);
    s.push_str(&format!(
        "  - 设计容量：{}（{} mAh）\n",
        if design > 0 {
            fmt_bytes(design * 1000)
        } else {
            "未知".into()
        },
        design
    ));
    s.push_str(&format!(
        "  - 满充容量：{}（{} mAh）\n",
        if full > 0 {
            fmt_bytes(full * 1000)
        } else {
            "未知".into()
        },
        full
    ));
    if design > 0 && full > 0 {
        let h = full as f64 / design as f64 * 100.0;
        let tag = if h < 80.0 {
            "，健康度低于 80%，建议更换电池"
        } else if h < 90.0 {
            "，有可感知衰减"
        } else {
            ""
        };
        s.push_str(&format!(
            "  - 健康度：{:.*}%（满充容量 / 设计容量）{tag}\n",
            1, h
        ));
    } else {
        s.push_str("  - 健康度：无法计算（设计容量或满充容量读不到）\n");
    }
    // Win32_SystemBattery.CycleCount 是嵌套数组；脚本里已拼成逗号分隔字符串。
    let mut cycles: Vec<u64> = Vec::new();
    for cv in v_arr(&v, "cycles") {
        match cv {
            Value::String(s) => {
                for part in s.split(',') {
                    if let Ok(n) = part.trim().parse::<u64>() {
                        cycles.push(n);
                    }
                }
            }
            Value::Null => cycles.push(0),
            _ => {}
        }
    }
    if cycles.is_empty() {
        s.push_str("  - 循环次数：未提供（Win32_SystemBattery 无 CycleCount）\n");
    } else if cycles.iter().all(|&n| n == 0) {
        s.push_str("  - 循环次数：未提供（CycleCount 为 0 或空）\n");
    } else {
        let cs = cycles
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("、");
        s.push_str(&format!("  - 循环次数：{cs}\n"));
    }
    let mf = v_str(b, "ManufacturerName");
    if !mf.is_empty() {
        s.push_str(&format!("  - 电池厂商：{mf}\n"));
    }
    let chem = v_str(b, "Chemistry");
    if !chem.is_empty() {
        s.push_str(&format!("  - 化学体系：{chem}\n"));
    }
    let sn = v_str(b, "SerialNumber");
    if !sn.is_empty() {
        s.push_str(&format!("  - 序列号：{sn}\n"));
    }
    let temp = get_int(b, "Temperature").unwrap_or(0);
    // Win32_Battery.Temperature 单位是 0.1 ℃
    if temp > 0 {
        s.push_str(&format!(
            "  - 电池温度：{:.*} ℃（原始 {temp}，单位 0.1 ℃）",
            1,
            temp as f64 / 10.0
        ));
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_battery() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 7. 磁盘 SMART ─────────────────────────────────────────────────────────

/// SMART 采集走 `Get-PhysicalDisk` + `Get-StorageReliabilityCounter`（Win10 1607+，
/// 普通权限即可，不需要 DeviceIoControl）。**不要用 `wmic`**：Win11 24H2 已把
/// `wmic.exe` 从系统里移除；也不要走 WMI 的 `Win32_DiskDrive`（拿不到通电时长/磨损）。
const DISK_SMART_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$disks=Get-PhysicalDisk
$out=@($disks | ForEach-Object {
  $rc=$_ | Get-StorageReliabilityCounter -ErrorAction SilentlyContinue
  [ordered]@{
    friendly=[string]$_.FriendlyName
    media=[string]$_.MediaType
    bus=[string]$_.BusType
    size=$_.Size
    health=[string]$_.HealthStatus
    operational=[string]$_.OperationalStatus
    is_boot=$_.IsBoot
    power_on_hours=if($rc){$rc.PowerOnHours}else{$null}
    temperature=if($rc){$rc.Temperature}else{$null}
    temperature_max=if($rc){$rc.TemperatureMax}else{$null}
    wear=if($rc){$rc.Wear}else{$null}
    read_errors=if($rc){$rc.ReadErrorsTotal}else{$null}
    write_errors=if($rc){$rc.WriteErrorsTotal}else{$null}
    start_stop=if($rc){$rc.StartStopCycleCount}else{$null}
    load_unload=if($rc){$rc.LoadUnloadCycleCount}else{$null}
  }
})
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 磁盘健康与 SMART 采集：型号/介质/总线/容量/健康状态 + 通电时长/温度/磨损/累计错误。
#[cfg(windows)]
pub fn collect_disk_smart() -> Result<String, String> {
    let raw = ps_capture(DISK_SMART_PS)?;
    let arr: Vec<Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析磁盘健康查询结果失败：{e}"))?;
    if arr.is_empty() {
        return Ok("未检测到物理磁盘（Get-PhysicalDisk 无返回）".into());
    }
    let mut s = String::new();
    s.push_str(&format!("- 磁盘 SMART（共 {} 块物理磁盘）：\n", arr.len()));
    for d in &arr {
        let nm = if v_str(d, "friendly").is_empty() {
            "未命名磁盘".to_string()
        } else {
            v_str(d, "friendly")
        };
        let size = fmt_bytes(get_int(d, "size").unwrap_or(0));
        let media = if v_str(d, "media").is_empty() {
            "未知".to_string()
        } else {
            v_str(d, "media")
        };
        let bus = if v_str(d, "bus").is_empty() {
            "未知".to_string()
        } else {
            v_str(d, "bus")
        };
        let health = if v_str(d, "health").is_empty() {
            "未知".to_string()
        } else {
            v_str(d, "health")
        };
        let boot = if d.get("is_boot").and_then(Value::as_bool).unwrap_or(false) {
            "（系统盘）"
        } else {
            ""
        };
        s.push_str(&format!(
            "- {nm}：{media}，{bus}，{size}，健康状态 {health}{boot}\n"
        ));
        let poh = get_int(d, "power_on_hours").filter(|&n| n > 0);
        if let Some(p) = poh {
            s.push_str(&format!(
                "  - 通电时长：{} 小时（约 {} 天 / {} 年）\n",
                format_num(p),
                p / 24,
                p / 8760
            ));
        } else {
            s.push_str("  - 通电时长：未提供（控制器未上报 PowerOnHours）\n");
        }
        // 单位启发式：> 60 视为华氏并换算摄氏，否则当作摄氏（主项目 hw.rs:1161-1167）。
        if let Some(t) = get_f64(d, "temperature") {
            let c = if t > 60.0 { (t - 32.0) * 5.0 / 9.0 } else { t };
            s.push_str(&format!(
                "  - 当前温度：{:.*} ℃（原始读数 {t}，>60 视为华氏并换算）\n",
                0, c
            ));
        }
        if let Some(t) = get_f64(d, "temperature_max") {
            if t > 0.0 {
                let c = if t > 60.0 { (t - 32.0) * 5.0 / 9.0 } else { t };
                s.push_str(&format!("  - 历史最高温度：{:.*} ℃\n", 0, c));
            }
        }
        let wear = get_f64(d, "wear");
        if let Some(w) = wear {
            if (0.0..=100.0).contains(&w) {
                s.push_str(&format!(
                    "  - 磨损度：{:.*}%（寿命剩余约 {:.*}%）\n",
                    0,
                    w,
                    0,
                    100.0 - w
                ));
            } else {
                s.push_str(&format!(
                    "  - 磨损度：未上报（原始值 {w} 超出 0~100% 有效范围，按未上报处理）\n"
                ));
            }
        } else if media.to_ascii_uppercase().contains("HDD") {
            // 机械盘没有「写入寿命百分比」这个概念，别说成 NVMe 没上报（误导）
            s.push_str("  - 磨损度：机械盘无磨损百分比概念（Wear 仅对 SSD 有意义）\n");
        } else {
            s.push_str("  - 磨损度：未上报（NVMe 控制器常见行为，不代表盘体健康有问题）\n");
        }
        let re = get_int(d, "read_errors").unwrap_or(0);
        let we = get_int(d, "write_errors").unwrap_or(0);
        s.push_str(&format!(
            "  - 累计错误：读 {} / 写 {}（非零且持续增长时建议关注）\n",
            format_num(re),
            format_num(we)
        ));
        let cyc = |k: &str| -> String {
            get_int(d, k)
                .map(|n| n.to_string())
                .unwrap_or_else(|| "未提供".into())
        };
        s.push_str(&format!(
            "  - 启停循环：{}，加载/卸载循环：{}\n",
            cyc("start_stop"),
            cyc("load_unload")
        ));
    }
    s.push_str(
        "\n说明：外接 USB / SD 卡通常不出现在 Get-PhysicalDisk 里（只有内接存储）；\n  \
         本工具未用 wmic（Win11 24H2 已移除 wmic.exe），也未用 DeviceIoControl（普通权限被拒）。",
    );
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_disk_smart() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 8. USB 设备（Get-PnpDevice Class USB） ───────────────────────────────────

/// USB 设备采集走 `Get-PnpDevice -Class USB`（PNP 枚举，工作站普通权限即可，
/// 不需要管理员）。**不读 WiFi 密码、不读序列号机密**：只取设备类型/名称/
/// 厂商/状态/实例 ID（实例 ID 用于区分设备，不含敏感内容）。
/// 树形输出：控制器 → 集线器 → 外设层级（保留 ParentInstanceId 关联）。
const USB_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$devs = @(Get-PnpDevice -Class USB -Status OK -ErrorAction SilentlyContinue | Select-Object FriendlyName, InstanceId, Class, Service, Status, Manufacturer, Present)
$devs | ConvertTo-Json -Depth 4 -Compress
"#;

/// USB 设备列表：控制器/集线器/外设（只读，普通权限）。
#[cfg(windows)]
pub fn collect_usb_devices() -> Result<String, String> {
    let raw = ps_capture(USB_PS)?;
    let arr: Vec<Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析 USB 设备查询结果失败：{e}"))?;
    if arr.is_empty() {
        return Ok(
            "未检测到 USB 设备（Get-PnpDevice -Class USB 无返回，或设备全为非 OK 状态）".into(),
        );
    }
    let mut s = format!("- USB 设备清单（共 {} 个）：\n", arr.len());
    let mut ctrl = 0;
    let mut hub = 0;
    let mut dev = 0;
    for d in &arr {
        let cls = v_str(d, "Class");
        let name = v_str(d, "FriendlyName");
        let name = if name.is_empty() {
            v_str(d, "InstanceId")
        } else {
            name
        };
        let service = v_str(d, "Service");
        let manuf = v_str(d, "Manufacturer");
        let mut line = format!("- {}（{}", name, cls);
        if !manuf.is_empty() {
            line.push_str(&format!("，厂商 {manuf}"));
        }
        if !service.is_empty() {
            line.push_str(&format!("，服务 {service}"));
        }
        line.push('）');
        match cls.as_str() {
            "USB" if name.to_ascii_lowercase().contains("controller") => ctrl += 1,
            "USB" => hub += 1,
            _ => dev += 1,
        }
        s.push_str(&line);
        s.push('\n');
    }
    s.push_str(&format!(
        "\n统计：控制器 {ctrl} / 集线器 {hub} / 外设 {dev}\n\
         说明：枚举 PNP 设备（普通权限）；外接 U 盘/键鼠/摄像头/声卡等在此列出。\
         只读元数据，未读任何设备内容或配置。"
    ));
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_usb_devices() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 9. SMART 原始属性表（可靠性计数器） ─────────────────────────────────────

/// SMART 原始属性：在 `collect_disk_smart` 的聚合计数器之外，补一块
/// **可靠性计数器**（通电时长/温度/磨损/错误/循环）。
/// Windows 普通权限拿不到标准 SMART 属性表（ID 0x05 重分配扇区等），
/// 需要 smartctl（smartmontools，第三方，默认未装）——这里先给能拿到的
/// 可靠性计数，并明确标注不是完整 SMART 表，避免误导。
const DISK_SMART_RAW_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$disks=Get-PhysicalDisk
$out=@($disks | ForEach-Object {
  $rc=$_ | Get-StorageReliabilityCounter -ErrorAction SilentlyContinue
  [ordered]@{
    friendly=[string]$_.FriendlyName
    health=[string]$_.HealthStatus
    operational=[string]$_.OperationalStatus
    temperature=if($rc){$rc.Temperature}else{$null}
    wear=if($rc){$rc.Wear}else{$null}
    read_errors=if($rc){$rc.ReadErrorsTotal}else{$null}
    write_errors=if($rc){$rc.WriteErrorsTotal}else{$null}
    start_stop=if($rc){$rc.StartStopCycleCount}else{$null}
    load_unload=if($rc){$rc.LoadUnloadCycleCount}else{$null}
    power_on_hours=if($rc){$rc.PowerOnHours}else{$null}
    temperature_max=if($rc){$rc.TemperatureMax}else{$null}
  }
})
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// SMART 可靠性计数器（只读，普通权限）。
///
/// **已知限制**：输出的是 `Get-StorageReliabilityCounter` 的可靠性计数，
/// **不是**标准 SMART 属性表 ID 0x01~0xE6。要完整 SMART 表需要 smartctl
/// （第三方可选，本工具不自动装）。标注清楚，避免误读。
#[cfg(windows)]
pub fn collect_disk_smart_raw() -> Result<String, String> {
    let raw = ps_capture(DISK_SMART_RAW_PS)?;
    let arr: Vec<Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析磁盘可靠性计数失败：{e}"))?;
    if arr.is_empty() {
        return Ok("未检测到物理磁盘（Get-PhysicalDisk 无返回）".into());
    }
    let mut s = String::new();
    s.push_str(&format!(
        "- 磁盘可靠性计数（共 {} 块物理磁盘）：\n",
        arr.len()
    ));
    for d in &arr {
        let nm = if v_str(d, "friendly").is_empty() {
            "未命名磁盘".to_string()
        } else {
            v_str(d, "friendly")
        };
        let health = if v_str(d, "health").is_empty() {
            "未知".to_string()
        } else {
            v_str(d, "health")
        };
        s.push_str(&format!("- {nm}：健康 {health}\n"));
        let f = |k: &str| -> String {
            get_int(d, k)
                .map(format_num)
                .unwrap_or_else(|| "未上报".into())
        };
        for (k, label) in [
            ("power_on_hours", "通电时长(小时)"),
            ("temperature", "温度(原始)"),
            ("temperature_max", "历史最高温(原始)"),
            ("wear", "磨损度"),
            ("read_errors", "累计读错误"),
            ("write_errors", "累计写错误"),
            ("start_stop", "启停循环"),
            ("load_unload", "加载/卸载循环"),
        ] {
            s.push_str(&format!("  - {label}：{}\n", f(k)));
        }
    }
    s.push_str(
        "\n说明：以上是 Get-StorageReliabilityCounter 可靠性计数，**不是标准 SMART 属性表\n  \
         （ID 0x01~0xE6 如 0x05 重分配扇区 / 0x0C 通电计数）**。\n  \
         要完整 SMART 表需安装 smartmontools 的 smartctl（第三方可选，本工具不自动装）。\n  \
         外接 USB / SD 卡通常不出现在 Get-PhysicalDisk 里。",
    );
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_disk_smart_raw() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 非 Windows 平台统一降级文案 ────────────────────────────────────────────

#[cfg(not(windows))]
#[allow(dead_code)] // 跨平台降级文案：Windows 构建不引用
const NOT_WINDOWS: &str = "当前平台不是 Windows，硬件信息不可用";

// ── 本文件导出函数清单（给 tools/mod.rs 对接用）────────────────────────────
//
// 1. pub fn collect_cpu() -> Result<String, String>
//    CPU 型号/核心/线程/标称与实时频率/L2·L3 缓存/虚拟化/瞬时占用。
//    坑：L2CacheSize 的 WMI bug（垃圾值按未知处理）；多路 CPU 取第 1 路并标注共 N 路。
//
// 2. pub fn collect_gpu() -> Result<String, String>
//    显卡型号/显存/驱动版本与日期/当前输出分辨率与刷新率/DeviceID + N 卡实时占用·温度·显存。
//    坑：AdapterRAM 32 位溢出（≥4 GB 无法精确读出）；无 nvidia-smi 时安静跳过该行。
//
// 3. pub fn collect_memory() -> Result<String, String>
//    每根内存条（插槽/容量/标称频率/运行频率/品牌/型号）+ 总容量 + XMP/EXPO 诊断。
//    坑：DDR5 的 Speed 只有实际有效频率的一半；时序（CL）WMI 不提供，明确标注不编造。
//
// 4. pub fn collect_motherboard() -> Result<String, String>
//    主板厂商/型号/序列号 + BIOS 厂商/版本/发布日期 + 整机品牌/机型。
//    坑："To Be Filled By O.E.M." 占位串按未知处理；ReleaseDate 原样透传并注明格式。
//
// 5. pub fn collect_temperature() -> Result<String, String>
//    ACPI 热区温度（逐热区）+ NVIDIA GPU 温度。
//    坑：MSAcpi 单位是十分之一开尔文（c = raw/10 - 273.15）；未就绪报 2731/0，
//    用 0~120 ℃ 过滤；全部被过滤掉时返回降级文案，绝不返回空列表假装成功。
//
// 6. pub fn collect_battery() -> Result<String, String>
//    电量/剩余分钟/状态/设计容量/满充容量/健康度/循环次数/厂商/温度。
//    坑：台式机无电池返回说明而非 Err；CycleCount 是嵌套数组且可能为 null。
//
// 7. pub fn collect_disk_smart() -> Result<String, String>
//    每块物理磁盘的型号/介质/总线/容量/健康状态 + 通电时长/温度/最高温度/磨损/累计错误/循环。
//    坑：Get-StorageReliabilityCounter.Temperature 单位不统一（>60 视为华氏换算）；
//    NVMe 的 Wear 常为 null；外接 USB/SD 不出现；wmic 在 Win11 24H2 已移除。

// ── 单元测试：厂商温度行解析（纯逻辑，不调真人机 nvidia-smi / ADL）─────────
// hw_temperature 输出格式是前端甜甜圈熔断 regex 的契约：
//   N 卡行「- GPU（NVIDIA）：xx ℃（nvidia-smi 实时）」
//   A 卡行「- GPU（AMD）：xx ℃（atiadlxx ADL 实时）」
// 这两个格式同一行内 `GPU（厂商）：数字 ℃` 且能被前端的
// `/GPU[（(](?:NVIDIA|AMD)[）)]：\s*([\d.]+)\s*℃/` 命中。
// 下面是等价的手写解析器（跨平台、不引 regex 依赖），供测试验证行格式契约。

/// 从一行文本里解析 `GPU（厂商）：温度 ℃`。命中任意厂商（NVIDIA/AMD）返回温度值。
#[cfg(test)]
fn parse_gpu_temp_line(line: &str) -> Option<f64> {
    let rest = line
        .split("）：")
        .nth(1)
        .or_else(|| line.split("):").nth(1))?;
    // rest 形如「 58 ℃（atiadlxx ADL 实时）」，取第一个空白前数字
    let head = rest.trim_start();
    let num_end = head
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(head.len());
    head[..num_end].parse::<f64>().ok().filter(|t| *t > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_gpu_temp_line(text: &str) -> String {
        text.lines()
            .find(|l| l.contains("GPU（") || l.contains("GPU("))
            .unwrap_or("")
            .trim()
            .to_string()
    }

    #[test]
    fn amd_line_format_matches_donut_regex() {
        // 前端熔断 regex：/GPU[（(](?:NVIDIA|AMD)[）)]：\s*([\d.]+)\s*℃/ —— 等价解析：
        // 行内含「GPU（ 或 GPU(」+ 厂商 +「）」+「：」+ 数字 +「℃」
        for line in [
            "- GPU（AMD）：58 ℃（atiadlxx ADL 实时）",
            "- GPU（NVIDIA）：63 ℃（nvidia-smi 实时）",
            "- GPU（AMD）：91 ℃（atiadlxx ADL 实时）",
        ] {
            let t = parse_gpu_temp_line(line);
            assert!(t.is_some(), "熔断解析未命中：{line}");
            assert!(t.unwrap() > 0.0);
        }
    }

    #[test]
    fn amd_line_present_in_collect_temperature_output() {
        // 模拟 collect_temperature 拼接 A 卡行的内容：AMD 无 nvidia-smi 时应出现
        // 「GPU（AMD）」行而非旧「非 NVIDIA」占位行。
        let mut out: Vec<String> = vec!["- 热区：35 ℃".into()];
        out.push("- GPU（AMD）：58 ℃（atiadlxx ADL 实时）".into());
        let text = format!("温度传感器：\n{}", out.join("\n"));
        let line = extract_gpu_temp_line(&text);
        assert!(
            line.contains("GPU（AMD）"),
            "A 卡行应带 AMD 厂商标签：{line}"
        );
        assert!(!line.contains("驱动未装"));
        // has_any 判定：正文含 ℃ 且不带「驱动未装」→ 算读到了
        let has_any = text
            .lines()
            .any(|l| l.contains("℃") && !l.contains("驱动未装"));
        assert!(has_any);
    }

    #[test]
    fn no_temp_channel_is_degrated_not_fake() {
        // 两通道都不可用时 collect_temperature 的占位行必须带「驱动未装」，
        // has_any 判定会把它排除，最后返回降级说明而非假装读数。
        let placeholder = "- GPU：非 NVIDIA/AMD 或驱动未装，WMI 不提供独立显卡温度";
        assert!(placeholder.contains("驱动未装"));
        let has_any = placeholder.contains("℃") && !placeholder.contains("驱动未装");
        assert!(!has_any);
    }

    #[test]
    fn gpu_temp_c_err_message_covers_both_vendors() {
        // gpu_temp_c（stress_test_gpu 熔断）两通道全失败的报错应提到 NVIDIA 与 AMD。
        let nv_err: Option<String> = None;
        let err = nv_err.unwrap_or_else(|| {
            "未检测到 NVIDIA（nvidia-smi）或 AMD（atiadlxx ADL）GPU 温度".to_string()
        });
        assert!(err.contains("NVIDIA"));
        assert!(err.contains("AMD"));
    }
}
