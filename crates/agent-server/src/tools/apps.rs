//! 已安装程序清单（Windows：注册表 Uninstall 键，HKLM 64/32 + HKCU）。
//! 只读枚举，不触发 UAC（无需管理员即可读当前用户与 HKLM 公共键）。
//! 用于回答「这个软件能卸载吗 / 机器上都装了什么」。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub install_location: String,
    pub estimated_size_mb: Option<u64>,
    pub uninstall_string: String,
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 读 REG_SZ 字符串值（带 NUL 自动截断）。
#[cfg(windows)]
fn read_str(key: windows_sys::Win32::System::Registry::HKEY, value: &[u16]) -> String {
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
    let n = (len as usize / 2 - 1).min(buf.len()); // 去结尾 NUL
    String::from_utf16_lossy(&buf[..n])
}

/// 读 REG_DWORD 值（EstimatedSize 单位 KB）。
#[cfg(windows)]
fn read_dword(key: windows_sys::Win32::System::Registry::HKEY, value: &[u16]) -> Option<u32> {
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

/// 枚举某个 Uninstall 键（含 flags 控制 32/64 视图）下的已安装程序。
#[cfg(windows)]
fn enum_uninstall_key(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &[u16],
    flags: windows_sys::Win32::System::Registry::REG_SAM_FLAGS,
    out: &mut Vec<InstalledApp>,
) {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, KEY_READ,
    };
    let mut key: HKEY = std::ptr::null_mut();
    let rc = unsafe { RegOpenKeyExW(root, subkey.as_ptr(), 0, KEY_READ | flags, &mut key) };
    if rc != 0 {
        return;
    }
    let mut idx = 0u32;
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
        // RegEnumKeyExW 的 name_len 不含 NUL，手动补
        let n = (name_len as usize).min(name_buf.len());
        let mut sub = name_buf[..n].to_vec();
        sub.push(0);

        let mut sub_key: HKEY = std::ptr::null_mut();
        let rc2 = unsafe { RegOpenKeyExW(key, sub.as_ptr(), 0, KEY_READ, &mut sub_key) };
        if rc2 != 0 {
            continue;
        }
        // 过滤系统组件与 AppX 注册残余
        let sys_comp = read_dword(sub_key, &wide("SystemComponent")).unwrap_or(0);
        let parent = read_str(sub_key, &wide("ParentKeyName"));
        let name = read_str(sub_key, &wide("DisplayName"));
        if sys_comp == 0 && parent.is_empty() && !name.is_empty() {
            out.push(InstalledApp {
                version: read_str(sub_key, &wide("DisplayVersion")),
                publisher: read_str(sub_key, &wide("Publisher")),
                install_location: read_str(sub_key, &wide("InstallLocation")),
                estimated_size_mb: read_dword(sub_key, &wide("EstimatedSize"))
                    .map(|kb| (kb / 1024) as u64),
                uninstall_string: read_str(sub_key, &wide("UninstallString")),
                name,
            });
        }
        unsafe { RegCloseKey(sub_key) };
    }
    unsafe { RegCloseKey(key) };
}

/// 枚举已安装程序（按名称排序）：HKLM 64 位 + HKLM 32 位（WOW6432Node）+ HKCU。
#[cfg(windows)]
pub fn list_installed_apps() -> Vec<InstalledApp> {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };
    const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    const UNINSTALL_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

    let mut out = Vec::new();
    enum_uninstall_key(
        HKEY_LOCAL_MACHINE,
        &wide(UNINSTALL),
        KEY_WOW64_64KEY,
        &mut out,
    );
    enum_uninstall_key(
        HKEY_LOCAL_MACHINE,
        &wide(UNINSTALL_32),
        KEY_WOW64_32KEY,
        &mut out,
    );
    enum_uninstall_key(HKEY_CURRENT_USER, &wide(UNINSTALL), 0, &mut out);

    out.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    out
}

#[cfg(not(windows))]
pub fn list_installed_apps() -> Vec<InstalledApp> {
    Vec::new()
}

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

// ── 软件许可证（Windows / Office 激活状态）─────────────────────────────

/// 解析 slmgr /xpr 混输输出（stdout+stderr）→ (是否激活, 描述)。
/// 纯函数，便于单元测试；分支（顺序很重要）：
/// 1. 权限拒绝（Access denied / 0xC0000022 / 0x80070005 / 拒绝访问）
/// 2. 未激活信号（not activated / 未激活 / license is not / 0xC004F* / 0xC004E*）
/// 3. 已激活信号（activated / licensed / genuine / 已激活 / 永久）
/// 4. 其余 → 未识别
#[cfg(windows)]
fn parse_activation_text(hay: &str) -> (bool, String) {
    let low = hay.to_ascii_lowercase();
    let denied = low.contains("access denied")
        || low.contains("0xc0000022")
        || low.contains("0x80070005")
        || hay.contains("拒绝访问");
    let not_activated = low.contains("not activated")
        || low.contains("license is not")
        || low.contains("0xc004f")
        || low.contains("0xc004e")
        || hay.contains("未激活")
        // KMS 批量授权：输出「批量激活将于 … 过期」= 非永久授权、即将过期
        || hay.contains("批量激活")
        || hay.contains("即将过期")
        || hay.contains("将于");
    let activated = low.contains("activated")
        || low.contains("licensed")
        || low.contains("genuine")
        || hay.contains("已激活")
        // 中文 slmgr 的 /xpr 成功输出是「…永久激活。……」，无「已激活」字样
        || hay.contains("永久激活");
    if denied {
        (
            false,
            "查询激活状态需要管理员权限（slmgr /xpr 被拒绝）".into(),
        )
    } else if not_activated {
        // 尽量保留 slmgr 原始行（如「批量激活将于 2026/10/2 过期」），信息量最大
        let line = hay
            .lines()
            .filter(|l| {
                let ll = l.to_ascii_lowercase();
                ll.contains("not activated")
                    || ll.contains("license is not")
                    || ll.contains("0xc004")
                    || l.contains("未激活")
                    || l.contains("批量激活")
                    || l.contains("过期")
            })
            .next();
        match line {
            Some(l) if !l.trim().is_empty() => (false, format!("Windows 未永久激活：{}", l.trim())),
            _ => (
                false,
                "Windows 未激活/即将过期（slmgr 输出未含激活信号）".into(),
            ),
        }
    } else if activated {
        // 优先取真正的激活描述行（永久/已激活/activated），
        // 避开 slmgr 头部如「RELEASE_LICENSED状态：SLMGR…」这种误命中 licensed 的行
        let line = hay
            .lines()
            .find(|l| {
                let ll = l.to_ascii_lowercase();
                l.contains("永久激活")
                    || l.contains("已激活")
                    || ll.contains("permanently activated")
                    || (ll.contains("activated") && !ll.contains("rerelease license"))
            })
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .unwrap_or("")
            .to_string();
        (
            true,
            if line.is_empty() {
                "Windows 已激活".into()
            } else {
                line
            },
        )
    } else {
        (
            false,
            format!(
                "slmgr 输出未识别：{}",
                hay.trim().chars().take(120).collect::<String>()
            ),
        )
    }
}

/// Windows 激活状态：`cscript //nologo slmgr.vbs /xpr` 输出解析。
///
/// slmgr.vbs 是写脚本，但 `/xpr` 只查询不修改（权威只读查询激活状态）。
/// 无管理员权限时 slmgr 会失败 → 如实返回「需要管理员权限」而非编造。
#[cfg(windows)]
fn win_activation() -> (bool, String) {
    use std::process::Command;
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let slmgr = format!("{sys}\\System32\\slmgr.vbs");
    if !std::path::Path::new(&slmgr).exists() {
        return (false, "未找到 slmgr.vbs（非 Windows 或精简系统）".into());
    }
    let mut cmd = Command::new("cscript");
    cmd.args(["//nologo", &slmgr, "/xpr"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output();
    match out {
        Ok(o) => {
            // cscript 输出跟随系统 ANSI 代码页：中文 Windows 上是 GBK，
            // UTF-8 解码失败时按 GBK 兜底，避免每个汉字变成  导致误判「未识别」。
            let dec = |raw: &[u8]| match std::str::from_utf8(raw) {
                Ok(s) => s.to_string(),
                Err(_) => {
                    #[cfg(windows)]
                    {
                        let (cow, _, _) = encoding_rs::GBK.decode(raw);
                        cow.into_owned()
                    }
                    #[cfg(not(windows))]
                    String::from_utf8_lossy(raw).into_owned()
                }
            };
            let text = dec(&o.stdout);
            let stderr = dec(&o.stderr);
            parse_activation_text(&format!("{text}{stderr}"))
        }
        Err(e) => (false, format!("运行 slmgr.vbs 失败：{e}")),
    }
}

/// Office 激活状态：查注册表 `OfficeClickToRun 配置键`（订阅版 vs 永久版）。
/// 只读注册表枚举，无需管理员。
#[cfg(windows)]
fn office_activation() -> String {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
    };
    // Office 许可注册表键：HKLM\SOFTWARE\Microsoft\Office\ClickToRun\Configuration
    let key = r"SOFTWARE\Microsoft\Office\ClickToRun\Configuration";
    let mut hkey: HKEY = std::ptr::null_mut();
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            wide(key).as_ptr(),
            0,
            KEY_READ,
            &mut hkey,
        )
    };
    if rc != 0 {
        return "未检测到 Office（ClickToRun 配置键不存在，可能未安装）".into();
    }
    // 读 Platform / ProductReleaseIds 判断订阅版还是永久版
    let platform = read_str(hkey, &wide("Platform"));
    if platform.is_empty() {
        // 无 ClickToRun → 尝试传统 MSI 安装键
        let legacy = r"SOFTWARE\Microsoft\Office";
        let mut lkey: HKEY = std::ptr::null_mut();
        let rc2 = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                wide(legacy).as_ptr(),
                0,
                KEY_READ,
                &mut lkey,
            )
        };
        unsafe { RegCloseKey(hkey) };
        if rc2 != 0 {
            return "未检测到 Office".into();
        }
        unsafe { RegCloseKey(lkey) };
        return "检测到传统 MSI 版 Office（激活状态需用 ospp.vbs 查询，未自动读取）".into();
    }
    // 订阅版（Microsoft 365）：ProductReleaseIds 含 'O365' 或 'ProPlus' 表示订阅
    let prod = read_str(hkey, &wide("ProductReleaseIds"));
    let is_o365 = prod.to_ascii_uppercase().contains("O365");
    let _ = unsafe { RegCloseKey(hkey) };
    if is_o365 {
        format!("Microsoft 365 订阅版（产品 ID：{prod}，激活状态由账户订阅决定）")
    } else {
        format!("Office 永久版（产品 ID：{prod}，需要 ospp 查询激活状态）")
    }
}

/// 软件许可证集合：Windows 激活 + Office 激活（只读）。
#[cfg(windows)]
pub fn collect_licenses() -> Result<String, String> {
    let (win_ok, win_txt) = win_activation();
    let office_txt = office_activation();
    let mut out = String::new();
    out.push_str("【Windows 激活】\n");
    out.push_str(&format!("- {}\n", if win_ok { "✅ " } else { "⚠️ " }));
    out.push_str(&format!("  {win_txt}\n"));
    out.push_str("【Office 激活】\n");
    out.push_str(&format!("- {office_txt}\n"));
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_licenses() -> Result<String, String> {
    Ok("当前平台不是 Windows，软件许可证不可用".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn parse_activation_text_branches() {
        // 已激活（英文 + 中文）
        let (ok, txt) =
            parse_activation_text("Windows(R), 专业版 (10586) is permanently activated.");
        assert!(ok);
        assert!(txt.contains("activated"));
        let (ok, txt) = parse_activation_text("Windows 已激活。");
        assert!(ok);
        assert!(txt.contains("激活"));
        // 中文 slmgr /xpr 成功输出：永久激活（无「已激活」字样）
        let (ok, txt) = parse_activation_text(
            "RELEASE_LICENSED状态：SLMGR rerelease license\nWindows(R), 专业版 (10586) 永久激活。",
        );
        assert!(ok, "中文输出「永久激活」应被识别为已激活");
        assert!(txt.contains("永久激活"));
        // 未激活
        let (ok, txt) = parse_activation_text("Windows is not activated");
        assert!(!ok);
        assert!(txt.contains("未永久激活"));
        // KMS 批量授权即将过期（中文）
        let (ok, txt) = parse_activation_text(
            "Windows(R), Education edition:\r\n    批量激活将于 2026/10/2 0:21:31 过期",
        );
        assert!(!ok, "「批量激活将于…过期」应识别为未永久激活");
        assert!(txt.contains("批量激活将于"));
        // 拒绝访问
        let (ok, txt) = parse_activation_text("Access denied");
        assert!(!ok);
        assert!(txt.contains("管理员权限"));
        // 未识别
        let (ok, txt) = parse_activation_text("??? 乱码输出 ???");
        assert!(!ok);
        assert!(txt.contains("未识别"));
    }

    #[cfg(windows)]
    #[test]
    fn office_activation_returns_something() {
        // 不依赖真实安装：有 ClickToRun 键 → 返回订阅/永久版描述；
        // 无键 → 返回「未检测到 Office」，绝不 panic。
        let s = office_activation();
        assert!(!s.is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn collect_licenses_non_windows_degrade() {
        let r = collect_licenses().unwrap();
        assert!(r.contains("不是 Windows"));
    }
}
