//! 已安装程序清单（Windows：注册表 Uninstall 键，HKLM 64/32 + HKCU）。
//! 只读枚举，不触发 UAC（无需管理员即可读当前用户与 HKLM 公共键）。
//! 用于回答「这个软件能卸载吗 / 机器上都装了什么」。

mod apps_enum;
mod apps_licenses;
mod apps_uninstall;

pub use apps_enum::list_installed_apps;
pub use apps_licenses::collect_licenses;
pub use apps_uninstall::uninstall_app;

// 回归测试引用的许可证解析符号：office_activation / parse_activation_text
// 仅 Windows 实现，对应测试 fn 均带 cfg(windows)。
#[cfg(all(test, windows))]
pub(crate) use apps_licenses::{office_activation, parse_activation_text};

// 2026-10-12 拆分：程序枚举 → `apps_enum`，卸载助手 → `apps_uninstall`，
// 软件许可证 → `apps_licenses`。

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
