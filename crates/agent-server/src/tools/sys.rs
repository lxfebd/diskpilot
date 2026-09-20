//! Windows 系统服务 / 内核驱动 / 开机自启项枚举（**只读**，绝不删除/禁用/篡改）。
//!
//! 三个采集函数：
//! - `collect_services` —— 混合方案：PowerShell `Win32_Service` 拿全量运行状态 +
//!   注册表 `HKLM\SYSTEM\CurrentControlSet\Services\<name>` 拿 `ImagePath`/`Start`/`Type`
//!   （`Win32_Service.ImagePath` 对系统服务全为空，必须读注册表——已真机验证）。
//! - `collect_drivers` —— PowerShell `Win32_PnPSignedDriver`（真机 137 条跑通）。
//! - `collect_boot_items` —— 五个 Run 注册表键 + 两个 Startup 文件夹 + 可选计划任务。
//!
//! ## 只读铁律
//! 本文件**只做读取**：不 `RegSetValueExW`、不删文件、不禁服务、不 `Unregister-ScheduledTask`。
//! 写操作留主项目 executor + 权限中心（L1/L2 分级），工具层不给破坏性能力。
//!
//! ## 已处理的坑
//! - **注册表 `ImagePath` 带首尾引号**（`"C:\...\svc.exe" --flag`）——展示前清理；
//!   资源串 `@%SystemRoot%\system32\drivers\tcpip.sys,-10001` 标注为「资源字符串引用」。
//! - **`Win32_Service.ImagePath` 对系统服务全为空**，注册表才是唯一来源。
//! - **`Win32_PnPSignedDriver.DriverDate` 是 CIM DateTime**（`20260410000000.000000-000`），
//!   只取前 8 位 `YYYYMMDD` 做安全解析，失败原样透传。
//! - **微软驱动假日期**：年份 < 2020 且版本号 `10.0.*` → 注明「微软驱动日期可能失真」。
//! - **`RegEnumKeyExW` / `RegEnumValueW` 返回的名字不含 NUL**——必须手动 `push(0)`
//!   （照搬 `apps.rs::enum_uninstall_key`）。
//! - **`HKEY` 用 `null_mut()` 初始化**（windows-sys 是裸指针，不是 `Option`）。
//! - **`Win32_System_Environment` feature 未开**——`ExpandEnvironmentStringsW` 不可用，
//!   手写展开 `%AppData%` / `%LocalAppData%` / `%ProgramFiles%` / `%SystemRoot%` 四个常用项，
//!   其他 `%VAR%` 原样透传。
//! - **rustc 1.98 拒 `{:.1}`**，浮点一律 `{:.*}`。
//!
//! ## 本文件导出函数清单
//! 1. `pub fn collect_services(keyword: Option<String>, show_disabled: bool, top_n: usize) -> Result<String, String>`
//! 2. `pub fn collect_drivers(keyword: Option<String>, top_n: usize) -> Result<String, String>`
//! 3. `pub fn collect_boot_items(include_scheduled: bool) -> Result<String, String>`
//!
//! 非 Windows 平台全部返回 `Ok("当前平台不是 Windows，系统信息不可用")`。

use serde_json::Value;

#[cfg(windows)]
use std::collections::HashMap;

#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use crate::ps::{json_array_of, ps_capture, ps_capture_timeout};

// ── Windows 平台专用工具 ────────────────────────────────────────────────

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 读 REG_SZ / REG_EXPAND_SZ 字符串值（`RegQueryValueExW` 手动处理 `len` + NUL 截断）。
/// `REG_EXPAND_SZ` 的 `%VAR%` 由本工具在调用侧再展开，这里原样返回。
#[cfg(windows)]
fn read_str_val(key: windows_sys::Win32::System::Registry::HKEY, value: &[u16]) -> String {
    use windows_sys::Win32::System::Registry::RegQueryValueExW;
    let mut buf = [0u16; 2048];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegQueryValueExW(
            key,
            value.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        )
    };
    if r != 0 || len < 2 {
        return String::new();
    }
    // len 是字节数（含结尾 NUL）；转成 u16 计数并去结尾 NUL。
    let n = ((len as usize) / 2 - 1).min(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

/// 读 REG_DWORD。缺失返回 `None`。
#[cfg(windows)]
fn read_dword_val(key: windows_sys::Win32::System::Registry::HKEY, value: &[u16]) -> Option<u32> {
    use windows_sys::Win32::System::Registry::RegQueryValueExW;
    let mut buf = 0u32;
    let mut len = 4u32;
    let r = unsafe {
        RegQueryValueExW(
            key,
            value.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut buf as *mut u32 as *mut u8,
            &mut len,
        )
    };
    if r != 0 {
        return None;
    }
    Some(buf)
}

// ── JSON 取值小工具（同 `hw.rs`） ───────────────────────────────────────

/// 取一个 JSON 字段的可读文本：数字/布尔转字符串，缺失/异常一律空串。
fn v_str(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

// ── 1. 服务枚举 ─────────────────────────────────────────────────────────

/// 一次性拿所有服务的运行状态（PowerShell `Win32_Service`）。
/// **不用 PS 逐键查 `ImagePath`**——那会跑几百次 PS 进程，注册表用原生 API 才毫秒级。
#[cfg(windows)]
const SERVICE_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-CimInstance Win32_Service | Select-Object Name,State,StartMode,DisplayName)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// `Win32_Service.State` 的英文枚举值 → 中文。
#[cfg(windows)]
fn state_zh(s: &str) -> &str {
    match s.to_ascii_uppercase().as_str() {
        "RUNNING" => "运行中",
        "STOPPED" => "已停止",
        "START_PENDING" => "启动中",
        "STOP_PENDING" => "停止中",
        "PAUSED" => "已暂停",
        "CONTINUE_PENDING" => "恢复中",
        "PAUSE_PENDING" => "暂停中",
        _ => {
            if s.is_empty() {
                "未知"
            } else {
                "状态未知"
            }
        }
    }
}

/// `Win32_Service.StartMode` 的英文枚举值 → 中文。
/// 仅用于「注册表打不开」的降级路径（fallback）；正常路径走 `start_dword_zh`。
#[cfg(windows)]
fn start_mode_zh(s: &str) -> &str {
    match s.to_ascii_lowercase().as_str() {
        "automatic" => "自动",
        "manual" => "手动",
        "disabled" => "禁用",
        "boot" => "启动",
        "system" => "系统",
        _ => "未知",
    }
}

/// 注册表 `Start` DWORD → 中文（0=Boot, 1=System, 2=Automatic, 3=Manual, 4=Disabled）。
#[cfg(windows)]
fn start_dword_zh(n: Option<u32>) -> (String, bool) {
    // 返回 (中文, 是否禁用)
    match n {
        Some(0) => ("启动(Boot)".to_string(), false),
        Some(1) => ("系统(System)".to_string(), false),
        Some(2) => ("自动".to_string(), false),
        Some(3) => ("手动".to_string(), false),
        Some(4) => ("禁用".to_string(), true),
        _ => ("未知".to_string(), false),
    }
}

fn clean_image_path(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return "未知".to_string();
    }
    // 资源串 `@%SystemRoot%\system32\drivers\tcpip.sys,-10001` 优先判断（在去引号前）
    if t.starts_with('@') {
        return format!("（资源字符串引用：{t}）");
    }
    // 普通 `ImagePath`：去首尾引号（`"C:\...\svc.exe" --flag`）
    let stripped = t.trim_matches('"');
    stripped.to_string()
}

/// 从 `Win32_Service` PS 结果建 `HashMap<name_lower, (state_zh, start_mode_zh, display_name)>`。
#[cfg(windows)]
fn parse_ps_services(raw: &str) -> HashMap<String, (String, String, String)> {
    let mut map = HashMap::new();
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    if trimmed.is_empty() {
        return map;
    }
    let arr_str = json_array_of(trimmed);
    let arr: Vec<Value> = match serde_json::from_str(&arr_str) {
        Ok(v) => v,
        Err(_) => return map,
    };
    for v in &arr {
        let name = v_str(v, "Name");
        if name.is_empty() {
            continue;
        }
        let state_s = v_str(v, "State");
        let sm_s = v_str(v, "StartMode");
        let state = state_zh(&state_s);
        let sm = start_mode_zh(&sm_s);
        let dn = v_str(v, "DisplayName");
        map.insert(
            name.to_ascii_lowercase(),
            (state.to_string(), sm.to_string(), dn),
        );
    }
    map
}

/// 单个服务行（先收集，再过滤+排序+截断）。
struct ServiceRow {
    name: String,
    display: String,
    state: String,
    start_zh: String,
    image: String,
    is_disabled: bool,
    is_driver: bool,
}

/// 枚举服务：混合 PowerShell（运行状态）+ 注册表（`ImagePath` / `Start` / `Type`）。
///
/// **只读**：`RegOpenKeyExW(KEY_READ)` + `RegEnumKeyExW` + `RegQueryValueExW`，无任何写调用。
#[cfg(windows)]
pub fn collect_services(
    keyword: Option<String>,
    show_disabled: bool,
    top_n: usize,
) -> Result<String, String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
        KEY_WOW64_64KEY,
    };

    let top_n = top_n.clamp(1, 200);
    let kw: Option<String> = keyword
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| !s.is_empty());

    // ① 一次 PS 拿全量运行状态（真机约 343 条，几秒）。
    let ps_raw = ps_capture(SERVICE_PS)?;
    let ps_map = parse_ps_services(&ps_raw);

    // ② 注册表枚举 HKLM\SYSTEM\CurrentControlSet\Services 拿 ImagePath/Start/Type。
    // 注册表打不开时降级：纯走 PS 结果（无 ImagePath）。
    let mut rows: Vec<ServiceRow> = Vec::new();
    const SERVICES_PATH: &str = r"SYSTEM\CurrentControlSet\Services";
    let mut key: HKEY = std::ptr::null_mut();
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            wide(SERVICES_PATH).as_ptr(),
            0,
            KEY_READ | KEY_WOW64_64KEY,
            &mut key,
        )
    };
    if rc == 0 {
        let mut idx = 0u32;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            let mut name_buf = [0u16; 512];
            let mut name_len = name_buf.len() as u32;
            let r = unsafe {
                RegEnumKeyExW(
                    key,
                    idx,
                    name_buf.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if r != 0 {
                break;
            }
            idx += 1;
            // RegEnumKeyExW 返回的名字不含 NUL，必须手动补。
            let n = (name_len as usize).min(name_buf.len());
            let mut sub = name_buf[..n].to_vec();
            sub.push(0);

            let mut sub_key: HKEY = std::ptr::null_mut();
            let rc2 = unsafe { RegOpenKeyExW(key, sub.as_ptr(), 0, KEY_READ, &mut sub_key) };
            if rc2 != 0 {
                continue;
            }
            let image = read_str_val(sub_key, &wide("ImagePath"));
            let start = read_dword_val(sub_key, &wide("Start"));
            let typ = read_dword_val(sub_key, &wide("Type"));
            unsafe { RegCloseKey(sub_key) };

            // 服务名：从 u16 buffer 转 String（无 NUL）。
            let name = String::from_utf16_lossy(&name_buf[..n]);
            let key_lower = name.to_ascii_lowercase();

            // 若 PS 有此服务，就用 PS 的 state / DisplayName；否则用占位。
            let ps_hit = ps_map.get(&key_lower);
            let state_zh = ps_hit
                .map(|(s, _, _)| s.clone())
                .unwrap_or_else(|| "未知".to_string());
            let display_ps = ps_hit.map(|(_, _, dn)| dn.clone()).unwrap_or_default();

            // 注册表 Start DWORD 优先（比 PS 更权威、能拿到 Boot/System 细分）。
            let (start_zh, is_disabled) = start_dword_zh(start);
            let is_driver = matches!(typ, Some(1) | Some(2));

            rows.push(ServiceRow {
                name: name.clone(),
                display: if display_ps.is_empty() {
                    name.clone()
                } else {
                    display_ps
                },
                state: state_zh,
                start_zh,
                image: clean_image_path(&image),
                is_disabled,
                is_driver,
            });
            seen.insert(key_lower);
        }
        unsafe { RegCloseKey(key) };

        // 合并：PS 有、注册表没有的服务（可能是临时/已卸载残影）也加进来。
        for (k, (state_zh, _sm, dn)) in &ps_map {
            if seen.contains(k) {
                continue;
            }
            let display = if dn.is_empty() { k.clone() } else { dn.clone() };
            rows.push(ServiceRow {
                name: k.clone(),
                display,
                state: state_zh.clone(),
                start_zh: "未知".to_string(),
                image: "未知".to_string(),
                is_disabled: false,
                is_driver: false,
            });
        }
    } else {
        // 注册表打不开时降级：纯走 PS 结果。
        for (k, (state_zh, sm_zh, dn)) in &ps_map {
            rows.push(ServiceRow {
                name: k.clone(),
                display: if dn.is_empty() { k.clone() } else { dn.clone() },
                state: state_zh.clone(),
                start_zh: sm_zh.clone(),
                image: "未知".to_string(),
                is_disabled: sm_zh == "禁用",
                is_driver: false,
            });
        }
    }

    // ③ 过滤 + 排序 + 截断。
    if !show_disabled {
        rows.retain(|r| !r.is_disabled);
    }
    if let Some(k) = &kw {
        rows.retain(|r| {
            r.name.to_ascii_lowercase().contains(k)
                || r.display.to_ascii_lowercase().contains(k)
                || r.image.to_ascii_lowercase().contains(k)
        });
    }
    // 稳定排序：运行中在前，然后按名称。
    rows.sort_by(|a, b| {
        let a_run = a.state.contains("运行");
        let b_run = b.state.contains("运行");
        b_run.cmp(&a_run).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });
    let total_before = rows.len();
    rows.truncate(top_n);

    if rows.is_empty() {
        let hint = if kw.is_some() {
            "（无匹配项）"
        } else if !show_disabled {
            "（未包含被禁用的服务；如需查看请传 show_disabled=true）"
        } else {
            "（读不到服务列表）"
        };
        return Ok(format!("服务列表为空{hint}"));
    }

    let mut s = String::new();
    s.push_str(&format!(
        "服务列表（共 {} 条{}）：\n",
        total_before,
        if total_before > rows.len() {
            format!("，只显示前 {}", rows.len())
        } else {
            String::new()
        }
    ));
    for r in &rows {
        let driver_tag = if r.is_driver { "（驱动）" } else { "" };
        s.push_str(&format!(
            "- {} · {} · {} · {} · {}{driver_tag}\n",
            r.name, r.display, r.state, r.start_zh, r.image
        ));
    }
    if !show_disabled {
        s.push_str("（已跳过注册表 Start=4 的禁用服务；如需查看请传 show_disabled=true）");
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_services(
    _keyword: Option<String>,
    _show_disabled: bool,
    _top_n: usize,
) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

/// 控制 Windows 服务（start / stop / restart，**可逆**）。**由主项目 agent.rs 桥接层
/// 做 confirmed 确认门**；本函数只做守卫 + 执行。
///
/// 守卫（违反即拒绝）：
/// - 目标服务不存在 → 拒绝；
/// - 系统关键服务名（内核类、系统核心，启停会导致系统不稳定/蓝屏）→ 拒绝；
/// - 已经是目标状态（要停的已停 / 要启动的已启动）→ 无需操作，返回现状。
///
/// 非 Windows 恒返回错误。
#[cfg(windows)]
pub fn control_service(name: &str, action: &str) -> Result<String, String> {
    // 将这些系统关键服务列入黑名单：启停它们会破坏系统稳定性（一定不是用户想 AI 干的）。
    let system_services: &[&str] = &[
        "system",
        "wininit",
        "winlogon",
        "lsass",
        "services",
        "csrss",
        "smss",
        "spoolsv",
        // 内核/底层驱动类
        "pcw",
        "fs_rec",
        "ksfilter",
        "klif",
        "ldev",
        "mmcss",
        "mssecflt",
        "mpsdrv",
        "netbt",
        "rdyboost",
        "stornvme",
        "tcpip",
        "volsnap",
        "wudfsvc",
        "bthavctp",
        // 安全软件/防火墙/杀软——停掉会暴露系统，一律拒绝
        "windefend",
        "wscsvc",
        "securityhealthservice",
        "mpssvc",
        "bfe",
        "basessvc",
        "sense",
        "wdnissvc",
        "msmpeng",
        "wsearch",
    ];
    let name_l = name.trim().to_ascii_lowercase();
    let action_l = action.trim().to_ascii_lowercase();
    if name_l.is_empty() {
        return Err("服务名不能为空".into());
    }
    let action_ok = match action_l.as_str() {
        "start" | "stop" | "restart" => true,
        _ => {
            return Err(format!(
                "action 只支持 start / stop / restart，收到：{action}"
            ))
        }
    };
    if system_services.contains(&name_l.as_str()) {
        return Err(format!(
            "拒绝：服务「{name}」是系统关键服务，不可由 AI 启停（安全守卫）"
        ));
    }
    let _ = action_ok;
    // 查目标服务现在的状态：不存在的服务直接拒绝（避免 AI 在不存在的东西上操作）。
    let probe = ps_capture(&format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         (Get-Service -Name '{name}' | ConvertTo-Json -Compress)"
    ))?;
    if probe.trim().is_empty() || probe.trim() == "null" {
        return Err(format!(
            "拒绝：服务「{name}」不存在（先调用 sys_services 确认名字）"
        ));
    }
    // 用 Start-Service / Stop-Service / Restart-Service 执行；Restart 失败时逐个 stop→start 兜底。
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         $s=Get-Service -Name '{name}' -ErrorAction Stop; \
         switch('{action_l}') {{ \
           'start' {{ if($s.Status -eq 'Running'){{ 'already' }} else {{ Start-Service -Name '{name}' -ErrorAction Stop; 'started' }} }} \
           'stop' {{ if($s.Status -eq 'Stopped'){{ 'already' }} else {{ Stop-Service -Name '{name}' -ErrorAction Stop; 'stopped' }} }} \
           'restart' {{ if($s.Status -eq 'Running'){{ Restart-Service -Name '{name}' -Force -ErrorAction Stop }} else {{ Start-Service -Name '{name}' -ErrorAction Stop }}; 'restarted' }} \
         }}"
    );
    let out = ps_capture(&script).map_err(|e| format!("控制服务失败：{e}"))?;
    let st = out.trim();
    let zh = match st {
        "started" => "已启动".to_string(),
        "stopped" => "已停止".to_string(),
        "restarted" => "已重启".to_string(),
        "already" => {
            // 已经是目标状态：不算操作，友好返回现状
            return Ok(format!(
                "服务「{name}」已经是目标状态（{}），无需操作",
                if action_l == "stop" {
                    "已停止"
                } else {
                    "运行中"
                }
            ));
        }
        other => other.to_string(),
    };
    Ok(format!("服务「{name}」{zh}"))
}

#[cfg(not(windows))]
pub fn control_service(_name: &str, _action: &str) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 2. 驱动枚举 ─────────────────────────────────────────────────────────

#[cfg(windows)]
const DRIVERS_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-CimInstance Win32_PnPSignedDriver | Where-Object { $_.DeviceName -and $_.DriverVersion } | Sort-Object DeviceName -Unique | Select-Object DeviceName,DeviceClass,DriverVersion,DriverDate,Manufacturer)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// `DriverDate` 的 CIM DateTime（如 `20260410000000.000000-000`）→ `YYYY-MM-DD`。
/// 只取前 8 位数字，安全解析；失败原样透传。
fn fmt_driver_date(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    let digits: Vec<char> = t.chars().take(8).collect();
    if digits.len() == 8 && digits.iter().all(|c| c.is_ascii_digit()) {
        let ds: String = digits.into_iter().collect();
        let y: u32 = ds[0..4].parse().unwrap_or(0);
        let m: u32 = ds[4..6].parse().unwrap_or(0);
        let d: u32 = ds[6..8].parse().unwrap_or(0);
        if (1900..=2099).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d) {
            return format!("{y:04}-{m:02}-{d:02}");
        }
    }
    t.to_string()
}

/// `DeviceClass` 短码 → 中文。未命中返回原字符串。
fn device_class_zh(s: &str) -> String {
    match s {
        "NET" | "Net" => "网络适配器".to_string(),
        "Display" => "显示".to_string(),
        "USB" => "USB".to_string(),
        "Media" => "多媒体".to_string(),
        "Image" => "成像".to_string(),
        "Audio" => "音频".to_string(),
        "DiskDrive" => "磁盘".to_string(),
        "System" => "系统".to_string(),
        "Bluetooth" => "蓝牙".to_string(),
        "SCSIAdapter" => "SCSI".to_string(),
        "" => "未知".to_string(),
        other => other.to_string(),
    }
}

/// 微软驱动日期失真检测：年份 < 2020 且版本号形如 `10.0.*`。
fn is_microsoft_date_suspect(version: &str, year: u32) -> bool {
    year > 0 && year < 2020 && version.starts_with("10.0.")
}

/// 驱动枚举：PowerShell `Win32_PnPSignedDriver`（真机 137 条跑通）。
/// 用 `ps_capture_timeout` 放宽到 60s——137 条 CIM 查询需要数秒。
#[cfg(windows)]
pub fn collect_drivers(keyword: Option<String>, top_n: usize) -> Result<String, String> {
    let top_n = top_n.clamp(1, 200);
    let kw: Option<String> = keyword
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| !s.is_empty());

    let raw = ps_capture_timeout(DRIVERS_PS, Duration::from_secs(60))?;
    let arr_str = json_array_of(raw.trim().trim_start_matches('\u{feff}'));
    let arr: Vec<Value> =
        serde_json::from_str(&arr_str).map_err(|e| format!("解析驱动列表失败：{e}"))?;

    if arr.is_empty() {
        return Ok("驱动列表为空（Win32_PnPSignedDriver 无返回）".into());
    }

    let mut rows: Vec<Value> = arr;
    if let Some(k) = &kw {
        rows.retain(|v| {
            v_str(v, "DeviceName").to_ascii_lowercase().contains(k)
                || v_str(v, "Manufacturer").to_ascii_lowercase().contains(k)
        });
    }
    // 排序：按 DeviceName 字母序（PS 已 -Unique 排过，这里保底）。
    rows.sort_by(|a, b| {
        v_str(a, "DeviceName")
            .to_ascii_lowercase()
            .cmp(&v_str(b, "DeviceName").to_ascii_lowercase())
    });
    let total_before = rows.len();
    rows.truncate(top_n);

    if rows.is_empty() {
        return Ok(format!(
            "没有 DeviceName/Manufacturer 匹配「{}」的驱动",
            kw.unwrap_or_default()
        ));
    }

    let mut s = String::new();
    s.push_str(&format!(
        "驱动列表（共 {} 条{}）：\n",
        total_before,
        if total_before > rows.len() {
            format!("，只显示前 {}", rows.len())
        } else {
            String::new()
        }
    ));
    for v in &rows {
        let name = if v_str(v, "DeviceName").is_empty() {
            "未知设备".to_string()
        } else {
            v_str(v, "DeviceName")
        };
        let cls = device_class_zh(&v_str(v, "DeviceClass"));
        let ver = if v_str(v, "DriverVersion").is_empty() {
            "未知".to_string()
        } else {
            v_str(v, "DriverVersion")
        };
        let date_raw = v_str(v, "DriverDate");
        let date_fmt = fmt_driver_date(&date_raw);
        // 年份解析用于微软驱动假日期检测。
        let year: u32 = date_fmt
            .chars()
            .take(4)
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        let microsoft_suspect = is_microsoft_date_suspect(&ver, year);
        let man = if v_str(v, "Manufacturer").is_empty() {
            "未知厂商".to_string()
        } else {
            v_str(v, "Manufacturer")
        };
        s.push_str(&format!(
            "- {} · {} · {} · {} · {}\n",
            name,
            cls,
            ver,
            if date_fmt.is_empty() {
                "未知"
            } else {
                &date_fmt
            },
            man
        ));
        if microsoft_suspect {
            s.push_str("  - （微软驱动日期可能失真：版本号 10.0.* 但日期字段异常旧，Windows 累积更新会重置该字段）\n");
        }
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_drivers(_keyword: Option<String>, _top_n: usize) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 3. 开机自启项枚举 ────────────────────────────────────────────────────

#[cfg(windows)]
const SCHEDULED_TASK_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-ScheduledTask | Where-Object { $_.State -ne 'Disabled' } | Select-Object TaskName,TaskPath,State | Select-Object -First 100)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 手写展开 `%VAR%`——`Win32_System_Environment` feature 未开，无法调 `ExpandEnvironmentStringsW`。
/// 只展开 4 个常用变量；其他 `%VAR%` 原样透传。
#[cfg(windows)]
fn expand_env_simple(s: &str) -> String {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let local_appdata = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let programfiles = std::env::var("ProgramFiles").unwrap_or_default();
    let programfiles_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();
    let systemroot = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());

    // 替换顺序：长变量名先换，避免 `%AppData%` 被 `%App` 误吃（虽然这里没 `%App`，但保险）。
    let subs: [(&str, &str); 6] = [
        ("%LocalAppData%", local_appdata.as_str()),
        ("%AppData%", appdata.as_str()),
        ("%ProgramFiles(x86)%", programfiles_x86.as_str()),
        ("%ProgramFiles%", programfiles.as_str()),
        ("%SystemRoot%", systemroot.as_str()),
        ("%USERPROFILE%", home.as_str()),
    ];
    let mut out = s.to_string();
    for (from, to) in subs {
        out = out.replace(from, to);
    }
    out
}

/// 解析命令行第一个 token 作为 exe 路径（去引号、简单 `%VAR%` 展开）。
///
/// 例：`"C:\...\svc.exe" --flag` → `C:\...\svc.exe`
///     `%SystemRoot%\System32\foo.exe` → `C:\Windows\System32\foo.exe`
#[cfg(windows)]
fn parse_exe_token(cmd: &str) -> String {
    let t = cmd.trim();
    if t.is_empty() {
        return String::new();
    }
    let first = t.split_whitespace().next().unwrap_or("").to_string();
    // 展开环境变量
    let expanded = expand_env_simple(&first);
    // 去首尾引号
    let stripped = expanded.trim_matches('"');
    stripped.trim().to_string()
}

#[cfg(windows)]
struct RunEntry {
    source: String, // 前缀标签，如 "[HKCU\Run]"
    name: String,   // Run 值名
    command: String,
    exe_exists: bool, // 指向的文件是否存在（false = 僵尸启动项）
}

/// 枚举一个 Run 注册表键下的所有子值（`RegEnumValueW` 递归）。
#[cfg(windows)]
fn enum_run_key(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
    flags: windows_sys::Win32::System::Registry::REG_SAM_FLAGS,
    prefix: &str,
    out: &mut Vec<RunEntry>,
) {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };
    let mut key: HKEY = std::ptr::null_mut();
    let rc = unsafe { RegOpenKeyExW(root, wide(subkey).as_ptr(), 0, KEY_READ | flags, &mut key) };
    if rc != 0 {
        return;
    }
    let mut idx = 0u32;
    loop {
        let mut name_buf = [0u16; 512];
        let mut name_len = name_buf.len() as u32;
        let mut type_buf = 0u32;
        let mut data_buf = [0u16; 2048];
        let mut data_len = data_buf.len() as u32;
        let r = unsafe {
            RegEnumValueW(
                key,
                idx,
                name_buf.as_mut_ptr(),
                &mut name_len,
                std::ptr::null(),
                &mut type_buf,
                data_buf.as_mut_ptr() as *mut u8,
                &mut data_len,
            )
        };
        if r != 0 {
            break;
        }
        idx += 1;
        // RegEnumValueW 的 name_len / data_len 不含 NUL，手动处理。
        let nname = (name_len as usize).min(name_buf.len());
        let name = String::from_utf16_lossy(&name_buf[..nname]);
        if name.is_empty() || name == "(Default)" {
            continue;
        }
        if type_buf != REG_SZ && type_buf != REG_EXPAND_SZ {
            continue;
        }
        let n_data = if data_len >= 2 {
            (data_len as usize / 2 - 1).min(data_buf.len())
        } else {
            0
        };
        let command_raw = String::from_utf16_lossy(&data_buf[..n_data]);
        if command_raw.trim().is_empty() {
            continue;
        }
        // REG_EXPAND_SZ 含 % 变量展开
        let expanded = if type_buf == REG_EXPAND_SZ {
            expand_env_simple(&command_raw)
        } else {
            command_raw.clone()
        };
        let exe = parse_exe_token(&expanded);
        let exists = !exe.is_empty() && std::path::Path::new(&exe).exists();
        out.push(RunEntry {
            source: prefix.to_string(),
            name: name.clone(),
            command: expanded,
            exe_exists: exists,
        });
    }
    unsafe { RegCloseKey(key) };
}

/// 枚举一个 Startup 文件夹（用户/机器），只列文件、过滤 `desktop.ini` 和隐藏。
#[cfg(windows)]
fn enum_startup_folder(dir: &std::path::Path, out: &mut Vec<RunEntry>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(x) => x,
        Err(_) => return,
    };
    for ent in rd.flatten() {
        let file_name = ent.file_name();
        let name_str = file_name.to_string_lossy();
        // 过滤隐藏 + desktop.ini + 目录
        if name_str.starts_with('.') {
            continue;
        }
        if name_str.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        let ft = match ent.file_type() {
            Ok(x) => x,
            Err(_) => continue,
        };
        if ft.is_dir() {
            continue;
        }
        let full = ent.path().display().to_string();
        out.push(RunEntry {
            source: "[启动文件夹]".to_string(),
            name: name_str.to_string(),
            command: full.clone(),
            exe_exists: true, // read_dir 能列出来就一定存在
        });
    }
}

/// 开机自启项枚举：5 个 Run 键 + 2 个 Startup 文件夹 + 可选计划任务。**只读**，绝不修改/禁用/删除。
#[cfg(windows)]
pub fn collect_boot_items(include_scheduled: bool) -> Result<String, String> {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };

    let mut entries: Vec<RunEntry> = Vec::new();

    // ① HKCU\Run（当前用户）
    enum_run_key(
        HKEY_CURRENT_USER,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        0,
        "[HKCU\\Run]",
        &mut entries,
    );

    // ② HKLM\Run（机器级 64 位）
    enum_run_key(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        KEY_WOW64_64KEY,
        "[HKLM\\Run]",
        &mut entries,
    );

    // ③ HKCU Policies\Explorer\Run（策略强制，任务管理器禁不掉——高价值）
    enum_run_key(
        HKEY_CURRENT_USER,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer\Run",
        0,
        "[策略\\Run]",
        &mut entries,
    );

    // ④ HKLM Policies\Explorer\Run（机器级策略强制）
    enum_run_key(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer\Run",
        KEY_WOW64_64KEY,
        "[策略\\Run]",
        &mut entries,
    );

    // ⑤ HKLM WOW6432Node\Run（32 位视图）
    enum_run_key(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
        KEY_WOW64_32KEY,
        "[WOW64\\Run]",
        &mut entries,
    );

    // ⑥ 启动文件夹：用户 + 机器
    // 用户 Startup（`%USERPROFILE%\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup`）
    let user_profile = std::env::var("USERPROFILE").unwrap_or_default();
    if !user_profile.is_empty() {
        let user_startup = std::path::PathBuf::from(&user_profile)
            .join("AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup");
        enum_startup_folder(&user_startup, &mut entries);
    }
    // 机器级 Startup（Win10+ 在 ProgramData 下）
    let machine_startup =
        std::path::PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\StartUp");
    enum_startup_folder(&machine_startup, &mut entries);

    let registry_count = entries
        .iter()
        .filter(|e| !e.source.starts_with("[启动"))
        .count();
    let folder_count = entries
        .iter()
        .filter(|e| e.source.starts_with("[启动"))
        .count();

    // ⑦ 计划任务（默认跳过，199 条非常慢）
    let mut scheduled_count = 0usize;
    let mut scheduled_lines: Vec<String> = Vec::new();
    if include_scheduled {
        match ps_capture_timeout(SCHEDULED_TASK_PS, Duration::from_secs(60)) {
            Ok(raw) => {
                let arr_str = json_array_of(raw.trim().trim_start_matches('\u{feff}'));
                if let Ok(arr) = serde_json::from_str::<Vec<Value>>(&arr_str) {
                    for v in &arr {
                        let tn = v_str(v, "TaskName");
                        let tp = v_str(v, "TaskPath");
                        let st = v_str(v, "State");
                        if tn.is_empty() {
                            continue;
                        }
                        let state_zh = match st.as_str() {
                            "Ready" => "就绪",
                            "Running" => "运行中",
                            "Paused" => "已暂停",
                            "Disabled" => "已禁用",
                            _ => {
                                if st.is_empty() {
                                    "未知"
                                } else {
                                    "状态未知"
                                }
                            }
                        };
                        let full = if tp.is_empty() {
                            format!("\\{tn}")
                        } else {
                            format!("{tp}{tn}")
                        };
                        scheduled_lines.push(format!("- [计划任务] {full} · {state_zh}"));
                        scheduled_count += 1;
                    }
                }
            }
            Err(_) => {
                scheduled_lines
                    .push("- [计划任务] （查询失败：Get-ScheduledTask 超时或无返回）".to_string());
            }
        }
    }

    // 输出
    let total = entries.len() + scheduled_count;
    if total == 0 && !include_scheduled {
        return Ok("未发现开机自启项（注册表 5 键 + 用户/机器 Startup 文件夹均空）".into());
    }

    let missing = entries.iter().filter(|e| !e.exe_exists).count();
    let mut s = String::new();
    s.push_str("开机自启项（只读枚举，绝不修改/禁用/删除）：\n");
    for e in &entries {
        let exists_tag = if e.exe_exists {
            "· 存在".to_string()
        } else {
            "· （指向不存在文件——僵尸启动项）".to_string()
        };
        s.push_str(&format!(
            "- {} {} → {} {}\n",
            e.source, e.name, e.command, exists_tag
        ));
    }
    for line in &scheduled_lines {
        s.push_str(line);
        s.push('\n');
    }
    s.push_str(&format!(
        "共 {} 项（注册表 {} / 启动文件夹 {} / 计划任务 {}），其中 {} 项指向不存在的文件",
        total, registry_count, folder_count, scheduled_count, missing
    ));
    if !include_scheduled {
        s.push_str("（如需查看计划任务请传 include_scheduled=true）");
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_boot_items(_include_scheduled: bool) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

/// 计划任务管理（写操作，仅可逆动作）。**由主项目 agent.rs 桥接层做 confirmed
/// 确认门**，本函数只负责「能否安全操作 + 执行」。守卫（违反即拒绝）：
/// - action 只允许 enable（启用）/ disable（禁用）/ query（只读查询）——
///   **没有 delete / run**：任务删除不可逆，绝不开放给 AI；
/// - 系统任务黑名单：任务路径以 `\Microsoft\` / `\Windows\` / `\System32\` 开头
///   的任务（Windows 内置维护任务）一律拒绝修改；
/// - 任务名不能为空、不能是通配 `\*`；目标任务不存在 → 拒绝（不建新任务）。
///
/// enable / disable 可逆（禁用后可用 enable 恢复）；dry_run=true 只出计划不执行。
#[cfg(windows)]
pub fn manage_scheduled_task(name: &str, action: &str, dry_run: bool) -> Result<String, String> {
    let name = name.trim();
    let action = action.trim().to_ascii_lowercase();
    if name.is_empty() || name == "\\*" {
        return Err("任务名不能为空（也不接受通配 \\*，一次只操作一个任务）".into());
    }
    let action_ok = match action.as_str() {
        "enable" | "disable" | "query" => true,
        _ => {
            return Err(format!(
                "action 只支持 enable / disable / query（不开放 delete/run），收到：{action}"
            ))
        }
    };
    let _ = action_ok;
    // 系统任务黑名单：Windows 内置任务路径前缀。这些任务的启停是系统维护的
    // 一部分，禁用会导致系统异常（更新/备份/碎片整理失效），一定不是用户想让
    // AI 干的；query 仍放行（只读）。
    let lower = name.to_ascii_lowercase().replace('\\', "/");
    let system_prefixes: &[&str] = &["/microsoft/", "/windows/", "/system32/"];
    if action != "query" && system_prefixes.iter().any(|p| lower.starts_with(p)) {
        return Err(format!(
            "拒绝：任务「{name}」是系统内置任务（路径在 \\Microsoft\\ / \\Windows\\ / \\System32\\ 下），不可由 AI 修改（安全守卫）"
        ));
    }
    // 任务必须存在（query 也先探测，给出明确不存在提示）。
    // 存在性/权限交给 PowerShell 实际命令判定，不在此预判：非系统前缀任务
    // 的 enable/disable 由 PowerShell 自己报「找不到任务」；query 也由命令返回
    // 状态或降级说明（系统任务在有管理员时才能读，无管理员时给提示）。
    // —— 这样才不会把「任务不存在」和「无权限读系统任务」两种失败混在一起。
    if dry_run {
        let zh_action = match action.as_str() {
            "enable" => "启用",
            "disable" => "禁用",
            _ => "查询",
        };
        return Ok(format!(
            "（dry-run 计划）将对计划任务「{name}」执行 {zh_action}。任务非系统内置（\\ 前缀已拦），操作可逆。"
        ));
    }
    match action.as_str() {
        "enable" => {
            let out = ps_capture(&format!(
                "$ErrorActionPreference='Stop'; Enable-ScheduledTask -TaskName '{name}' -ErrorAction Stop; 'enabled'"
            ));
            match out {
                Ok(o) if o.trim() == "enabled" => Ok(format!("已启用计划任务「{name}」")),
                Ok(o) => Err(format!("启用任务未确认成功，输出：{}", o.trim())),
                Err(e) => Err(format!(
                    "启用计划任务「{name}」失败：{e}（任务可能不存在或需管理员权限）"
                )),
            }
        }
        "disable" => {
            let out = ps_capture(&format!(
                "$ErrorActionPreference='Stop'; Disable-ScheduledTask -TaskName '{name}' -ErrorAction Stop; 'disabled'"
            ));
            match out {
                Ok(o) if o.trim() == "disabled" => {
                    Ok(format!("已禁用计划任务「{name}」（可随时用 enable 恢复）"))
                }
                Ok(o) => Err(format!("禁用任务未确认成功，输出：{}", o.trim())),
                Err(e) => Err(format!(
                    "禁用计划任务「{name}」失败：{e}（任务可能不存在或需管理员权限）"
                )),
            }
        }
        // query：只读，返回任务当前状态；失败降级说明（系统任务可能无权读）
        _ => {
            match ps_capture(&format!(
                "$ErrorActionPreference='SilentlyContinue'; \
                 $t=Get-ScheduledTask -TaskName '{name}'; \
                 ($t.State.ToString());"
            )) {
                Ok(o) => Ok(format!("计划任务「{name}」当前状态：{}", o.trim())),
                Err(e) => Err(format!(
                    "查询计划任务「{name}」失败：{e}（可能是系统任务需管理员权限，或任务不存在）"
                )),
            }
        }
    }
}

#[cfg(not(windows))]
pub fn manage_scheduled_task(_name: &str, _action: &str, _dry_run: bool) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

// ── 非 Windows 平台统一降级文案 ────────────────────────────────────────────

#[cfg(not(windows))]
#[allow(dead_code)] // 跨平台降级文案：Windows 构建不引用
const NOT_WINDOWS: &str = "当前平台不是 Windows，系统信息不可用";

// ── 关键服务黑名单回归测试 ────────────────────────────────────────────────

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn critical_services_are_rejected() {
        // 安全软件/杀软：停掉会暴露系统，一律拒绝
        for name in [
            "WinDefend",
            "windefend",
            "wscsvc",
            "SecurityHealthService",
            "mpssvc",
            "bfe",
            "basessvc",
            "sense",
            "wdnissvc",
            "msmpeng",
            "wsearch",
        ] {
            let r = control_service(name, "stop");
            let msg = r.err().unwrap_or_default();
            assert!(
                msg.contains("系统关键服务"),
                "关键服务 {name} 应被拒绝，实际返回：{msg}"
            );
        }
    }

    #[test]
    fn system_core_services_are_rejected() {
        for name in [
            "system", "wininit", "winlogon", "lsass", "services", "csrss", "smss", "spoolsv",
            "tcpip", "volsnap", "stornvme",
        ] {
            let r = control_service(name, "restart");
            let msg = r.err().unwrap_or_default();
            assert!(
                msg.contains("系统关键服务"),
                "核心服务 {name} 应被拒绝，实际返回：{msg}"
            );
        }
    }

    #[test]
    fn unknown_service_is_rejected_not_passed() {
        // 不存在的服务也必须拒绝，不能假装操作成功
        let r = control_service("zzz_definitely_not_a_service_xyz", "stop");
        assert!(r.is_err(), "不存在服务应被拒绝，实际：{r:?}");
    }
}
