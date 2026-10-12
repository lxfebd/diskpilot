//! 修复执行域（从 selfheal.rs 拆分，2026-10-12）。
//! selfheal_fix / install_driver / run_diag_after_install / reset_fan / fan_control（两平台版）。

// json! 宏被 cfg(not(windows)) 降级 fn 使用（Linux 下也有调用方），保持跨平台。
use serde_json::json;

// 采集实现均为 cfg(windows)（Linux 仅保留返回降级文案的 fn），
// Value 类型随 Windows 实现一起编译，避免 Linux 下 unused-import 告警。
#[cfg(windows)]
use serde_json::Value;

// sh_core 全为 Windows 实现，仅 cfg(windows) 分支引用。
#[cfg(windows)]
use super::sh_core::{find_fancmd, is_admin, run_fancmd};
#[cfg(windows)]
use super::sh_diag::parse_diag;

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
