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
//!
//! 2026-10-12 二次拆分：服务枚举 → `sys_services`，驱动枚举 → `sys_drivers`，
//! 开机自启项 → `sys_boot`。本文件保留模块声明、NOT_WINDOWS 降级文案与回归测试。

mod sys_boot;
mod sys_drivers;
mod sys_services;

pub(crate) use sys_boot::{collect_boot_items, manage_scheduled_task};
pub(crate) use sys_drivers::collect_drivers;
pub(crate) use sys_services::{collect_services, control_service};

// 供 net_wifi.rs 与回归测试引用（PS 单引号转义，原 sys.rs 顶层 re-export 保留路径）。
#[cfg(windows)]
pub(crate) use sys_services::escape_ps_single_quoted;

// ── 非 Windows 平台统一降级文案 ────────────────────────────────────────────

#[cfg(not(windows))]
#[allow(dead_code)] // 跨平台降级文案：Windows 构建不引用
pub(crate) const NOT_WINDOWS: &str = "当前平台不是 Windows，系统信息不可用";

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

    #[test]
    fn ps_single_quote_escape_closes_literal_only() {
        // 注入防护回归：单引号必须翻倍，且翻倍后字符串仍是合法 PS 字面量。
        assert_eq!(escape_ps_single_quoted("plain"), "plain");
        assert_eq!(escape_ps_single_quoted("a'b"), "a''b");
        assert_eq!(
            escape_ps_single_quoted("'; calc.exe; '"),
            "''; calc.exe; ''"
        );
        // 真正的逃逸条件：字面量里出现奇数长度的连续引号串会让字符串提前
        // 闭合（如 `'` 单独出现、或 `'''` 剩一个单引号）。翻倍转义后任何
        // 引号串长度必须是偶数——这是「无法逃逸」的充要条件。
        for probe in ["'", "'; whoami; '", "a' ; b", "''''", "';'"] {
            let escaped = escape_ps_single_quoted(probe);
            let bytes = escaped.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'\'' {
                    let start = i;
                    while i < bytes.len() && bytes[i] == b'\'' {
                        i += 1;
                    }
                    assert!(
                        (i - start) % 2 == 0,
                        "转义后引号串长度必须为偶数（{probe:?} → {escaped:?}）"
                    );
                } else {
                    i += 1;
                }
            }
        }
    }
}
