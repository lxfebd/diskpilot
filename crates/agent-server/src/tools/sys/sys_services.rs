//! 服务枚举域（从 sys.rs 拆分，2026-10-12）。
//! collect_services / control_service + 注册表 ImagePath/Start 解析。

#[cfg(windows)]
use crate::ps::ps_capture;

#[cfg(windows)]
use crate::tools::{reg_read_dword, reg_read_str};

#[cfg(not(windows))]
use super::NOT_WINDOWS;

// ── Windows 平台专用工具 ────────────────────────────────────────────────

// PS 单引号字符串字面量转义（单引号翻倍 `''` 是 PS 唯一需要的转义）已统一
// 收进共享 crate `diskpilot-psutil`（原桌面 startup.rs 与 agent-server sys.rs
// 各一份，防漂移合并）。所有把 AI/用户可控文本放进 PowerShell 源码字面量的
// 位置都必须过它，否则 `'` 可逃出字符串、`;` 后接任意命令（命令注入）。
#[cfg(windows)]
pub(crate) use diskpilot_psutil::escape_ps_single_quoted;

/// UTF-16 宽字符串（注册表 API 输入用）。
#[cfg(windows)]
pub(crate) fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

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
pub(crate) fn state_zh(s: &str) -> &str {
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
mod parse;
#[cfg(windows)]
pub(crate) use parse::{clean_image_path, parse_ps_services, start_dword_zh, ServiceRow};

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
            let image = reg_read_str(sub_key, &wide("ImagePath"));
            let start = reg_read_dword(sub_key, &wide("Start"));
            let typ = reg_read_dword(sub_key, &wide("Type"));
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
            format!(
                "，只显示前 {}；用 keyword 缩小范围或传 top_n（上限 200）看更多",
                rows.len()
            )
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
    match action_l.as_str() {
        "start" | "stop" | "restart" => {}
        _ => {
            return Err(format!(
                "action 只支持 start / stop / restart，收到：{action}"
            ))
        }
    }
    if system_services.contains(&name_l.as_str()) {
        return Err(format!(
            "拒绝：服务「{name}」是系统关键服务，不可由 AI 启停（安全守卫）"
        ));
    }
    // 注入防护：name 一律不拼进 PS 源码，改由 `-Name $n` 变量绑定传入
    // （PS 变量绑定不做字符串解析，单引号/分号等恶意内容无法逃逸）。
    // 变量赋值里的字面量串仍需按 PS 规则转义：单引号翻倍（''）是唯一逃逸。
    let probe = ps_capture(&format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         $n='{escaped}'; (Get-Service -Name $n | ConvertTo-Json -Compress)",
        escaped = escape_ps_single_quoted(name)
    ))?;
    if probe.trim().is_empty() || probe.trim() == "null" {
        return Err(format!(
            "拒绝：服务「{name}」不存在（先调用 sys_services 确认名字）"
        ));
    }
    // 用 Start-Service / Stop-Service / Restart-Service 执行；Restart 失败时逐个 stop→start 兜底。
    // action 已在上方白名单校验（start/stop/restart），$a 绑定后 switch 匹配，杜绝拼接注入。
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         $n='{escaped}'; $a='{action_l}'; \
         $s=Get-Service -Name $n -ErrorAction Stop; \
         switch($a) {{ \
           'start' {{ if($s.Status -eq 'Running'){{ 'already' }} else {{ Start-Service -Name $n -ErrorAction Stop; 'started' }} }} \
           'stop' {{ if($s.Status -eq 'Stopped'){{ 'already' }} else {{ Stop-Service -Name $n -ErrorAction Stop; 'stopped' }} }} \
           'restart' {{ if($s.Status -eq 'Running'){{ Restart-Service -Name $n -Force -ErrorAction Stop }} else {{ Start-Service -Name $n -ErrorAction Stop }}; 'restarted' }} \
         }}",
        escaped = escape_ps_single_quoted(name),
        action_l = action_l
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
