//! 进程信息（Windows：Toolhelp32 枚举 + Psapi 内存 + QueryFullProcessImageNameW 路径；
//! 非 Windows 降级为空列表）。
//!
//! 2026-10-12 拆分：枚举/详情 → `proc_enum`，CPU 使用率 → `proc_cpu`，
//! 结束进程 → `proc_kill`，启动进程 → `proc_start`。

mod proc_cpu;
mod proc_enum;
mod proc_kill;
mod proc_start;

pub use proc_cpu::collect_cpu_usage;
pub use proc_enum::{list_processes, process_info};
pub use proc_kill::kill_process;
pub use proc_start::start_process;

// 回归测试引用的私有符号（测试模块 cfg(all(test, windows))，符号也仅 Windows 有）。
#[cfg(all(test, windows))]
pub(crate) use proc_kill::KILL_GUARD_PS;
#[cfg(all(test, windows))]
pub(crate) use proc_start::is_whitelisted_exe;

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// M2 回归：8.3 短名白名单变体必须放行（展开后命中 Program Files），
    /// 而短名逃逸目录（不在白名单的短名路径）必须拒绝。
    #[test]
    fn whitelist_expands_short_names() {
        // 真实存在：C:\PROGRA~1 → C:\Program Files
        if !std::path::Path::new("C:\\PROGRA~1").exists() {
            return; // 该机 8.3 关闭，跳过（展开路径不可验证）
        }
        // 白名单内的短名（Program Files 下真实 exe）应放行
        let exe = std::path::Path::new("C:\\PROGRA~1")
            .join("Common Files")
            .join("Microsoft Shared")
            .join("inkobj.exe");
        if exe.exists() {
            assert!(
                is_whitelisted_exe(&exe.to_string_lossy()),
                "Program Files 短名路径应放行: {}",
                exe.display()
            );
        }
        // 逃逸目录短名：指向用户目录的短名路径不应放行
        let escape = std::path::Path::new("C:\\PROGRA~1").join("..\\Users\\31672\\evil.exe");
        assert!(
            !is_whitelisted_exe(&escape.to_string_lossy()),
            "逃逸短名路径必须拒绝: {}",
            escape.display()
        );
    }

    /// M2 回归：`..` 变体归一化后必须正确判定（白名单目录内的 `..` 展开后
    /// 命中放行；逃逸出白名单目录的拒绝）。
    #[test]
    fn whitelist_dotdot_variants() {
        let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        let exe = format!("{win}\\System32\\notepad.exe");
        if !std::path::Path::new(&exe).exists() {
            return;
        }
        // C:\Windows\System32\..\System32\notepad.exe（词法展开后仍命中白名单）
        let mixed = format!("{win}\\System32\\..\\System32\\notepad.exe");
        assert!(
            is_whitelisted_exe(&mixed),
            "白名单内 .. 变体应放行: {mixed}"
        );
        // 逃逸：C:\Windows\..\Users\evil.exe（展开后不在白名单）
        let escape = format!("{win}\\..\\Users\\31672\\evil.exe");
        assert!(
            !is_whitelisted_exe(&escape),
            "逃逸出白名单的 .. 变体必须拒绝: {escape}"
        );
    }

    /// M2 回归：非 .exe 与带参数命令继续拒绝（不回归原有防线）。
    #[test]
    fn whitelist_rejects_non_exe_and_args() {
        assert!(!is_whitelisted_exe("C:\\Windows\\System32\\notepad"));
        assert!(!is_whitelisted_exe("C:\\Windows\\System32\\notepad.exe -h"));
        assert!(!is_whitelisted_exe(
            "\"C:\\Windows\\System32\\notepad.exe\""
        ));
    }

    /// 守卫脚本参数名回归：`$pid` 是 PowerShell 内置**只读**自动变量，
    /// 拿它当函数参数名会让绑定报
    /// "Cannot overwrite variable pid because it is read-only or constant"
    /// → 整个 KILL_GUARD_PS 执行失败（ps_capture 视为 Err）→ kill_process
    /// 对任何 pid 都返回"结束进程失败"（2026-10-11 实测确认的全量功能故障）。
    #[test]
    #[cfg(windows)]
    fn kill_guard_ps_never_shadows_readonly_pid() {
        assert!(
            !KILL_GUARD_PS.contains("[int]$pid"),
            "KILL_GUARD_PS 不得用 PowerShell 只读自动变量 $pid 当参数名"
        );
        assert!(
            KILL_GUARD_PS.contains("[int]$targetPid"),
            "守卫函数参数应为 $targetPid"
        );
        assert!(
            KILL_GUARD_PS.contains("blocked-pid $args[0]"),
            "调用点应把命令行首个参数传给 blocked-pid"
        );
    }

    /// 前缀边界回归：白名单目录名后跟空格/文件名变体，不得被当作命中。
    #[test]
    fn whitelist_rejects_prefix_sibling_and_spaced_variant() {
        let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        // Program Files 目录名后接空格 + 其它目录
        let spaced = format!("{}\\Program Files Evil\\evil.exe", win);
        assert!(
            !is_whitelisted_exe(&spaced),
            "Program Files 目录名后接后缀必须拒绝: {spaced}"
        );
        // System32 后接更多字符（System32x）不是目录内部
        let sibling = format!("{}\\System32x\\evil.exe", win);
        assert!(
            !is_whitelisted_exe(&sibling),
            "System32 目录名后接后缀必须拒绝: {sibling}"
        );
        // 真正的目录边界内仍放行（防误伤合法白名单命中）
        let inner = format!("{}\\System32\\notepad.exe", win);
        if std::path::Path::new(&inner).exists() {
            assert!(
                is_whitelisted_exe(&inner),
                "白名单目录内真实 exe 仍应放行: {inner}"
            );
        }
    }
}
