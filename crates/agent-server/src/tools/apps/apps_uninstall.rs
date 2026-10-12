//! R5 卸载助手域（从 apps.rs 拆分，2026-10-12）。
//! uninstall_app + 白名单/静默参数校验。

// list_installed_apps 仅供 cfg(windows) 的 uninstall_app 使用（Linux 仅返回降级文案）。
#[cfg(windows)]
use super::apps_enum::list_installed_apps;

// ── R5 卸载助手 ─────────────────────────────────────────────────────────
// `uninstall_app`：写工具，从 UninstallString 白名单静默参数执行卸载。
// 安全模型（与 R4 process_start 同源，但更严格——卸载不可轻率）：
// - UninstallString 解析：支持引号包裹路径（"C:\Program Files\x\uninst.exe" /S）；
// - exe 白名单：必须位于已安装程序目录（System32 / Program Files / ProgramFiles(x86)
//   / LocalAppData / ProgramData）下的 .exe，拒绝 msiexec/rundll32/cmd 之外的裸命令
//   与任意路径（msiexec 本身是系统组件，AI 不应绕过 MSI 包直接调）；
// - 参数白名单：只允许常见静默参数（/S /silent /quiet /qn /VERYSILENT /SUPPRESSMSGBOXES
//   /NORESTART /norestart /usecurrentuser），拒绝 runas/delete/format/清注册表等危险词；
// - 卸载器本身可逆性由 Windows 卸载器决定（多数有确认/进度，用户在场）。
// **由主项目 agent.rs 桥接层做 confirmed 确认门**，本函数只负责「能否安全卸载 + 执行」。

#[cfg(windows)]
fn uninstall_dir_whitelisted(exe_lower: &str) -> bool {
    let mut whitelist: Vec<String> = Vec::new();
    if let Ok(sys) = std::env::var("WINDIR") {
        let w = sys.trim_end_matches('\\').to_lowercase();
        whitelist.push(format!("{w}\\system32"));
        whitelist.push(format!("{w}\\syswow64"));
    }
    if let Ok(pf) = std::env::var("PROGRAMFILES") {
        whitelist.push(pf.trim_end_matches('\\').to_lowercase());
    }
    if let Ok(pf86) = std::env::var("PROGRAMFILES(X86)") {
        whitelist.push(pf86.trim_end_matches('\\').to_lowercase());
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        whitelist.push(local.trim_end_matches('\\').to_lowercase());
    }
    if let Ok(pd) = std::env::var("PROGRAMDATA") {
        whitelist.push(pd.trim_end_matches('\\').to_lowercase());
    }
    whitelist
        .iter()
        .any(|w| exe_lower.starts_with(w.as_str()) || exe_lower.starts_with(&format!("{w}\\")))
}

/// 从 UninstallString 拆出 exe 路径与参数（支持引号包裹）。
/// 返回 (exe_path, args)。解析失败（无法定位 .exe）返回 None。
#[cfg(windows)]
fn parse_uninstall_string(s: &str) -> Option<(String, Vec<String>)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut args: Vec<String> = Vec::new();
    if let Some(stripped) = s.strip_prefix('"') {
        // "C:\path\uninst.exe" /S
        let end = stripped.find('"')? + 1;
        let exe = stripped[..end].to_string();
        for a in stripped[end + 1..].split_whitespace() {
            args.push(a.to_string());
        }
        Some((exe, args))
    } else {
        // 无引号：第一个空格前的整段是 exe（路径含空格时 UninstallString 通常带引号）
        let mut parts = s.splitn(2, ' ');
        let exe = parts.next()?.to_string();
        if let Some(rest) = parts.next() {
            for a in rest.split_whitespace() {
                args.push(a.to_string());
            }
        }
        Some((exe, args))
    }
}

/// 静默参数白名单：只放行常见无交互静默卸载参数。
/// 拒绝 runas / delete / format / /REMSAVEUSERDATA / CleanInstall 等危险词。
#[cfg(windows)]
fn args_silent_whitelisted(args: &[String]) -> Result<(), String> {
    const SILENT_OK: &[&str] = &[
        "/s",
        "/silent",
        "/quiet",
        "/qn",
        "/verysilent",
        "/suppressmsgbaxes",
        "/suppressmsgbaxes2",
        "/norestart",
        "/nor",
        "/usecurrentuser",
        "/allusers",
        "/currentuser",
        "-silent",
        "-quiet",
        "/qb",
        "-qb",
    ];
    for a in args {
        let l = a.to_ascii_lowercase();
        // 纯参数（以 - 或 / 开头）必须命中白名单
        if l.starts_with('-') || l.starts_with('/') {
            let core = l
                .trim_start_matches('-')
                .trim_start_matches('/')
                .split('=')
                .next()
                .unwrap_or("");
            if !SILENT_OK.contains(&core) {
                return Err(format!(
                    "拒绝：卸载参数「{a}」不在静默白名单内（只允许 {} 等无交互参数）",
                    SILENT_OK.join(" / ")
                ));
            }
        }
        // 裸词（非参数）不拦（可能是卸载器自带的文件名参数），但危险词一律拒绝
        if [
            "runas",
            "delete",
            "format",
            "remsaveuserdata",
            "cleaninstall",
            "f",
        ]
        .contains(&l.as_str())
        {
            return Err(format!(
                "拒绝：参数「{a}」涉及高危操作（runas/delete/format），不允许"
            ));
        }
    }
    Ok(())
}

/// 执行已安装程序的卸载器（**写操作**）。**由主项目 agent.rs 桥接层做 confirmed
/// 确认门**。守卫（违反即拒绝）：
/// - UninstallString 为空 / 无法解析出 .exe → 拒绝；
/// - exe 不在已安装程序目录白名单内 → 拒绝；
/// - 卸载参数含白名单之外的危险参数 → 拒绝。
///
/// 卸载器以分离方式 spawn（不等待），返回其 PID；卸载进度由卸载器自己的 UI 呈现。
#[cfg(windows)]
pub fn uninstall_app(app_name: &str) -> Result<String, String> {
    let name = app_name.trim();
    if name.is_empty() {
        return Err("程序名不能为空（先调用 app_list 确认 DisplayName）".into());
    }
    let apps = list_installed_apps();
    let app = apps
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            format!(
                "拒绝：未找到已安装程序「{name}」（先调用 app_list 确认名称，注意大小写与空格）"
            )
        })?;
    if app.uninstall_string.is_empty() {
        return Err(format!(
            "拒绝：程序「{name}」没有 UninstallString（可能是系统组件或商店应用，不能这样卸载）"
        ));
    }
    let (exe, args) = parse_uninstall_string(&app.uninstall_string).ok_or_else(|| {
        format!(
            "无法解析「{name}」的 UninstallString：{}",
            app.uninstall_string
        )
    })?;
    let exe_lower = exe.to_ascii_lowercase();
    if !uninstall_dir_whitelisted(&exe_lower) {
        return Err(format!(
            "拒绝：卸载器「{exe}」不在已安装程序目录白名单内（只允许 Program Files / LocalAppData 等目录下的卸载器）"
        ));
    }
    if !std::path::Path::new(&exe).is_file() {
        return Err(format!(
            "拒绝：卸载器「{exe}」不存在（程序可能已被移动或损坏）"
        ));
    }
    args_silent_whitelisted(&args)?;
    // 分离 spawn 卸载器（不等待——卸载器有自己的 UI/进度，用户在场确认）
    let child = std::process::Command::new(&exe)
        .args(&args)
        .spawn()
        .map_err(|e| format!("启动卸载器失败：{e}"))?;
    let arg_str = if args.is_empty() {
        String::new()
    } else {
        format!(" {}", args.join(" "))
    };
    Ok(format!(
        "已启动「{name}」的卸载器（{exe}{arg_str}，PID {}）。卸载过程由卸载器界面呈现，完成后建议用 app_list 复查是否还在。",
        child.id()
    ))
}

#[cfg(not(windows))]
pub fn uninstall_app(_app_name: &str) -> Result<String, String> {
    Ok("当前平台不是 Windows，卸载助手不可用".into())
}
