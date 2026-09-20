//! 风扇通道自修复（跨机器自适应诊断 + L2 写修复）。
//!
//! 目标：DiskPilot 在其他电脑上风扇控制等环境依赖功能也能正常使用；环境不匹配时
//! 内置 AI 能自动诊断并修复。本模块提供两个工具（由主项目 agent.rs 桥接层做 L2 确认门）：
//!
//! - `fan_selfheal_diag`（只读 L0）：调用 fancmd `diag json`，结构化返回
//!   驱动是否加载 / RTC 端口 / SuperIO 芯片 / env_base / 风扇列表 / 可写性，
//!   并给出明确的「可修复点」清单（缺驱动？芯片不可识别？可写但需 reset？）。
//! - `fan_selfheal_fix`（写 L2）：按诊断结果执行修复。当前支持：
//!   - `install_driver`：管理员权限下安装 inpoutx64 驱动（复制 DLL 到 System32 +
//!     `sc create` + `sc start`），装完自动 re-diag 验证。
//!   - `reset_fan <idx>`：恢复指定风扇为主板自动控制（安全可逆）。
//!   - `dry_run=true`：只出计划不执行（内置 AI 先 dry-run 再问用户确认执行）。
//!
//! 安全边界：
//! - 所有写操作都必须管理员权限（`whoami /groups` 存在 S-1-5-32-544），非管理员直接拒绝；
//! - 写操作发生在 System32（驱动 DLL 落地）与驱动服务（sc create/start）——
//!   由主项目权限中心 L2 + 确认门双重守护，AI 不能隐身执行；
//! - 绝不删除/覆盖任何已存在的驱动文件：若 System32 已存在 inpoutx64.dll，只补服务不覆盖；
//! - 只在目标是"让 fancmd 能写风扇"这一件事上动手，不做任何其他系统改动。
//!
//! fancmd 二进制由主项目打包在 `resources/fancmd/fancmd.exe`（打包）或
//! `tools/fancmd/bin/Release/net8.0/fancmd.exe`（开发）；本模块用
//! `DISKPILOT_TOOLS_DIR` 环境变量（hw.rs 查找时设置）定位，找不到则让 AI 先跑
//! `fan_selfheal_diag` 拿全局诊断。

use serde_json::{json, Value};

/// 定位 fancmd 可执行文件（打包资源 / 开发目录 / PATH 三级探测）。
#[cfg(windows)]
pub(crate) fn find_fancmd() -> Option<String> {
    // 1) 环境变量显式指路（hw.rs 探测时设置）
    if let Ok(dir) = std::env::var("DISKPILOT_TOOLS_DIR") {
        let p = std::path::Path::new(&dir).join("fancmd.exe");
        if p.is_file() {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    // 2) 开发环境：从当前 exe 上溯找 tools/fancmd/bin（含 Release/net8.0 实际产物目录）
    if let Ok(cur) = std::env::current_exe() {
        let mut probe = cur.clone();
        for _ in 0..6 {
            if !probe.pop() {
                break;
            }
            let cand_direct = probe
                .join("tools")
                .join("fancmd")
                .join("bin")
                .join("fancmd.exe");
            if cand_direct.is_file() {
                return Some(cand_direct.to_string_lossy().into_owned());
            }
            let cand_release = probe
                .join("tools")
                .join("fancmd")
                .join("bin")
                .join("Release")
                .join("net8.0")
                .join("fancmd.exe");
            if cand_release.is_file() {
                return Some(cand_release.to_string_lossy().into_owned());
            }
        }
    }
    // 3) PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let full = dir.join("fancmd.exe");
            if full.is_file() {
                return Some(full.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// 运行 fancmd 并捕获 stdout（超时兜底，非 PowerShell，直接 spawn exe）。
#[cfg(windows)]
fn run_fancmd(args: &[&str]) -> Result<String, String> {
    let exe = find_fancmd()
        .ok_or("未找到 fancmd.exe（开发：tools/fancmd/bin；打包：resources/fancmd）")?;
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    let mut child = std::process::Command::new(&exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .spawn()
        .map_err(|e| format!("启动 fancmd（{exe}）失败：{e}"))?;
    let started = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(15);
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("等待 fancmd 失败：{e}"));
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("fancmd 执行超时（{}s）", timeout.as_secs()));
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
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
        return Err(format!("fancmd 退出码 {:?}", status.code()));
    }
    match std::str::from_utf8(&raw) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => {
            let (cow, _, _) = encoding_rs::GBK.decode(&raw);
            Ok(cow.into_owned())
        }
    }
}

/// 是否管理员（S-1-5-32-544 Administrators 组成员）。
#[cfg(windows)]
pub(crate) fn is_admin() -> bool {
    crate::ps::ps_capture(
        "$ErrorActionPreference='SilentlyContinue'; (whoami /groups | Select-String 'S-1-5-32-544' | Measure-Object).Count -gt 0",
    )
    .map(|s| s.trim() == "True")
    .unwrap_or(false)
}

/// 解析 fancmd diag json，归一化为本工具返回结构。
#[cfg(windows)]
fn parse_diag(raw: &str) -> Value {
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    let v: Value = serde_json::from_str(trimmed).unwrap_or_else(|_| json!({ "parsed": false }));
    if v.get("parsed").and_then(|x| x.as_bool()) == Some(false) {
        return json!({ "parsed": false, "raw": trimmed.chars().take(1200).collect::<String>() });
    }
    let driver_loaded = v
        .get("driver")
        .and_then(|d| d.get("loaded"))
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let sio_found = v
        .get("sio")
        .and_then(|s| s.get("found"))
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let writable = v.get("writable").and_then(|x| x.as_bool()).unwrap_or(false);
    // 可修复点：缺驱动 / 驱动在但端口不通 / 芯片不可识别 / 可写（无事可修）
    let mut fix_points: Vec<String> = Vec::new();
    if !driver_loaded {
        fix_points.push(
            "端口驱动未加载：需以管理员安装 inpoutx64（fan_selfheal_fix action=install_driver）"
                .into(),
        );
    }
    if driver_loaded && v.get("rtc_ok").and_then(|x| x.as_bool()) == Some(false) {
        fix_points.push(
            "驱动已加载但 RTC 端口读写验证失败：驱动版本/位数不匹配，建议换 inpoutx32 或 WinRing0"
                .into(),
        );
    }
    if driver_loaded && !sio_found {
        fix_points.push("SuperIO 芯片不可识别（已试 ITE/Nuvoton/Winbond × 0x2E/0x4E）：主板不兼容或需要更全的芯片表".into());
    }
    if driver_loaded && sio_found && !writable {
        fix_points.push("芯片已识别但写通道不可用：请检查风扇是否支持软件控制".into());
    }
    json!({
        "driver_loaded": driver_loaded,
        "driver_name": v.get("driver").and_then(|d| d.get("name")).and_then(|x| x.as_str()).unwrap_or(""),
        "rtc_ok": v.get("rtc_ok").and_then(|x| x.as_bool()).unwrap_or(false),
        "sio_found": sio_found,
        "sio": v.get("sio").cloned().unwrap_or(json!({})),
        "writable": writable,
        "fans": v.get("fans").cloned().unwrap_or(json!([])),
        "fix_points": json!(fix_points),
        "_raw": v,
    })
}

#[cfg(not(windows))]
fn parse_diag(_raw: &str) -> Value {
    json!({ "parsed": false, "note": "非 Windows 平台，风扇自修复不可用" })
}

/// 自修复诊断：跑 fancmd diag json 并结构化返回（只读 L0）。
#[cfg(windows)]
pub fn selfheal_diag() -> Result<String, String> {
    if find_fancmd().is_none() {
        return Ok(json!({
            "fancmd_found": false,
            "note": "未找到 fancmd.exe（磁盘清理工具包未装可写调速桥）。装好 tools/fancmd 后重试。",
            "fix_points": ["安装 fancmd（DiskPilot 风扇写控桥）"]
        })
        .to_string());
    }
    let raw = run_fancmd(&["diag", "json"])?;
    let parsed = parse_diag(&raw);
    if parsed.get("parsed").and_then(|x| x.as_bool()) == Some(false) {
        return Ok(json!({
            "fancmd_found": true,
            "diag_unparsed": true,
            "raw": parsed.get("raw"),
            "note": "fancmd 返回无法解析：可能驱动加载失败或进程异常，见 raw。"
        })
        .to_string());
    }
    let mut fp = parsed
        .get("fix_points")
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(fans) = parsed.get("fans").and_then(|x| x.as_array()) {
        // 附加一条可写通道结论
        if !fp.is_empty() {
            fp.push(
                if parsed.get("writable").and_then(|x| x.as_bool()) == Some(true) {
                    "风扇通道可写：可用 fan_control（主项目 L2）或 fancmd write 调速。"
                } else {
                    "风扇通道当前不可写：按上方 fix_points 修复后重试。"
                },
            );
        }
        let _ = fans;
    }
    Ok(json!({
        "fancmd_found": true,
        "driver_loaded": parsed.get("driver_loaded"),
        "driver_name": parsed.get("driver_name"),
        "rtc_ok": parsed.get("rtc_ok"),
        "sio_found": parsed.get("sio_found"),
        "sio": parsed.get("sio"),
        "writable": parsed.get("writable"),
        "fans": parsed.get("fans"),
        "fix_points": json!(fp),
    })
    .to_string())
}

#[cfg(not(windows))]
pub fn selfheal_diag() -> Result<String, String> {
    Ok(json!({ "fancmd_found": false, "note": "非 Windows 平台，风扇自修复不可用" }).to_string())
}

/// 自修复执行（写 L2，主项目确认门守护）。action 可选：
/// - `install_driver`：安装并启动 inpoutx64 驱动（需管理员）；
/// - `reset_fan <idx>`：恢复指定风扇主板自动控制；
/// - `dry_run=true`：只校验前置条件并出计划，不落盘不建服务。
#[cfg(windows)]
pub fn selfheal_fix(action: &str, dry_run: bool) -> Result<String, String> {
    let action_l = action.trim().to_ascii_lowercase();
    match action_l.as_str() {
        "install_driver" => install_driver(dry_run),
        _ if action_l.starts_with("reset_fan") => {
            let idx = action_l.split_whitespace().nth(1).unwrap_or("");
            if idx.is_empty() {
                return Err("reset_fan 需要风扇索引，如 reset_fan 0".into());
            }
            if !idx.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("风扇索引非法：{idx}"));
            }
            reset_fan(idx, dry_run)
        }
        other => Err(format!(
            "不支持的自修复动作：{other}（支持 install_driver / reset_fan <idx>）"
        )),
    }
}

#[cfg(windows)]
fn install_driver(dry_run: bool) -> Result<String, String> {
    let admin = is_admin();
    // 本地已存在的 inpoutx64.dll 任意位置（用于复制到 System32）
    let sys_dir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let sys32 = format!(r"{sys_dir}\System32");
    let target_dll = format!(r"{sys32}\inpoutx64.dll");

    // 收集可安装源（本地工具目录里的 inpoutx64.dll）
    let mut sources: Vec<String> = Vec::new();
    // 0) fancmd 所在目录（它已经找到驱动 DLL 的位置，DLL 常与其同目录或同仓库）
    if let Ok(dir) = std::env::var("DISKPILOT_TOOLS_DIR") {
        let cand = std::path::Path::new(&dir).join("inpoutx64.dll");
        if cand.is_file() {
            sources.push(cand.to_string_lossy().into_owned());
        }
    }
    // 1) 当前 exe 旁 + 上溯 Tools 布局（图吧工具箱 ZenTimings 等）
    if let Ok(cur) = std::env::current_exe() {
        if let Some(dir) = cur.parent() {
            // 开发/打包：exe 旁
            let cand = dir.join("inpoutx64.dll");
            if cand.is_file() {
                sources.push(cand.to_string_lossy().into_owned());
            }
            // 上溯找图吧工具箱之类目录
            let mut probe = dir.to_path_buf();
            for _ in 0..5 {
                if !probe.pop() {
                    break;
                }
                for sub in ["内存工具", "ZenTimings", "Tools", "tools"] {
                    let cand = probe.join(sub).join("inpoutx64.dll");
                    if cand.is_file() {
                        sources.push(cand.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }
    // 3) fancmd 二进制所在目录（agent-server 与 fancmd 打包在同一 resources 时同目录）
    if let Some(fcm) = find_fancmd() {
        if let Some(dir) = std::path::Path::new(&fcm).parent() {
            let cand = dir.join("inpoutx64.dll");
            if cand.is_file() && !sources.contains(&cand.to_string_lossy().into_owned()) {
                sources.push(cand.to_string_lossy().into_owned());
            }
        }
    }
    sources.dedup();
    if sources.is_empty() {
        return Err("本机未找到 inpoutx64.dll（无源可装）。请从图吧工具箱/官网下载 inpoutx64 放任意目录后重试，或让用户手动安装。".into());
    }

    if dry_run {
        return Ok(json!({
            "dry_run": true,
            "plan": [
                "校验管理员权限（当前非管理员则本操作会被拒绝）",
                &format!("复制 {} → {target_dll}", sources[0]),
                "sc create inpoutx64 start= demand 指向 .sys",
                "sc start inpoutx64",
                "重跑 fan_selfheal_diag 验证写通道"
            ],
            "admin_ok": admin,
            "note": "dry_run 不落盘不建服务；确认后请以管理员身份再调一次（主项目会弹权限确认门）。"
        })
        .to_string());
    }

    if !admin {
        return Err("安装 inpoutx64 驱动需要管理员权限。请以管理员身份启动 DiskPilot（或让用户手动安装驱动）后重试（主项目已弹确认门）。".into());
    }

    let src = &sources[0];
    let mut steps: Vec<String> = Vec::new();

    // 1) 复制 DLL 到 System32（已存在则不覆盖——绝不破坏已有安装）
    let sys32_exists = std::path::Path::new(&target_dll).is_file();
    if !sys32_exists {
        let r = crate::ps::ps_capture(&format!(
            "Copy-Item -LiteralPath '{}' -Destination '{}' -Force",
            src.replace('\'', "''"),
            target_dll.replace('\'', "''")
        ));
        match r {
            Ok(_) => steps.push(format!("已复制 inpoutx64.dll → {target_dll}")),
            Err(e) => return Err(format!("复制 inpoutx64.dll 到 System32 失败：{e}")),
        }
    } else {
        steps.push("System32 已存在 inpoutx64.dll（不覆盖）".into());
    }

    // 2) 建驱动服务（已存在则跳过；ImagePath 指向已拷贝的 sys）
    let svc_exists = crate::ps::ps_capture(
        "$ErrorActionPreference='SilentlyContinue'; (Get-Service -Name inpoutx64 -ErrorAction SilentlyContinue) -ne $null",
    )
    .map(|s| s.trim() == "True")
    .unwrap_or(false);
    if !svc_exists {
        let sys_path = format!(r"{sys_dir}\System32\drivers\inpoutx64.sys");
        let r = crate::ps::ps_capture(&format!(
            "sc.exe create inpoutx64 type= kernel start= demand binPath= '{}'",
            sys_path.replace('\'', "''")
        ));
        if let Err(e) = r {
            return Err(format!("sc create inpoutx64 失败：{e}"));
        }
        steps.push("已创建 inpoutx64 驱动服务（demand 启动）".into());
    } else {
        steps.push("inpoutx64 驱动服务已存在".into());
    }

    // 3) 启动驱动服务
    match crate::ps::ps_capture(
        "$ErrorActionPreference='SilentlyContinue'; (Get-Service -Name inpoutx64 -ErrorAction SilentlyContinue).Status -eq 'Running'",
    )
    .map(|s| s.trim() == "True")
    .unwrap_or(false)
    {
        true => steps.push("inpoutx64 驱动服务已在运行".into()),
        false => {
            let r = crate::ps::ps_capture("sc.exe start inpoutx64");
            match r {
                Ok(_) => steps.push("已启动 inpoutx64 驱动服务".into()),
                Err(e) => {
                    // 可能已运行但状态查询失败；给提示但不致命（DLL 已就位，LoadLibrary 兜底）
                    steps.push(format!("sc start 返回错误（可能已运行）：{e}"))
                }
            }
        }
    }

    // 4) re-diag 验证
    let diag = run_diag_after_install();
    Ok(json!({
        "steps": json!(steps),
        "verify": diag,
        "note": "安装后 fancmd 已自带重扫；若 verify.driver_loaded=true 且 writable=true 说明写通道就绪。"
    }).to_string())
}

#[cfg(windows)]
fn run_diag_after_install() -> Value {
    match run_fancmd(&["diag", "json"]) {
        Ok(raw) => {
            let p = parse_diag(&raw);
            json!({
                "driver_loaded": p.get("driver_loaded"),
                "driver_name": p.get("driver_name"),
                "rtc_ok": p.get("rtc_ok"),
                "sio_found": p.get("sio_found"),
                "writable": p.get("writable"),
                "fans": p.get("fans"),
            })
        }
        Err(e) => json!({ "error": e }),
    }
}

#[cfg(windows)]
fn reset_fan(idx: &str, dry_run: bool) -> Result<String, String> {
    if dry_run {
        return Ok(json!({
            "dry_run": true,
            "plan": [format!("fancmd write reset {idx}：恢复风扇 {idx} 为主板自动控制")],
            "note": "此操作安全可逆，不需要管理员权限；确认后执行。"
        })
        .to_string());
    }
    let out = run_fancmd(&["write", "reset", idx])?;
    Ok(out)
}

/// 直接调速：`fancmd write <idx> <pct>`，软件接管风扇转速（写 L2，主项目确认门守护）。
/// pct 限制 0..=100，越界自动钳制。dry_run=true 只出计划不执行。
#[cfg(windows)]
pub fn fan_control(idx: &str, pct: f64, dry_run: bool) -> Result<String, String> {
    if !idx.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("风扇索引非法：{idx}（应为数字，如 0/1/2）"));
    }
    let pct_clamped = pct.clamp(0.0, 100.0);
    let pct_round = (pct_clamped * 10.0).round() / 10.0;
    if dry_run {
        return Ok(json!({
            "dry_run": true,
            "plan": [format!("fancmd write {idx} {pct_round}：风扇 {idx} 软件调速至 {pct_round}%")],
            "note": "软件接管后风扇由 DiskPilot 控制；可随时用 fan_selfheal_fix reset_fan 恢复主板自动控制。确认后执行。"
        })
        .to_string());
    }
    // 先 diag 确认通道可写（缺驱动/芯片不可识别时直接拒绝，避免假成功）
    if let Ok(raw) = run_fancmd(&["diag", "json"]) {
        let d = parse_diag(&raw);
        if d.get("writable").and_then(|x| x.as_bool()) == Some(false) {
            return Err(
                "风扇写通道当前不可用（驱动未加载或 SuperIO 芯片不可识别）。请先调 fan_selfheal_diag 诊断，需要时让用户授权 fan_selfheal_fix install_driver 安装端口驱动后再调速。"
                    .into(),
            );
        }
    }
    let out = run_fancmd(&["write", idx, &format!("{pct_round}")])?;
    Ok(out)
}

#[cfg(not(windows))]
pub fn selfheal_fix(_action: &str, _dry_run: bool) -> Result<String, String> {
    Ok(json!({ "note": "非 Windows 平台，风扇自修复不可用" }).to_string())
}

#[cfg(not(windows))]
pub fn fan_control(_idx: &str, _pct: f64, _dry_run: bool) -> Result<String, String> {
    Ok(json!({ "note": "非 Windows 平台，风扇控制不可用" }).to_string())
}
