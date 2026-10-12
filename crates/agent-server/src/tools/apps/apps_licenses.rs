//! 软件许可证域（从 apps.rs 拆分，2026-10-12）。
//! collect_licenses（Windows / Office 激活状态）。

#[cfg(windows)]
use super::apps_enum::wide;

#[cfg(windows)]
use crate::tools::reg_read_str;

// ── 软件许可证（Windows / Office 激活状态）─────────────────────────────

/// 解析 slmgr /xpr 混输输出（stdout+stderr）→ (是否激活, 描述)。
/// 纯函数，便于单元测试；分支（顺序很重要）：
/// 1. 权限拒绝（Access denied / 0xC0000022 / 0x80070005 / 拒绝访问）
/// 2. 未激活信号（not activated / 未激活 / license is not / 0xC004F* / 0xC004E*）
/// 3. 已激活信号（activated / licensed / genuine / 已激活 / 永久）
/// 4. 其余 → 未识别
#[cfg(windows)]
pub(crate) fn parse_activation_text(hay: &str) -> (bool, String) {
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
        let line = hay.lines().find(|l| {
            let ll = l.to_ascii_lowercase();
            ll.contains("not activated")
                || ll.contains("license is not")
                || ll.contains("0xc004")
                || l.contains("未激活")
                || l.contains("批量激活")
                || l.contains("过期")
        });
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
pub(crate) fn office_activation() -> String {
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
    let platform = reg_read_str(hkey, &wide("Platform"));
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
    let prod = reg_read_str(hkey, &wide("ProductReleaseIds"));
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
