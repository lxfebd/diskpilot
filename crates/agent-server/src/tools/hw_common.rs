//! 硬件采集公共底座：JSON 取值工具 + GPU 实时通道（nvidia-smi / ADL）+ 非 Windows 降级文案。
//!
//! 从 `hw.rs` 拆分（2026-10-12）：被所有 hw_* 子模块共享，符号全部 `pub(crate)`。
// json_array_of / ps_capture / Value 由跨平台编译的 json_of / json_arr_of 引用
//（这两个 fn 在 Linux 下虽无调用方，仍编译通过），保持裸 use。
use crate::ps::{json_array_of, ps_capture};
use serde_json::Value;
// ADL_PROBE_PS / v_str 仅供 cfg(windows) 的 AMD ADL 通道使用，随 Windows 编译。
#[cfg(windows)]
use crate::ps::ADL_PROBE_PS;
#[cfg(windows)]
use crate::tools::v_str;
// Read 仅供 cfg(windows) 的 nvidia-smi 调用使用。
#[cfg(windows)]
use std::io::Read;

// ── JSON 取值小工具（与主项目 hw.rs 的 v_str / arr 同语义）─────────────────
// v_str 已在 tools/mod.rs 共享，此处不再复刻。

/// 取数组字段；缺失返回空 Vec（配合下面的 `arr().iter()` 直接用）。
/// 消费方均为 cfg(windows) 采集实现，Linux 下无调用方 → 豁免 dead_code。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn v_arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(Value::as_array).map_or(&[], |a| a)
}

/// 可选整型：`null` / 缺失 / 非数字 → `None`。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn get_int(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(Value::as_u64)
}

/// 可选浮点：`null` / 缺失 / 非数字 → `None`。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn get_f64(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

/// 温度是否真实可用：落在 `0..=120℃` 合理区间，且排除 MSACPI 未就绪的
/// 标记值 2731（= 27.31 ℃ 的假值，恰好落入合理区间，须单独排除）。
/// 五条温度采集路径共用同一判定，收敛到这里。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn thermal_ready(c: f64) -> bool {
    (0.0..=120.0).contains(&c) && !(27.0..=27.6).contains(&c)
}

/// 大整数千分位（`1,234,567`），避免累计错误数挤成一团。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn format_num(n: u64) -> String {
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
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn json_of(script: &str, what: &str) -> Result<Value, String> {
    let raw = ps_capture(script)?;
    let raw = raw.trim().trim_start_matches('\u{feff}');
    serde_json::from_str(raw).map_err(|e| format!("解析{what}查询结果失败：{e}"))
}

/// 跑一段输出数组的脚本并解析成数组（`ConvertTo-Json` 单项折叠成对象的坑在此兜底）。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn json_arr_of(script: &str, what: &str) -> Result<Vec<Value>, String> {
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
/// WMI 日期字段序列化后是 COM 日期串 `/Date(1668470400000)/`（毫秒时间戳），
/// 直接透传没人看得懂；能解析就转成 `yyyy-MM-dd`，失败就原样返回。
///
/// `Win32_BIOS.ReleaseDate` 在不同系统上可能是 COM 日期串、也可能是
/// `20200923121500.000+080` 这种数字串；后者只做「前 8 位日期」的安全解析，
/// 解析不了就原样透传，绝不猜。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn fmt_wmi_date(s: &str) -> String {
    let t = s.trim();
    if let Some(inner) = t.strip_prefix("/Date(").and_then(|x| x.strip_suffix(")/")) {
        // PowerShell ConvertTo-Json 有两种形态：`/Date(1150848000000)/` 和
        // `/Date(1150848000000+0800)/`（带时区偏移）。以前只认前者，带偏移的
        // 直接原样透传，界面上就出现 `/Date(...)/` 这种没人看得懂的串。
        let head: String = inner
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        if let Ok(ms) = head.parse::<i64>() {
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
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn com_date_to_iso(ms: i64) -> String {
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
pub(crate) struct NvidiaGpuLive {
    pub(crate) util: u32,
    pub(crate) temp: u32,
    pub(crate) mem_used: Option<u64>,
    pub(crate) mem_total: u64,
}

/// 取 GPU 温度（℃），给 `collect_temperature` 复用。无 N 卡 / 无 nvidia-smi → `None`
/// （正常情况，不算错误）。
#[cfg(windows)]
pub(crate) fn nvidia_gpu_temp() -> Option<String> {
    nvidia_gpu_live()
        .ok()
        .flatten()
        .map(|g| format!("{} ℃", g.temp))
}

/// 取物理 N 卡型号名（`nvidia-smi --query-gpu=name`）。失败/无卡 → `None`。
#[cfg(windows)]
pub(crate) fn nvidia_gpu_name() -> Option<String> {
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
pub(crate) fn nvidia_gpu_live() -> Result<Option<NvidiaGpuLive>, String> {
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

/// AMD 卡实时指标（ADL 通道）。DLL 缺失/初始化失败/无 A 卡 → `Ok(Some(vec))` 可能为空或
/// `Ok(None)`（正常情况，不算错误）；PowerShell 子进程崩了 → ps_capture Err → 外层吞 None。
#[cfg(windows)]
pub(crate) struct AmdAdlLive {
    pub(crate) index: u32,
    pub(crate) temp_c: u32,
    pub(crate) gpu_util: u32,
}

#[cfg(windows)]
pub(crate) fn amd_adl_live() -> Result<Option<Vec<AmdAdlLive>>, String> {
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
pub(crate) fn amd_adl_temp() -> Option<String> {
    amd_adl_live()
        .ok()
        .flatten()
        .and_then(|l| l.into_iter().find(|a| a.temp_c > 0))
        .map(|a| format!("{} ℃", a.temp_c))
}

/// 跨平台降级文案：引用它的都是 `#[cfg(not(windows))]` 分支，Windows 下无人调用。
#[allow(dead_code)]
pub(crate) const NOT_WINDOWS: &str = "当前平台不是 Windows，硬件信息不可用";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wmi_date_com_form_and_offset_form_agree() {
        // PowerShell ConvertTo-Json 的两种形态必须给出同一个日期：带 +0800 的那种
        // 以前解析不了，界面上就露出 /Date(...)/ 原文。
        let a = fmt_wmi_date("/Date(1150848000000)/");
        let b = fmt_wmi_date("/Date(1150848000000+0800)/");
        assert_eq!(a, b);
        assert!(!a.contains("/Date("), "解析失败会露出原文：{a}");
        assert!(a.starts_with("2006-06-"), "实测该时间戳是 2006-06-21：{a}");
    }

    #[test]
    fn wmi_date_epoch_is_exact() {
        assert_eq!(fmt_wmi_date("/Date(0)/"), "1970-01-01");
        assert_eq!(fmt_wmi_date("/Date(86400000)/"), "1970-01-02");
    }

    #[test]
    fn wmi_date_cim_numeric_form_and_garbage_passthrough() {
        let s = fmt_wmi_date("20260410000000.000000-000");
        assert!(s.starts_with("2026-04-10"), "{s}");
        // 认不出来的绝不猜，原样透传。
        assert_eq!(fmt_wmi_date("/Date(abc)/"), "/Date(abc)/");
        assert_eq!(fmt_wmi_date(""), "");
    }
}
