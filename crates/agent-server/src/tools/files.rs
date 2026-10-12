//! 文件操作（只读：列目录 / 读文件 / 按文件名查找 / 目录树）—— 全部在 PathGuard 安全边界内。

use std::path::{Path, PathBuf};

#[cfg(windows)]
mod win_path {
    /// 把路径展开为 Windows 真实长路径：
    /// - GetFullPathNameW：相对路径拼接 + `.` / `..` 段规范化（不要求路径存在）；
    /// - GetLongPathNameW：把 8.3 短名段（`C:\PROGRA~1` → `C:\Program Files`）展开。
    ///
    /// AI / 用户可能以短名形态传路径（`C:\PROGRA~1\...` 或 `C:\SYSTEM~1\...`），
    /// 若守卫只做词法比对，会漏判系统目录（M1 修复）。这里对**实际存在的路径**
    /// 做真展开；无法展开（路径不存在 / 8.3 关闭）时由调用方回退词法
    /// guard_canonical（短名本身不可用，词法兜底足够）。
    pub fn expand_short_name(p: &str) -> Option<String> {
        use std::os::windows::ffi::OsStrExt;

        let wide: Vec<u16> = std::ffi::OsStr::new(p)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // 1) 绝对化 + 展开 . / ..（GetFullPathNameW 不要求路径存在）
        let full = {
            let needed = unsafe {
                GetFullPathNameW(wide.as_ptr(), 0, std::ptr::null_mut(), std::ptr::null_mut())
            };
            if needed == 0 {
                return None;
            }
            let mut buf = vec![0u16; (needed + 1) as usize];
            let n = unsafe {
                GetFullPathNameW(
                    wide.as_ptr(),
                    buf.len() as u32,
                    buf.as_mut_ptr(),
                    std::ptr::null_mut(),
                )
            };
            if n == 0 {
                return None;
            }
            buf.truncate(n as usize);
            buf
        };
        // 2) 8.3 短名 → 长名（GetLongPathNameW 要求路径真实存在）
        let needed = unsafe { GetLongPathNameW(full.as_ptr(), std::ptr::null_mut(), 0) };
        if needed == 0 {
            return None;
        }
        let mut buf = vec![0u16; (needed + 1) as usize];
        let n = unsafe { GetLongPathNameW(full.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if n == 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(String::from_utf16_lossy(&buf))
    }

    #[link(name = "kernel32")]
    extern "system" {
        #[link_name = "GetFullPathNameW"]
        fn GetFullPathNameW(
            lp_file_name: *const u16,
            n_buffer_length: u32,
            lp_buffer: *mut u16,
            lp_file_part: *mut *mut u16,
        ) -> u32;
        #[link_name = "GetLongPathNameW"]
        fn GetLongPathNameW(
            lpsz_short_path: *const u16,
            lpsz_long_path: *mut u16,
            cch_buffer: u32,
        ) -> u32;
    }
}

#[cfg(windows)]
pub fn realpath_or_lexical(p: &Path) -> PathBuf {
    win_path::expand_short_name(&p.to_string_lossy())
        .map(PathBuf::from)
        .unwrap_or_else(|| guard_canonical(p))
}

#[cfg(not(windows))]
pub fn realpath_or_lexical(p: &Path) -> PathBuf {
    guard_canonical(p)
}

mod files_guard;
mod files_lookup;
mod files_undo;

pub use files_guard::{guard_canonical, PathGuard};
pub use files_lookup::{find_files, list_dir, read_text_file};
pub use files_undo::{file_tree, recycle_file, require_confirmed};

// 回归测试引用的私有符号（路径守卫判定、撤销追加、字节统计、读取上限）。
#[cfg(test)]
pub(crate) use files_guard::{is_windows_root, GuardError};
#[cfg(test)]
pub(crate) use files_lookup::MAX_READ_BYTES;
#[cfg(test)]
pub(crate) use files_undo::{append_undo, path_bytes, UndoEntry};

// 2026-10-12 拆分：路径守卫 → `files_guard`，只读查找 → `files_lookup`，
// 目录树/撤销/回收 → `files_undo`。realpath_or_lexical 保留在本模块。

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn guard_canonical_rewrites_dot_dotdot() {
        // 变体路径 → 展开后的规范形态（纯词法，不 touch 磁盘）
        assert_eq!(guard_canonical(Path::new("C:/./")).to_string_lossy(), "C:");
        assert_eq!(guard_canonical(Path::new("C:/../")).to_string_lossy(), "C:");
        assert_eq!(
            guard_canonical(Path::new("C:/Windows/../Windows/System32")).to_string_lossy(),
            "C:\\Windows\\System32"
        );
        assert_eq!(
            guard_canonical(Path::new("C:/Windows/")).to_string_lossy(),
            "C:\\Windows"
        );
    }

    #[test]
    fn guard_canonical_is_windows_root_variants() {
        // 变体盘根应被识别为盘根
        for p in ["C:/", "C:/./", "C:/../", "C:/.././"] {
            assert!(is_windows_root(Path::new(p)), "{p} 应识别为盘根");
        }
        assert!(
            !is_windows_root(Path::new("C:/Windows")),
            "C:/Windows 不是盘根"
        );
    }

    #[test]
    fn check_for_write_rejects_system_subdir() {
        let g = PathGuard;
        // 系统目录下的文件：写操作必须拒绝（只读 check 同口径前缀匹配后同样拒绝）
        let p = Path::new("C:/Windows/System32/cmd.exe");
        assert!(
            matches!(
                g.check(p),
                Err(GuardError::SystemDir) | Err(GuardError::Missing)
            ),
            "只读 check 也应拒绝系统区子路径"
        );
        // 存在性前置：本机测试时若路径不存在会先报 Missing，属正确守卫路径；
        // 核心断言是 check_for_write 不会因"只是系统子目录"而放行
        match g.check_for_write(p) {
            Err(GuardError::Missing) | Err(GuardError::SystemDir) => {}
            Ok(()) => panic!("check_for_write 放行了系统目录内文件"),
            Err(e) => panic!("unexpected: {e}"),
        }
    }

    #[test]
    fn check_rejects_windows_root_variants() {
        let g = PathGuard;
        for p in ["C:/Windows", "C:/Windows/System32"] {
            // 系统根自身与系统区子路径 → SystemDir（只读守卫同口径前缀匹配，
            // 避免 read_file 把 C:\Windows\System32\hosts 这类系统文件内容泄漏出去）
            let r = g.check(Path::new(p));
            assert!(
                matches!(r, Err(GuardError::SystemDir)) || matches!(r, Err(GuardError::Missing)),
                "{p} check 结果异常: {r:?}"
            );
        }
    }

    /// M1 回归：8.3 短名（C:\PROGRA~1 / C:\SYSTEM~1 等）必须被展开后识别为
    /// 系统目录，不能只靠词法比对漏过去。仅在有该短名的 Windows 上断言
    /// （非 Windows 平台词法展开路径自带 `..` 处理，短名不存在，跳过）。
    #[test]
    fn system_dir_short_name_is_rejected() {
        let g = PathGuard;
        #[cfg(windows)]
        {
            // FALLBACK 名单内系统目录的 8.3 短名（均为本机实测存在的短名）：
            // PROGRA~1→Program Files / PROGRA~2→Program Files (x86) /
            // PROGRA~3→ProgramData / SYSTEM~1→System Volume Information / WIND~1→Windows
            for short in [
                "C:\\PROGRA~1",
                "C:\\PROGRA~2",
                "C:\\PROGRA~3",
                "C:\\SYSTEM~1",
                "C:\\WIND~1",
            ] {
                if !Path::new(short).exists() {
                    continue; // 该机无此短名（8.3 关闭或异名），跳过
                }
                let r = g.check(Path::new(short));
                assert!(
                    matches!(r, Err(GuardError::SystemDir)),
                    "8.3 短名 {short} 应被识别为系统目录，got: {r:?}"
                );
            }
        }
        #[cfg(not(windows))]
        {
            // 非 Windows 只需保证不会崩 + 存在性检查仍生效即可
            let r = g.check(Path::new("/System/Volume"));
            assert!(matches!(r, Ok(()) | Err(GuardError::Missing)));
        }
    }

    /// M1 回归：词法 + 真实路径组合的守卫列表——短名段与 `..` 同时出现也必须拦。
    #[test]
    fn system_dir_mixed_short_and_dotdot() {
        #[cfg(windows)]
        let g = PathGuard;
        #[cfg(windows)]
        {
            // C:\PROGRA~1\..\PROGRA~1 ⇒ C:\Program Files（展开后系统目录）
            for mixed in ["C:\\PROGRA~1\\.", "C:\\PROGRA~1\\..\\PROGRA~1"] {
                if !Path::new(&mixed.replace('\\', "/")).exists() && !Path::new(&mixed).exists() {
                    continue;
                }
                let r = g.check(Path::new(mixed));
                assert!(
                    matches!(r, Err(GuardError::SystemDir) | Err(GuardError::Missing)),
                    "混合路径 {mixed} 应被守卫拦截，got: {r:?}"
                );
            }
        }
    }

    /// M1 回归：check_for_write 对系统目录**子项**（用 8.3 短名前缀）也必须拒绝。
    #[test]
    fn check_for_write_short_name_prefix_rejected() {
        #[cfg(windows)]
        let g = PathGuard;
        #[cfg(windows)]
        {
            for short_root in ["C:\\PROGRA~1", "C:\\PROGRA~3", "C:\\SYSTEM~1"] {
                if !Path::new(short_root).exists() {
                    continue;
                }
                // 子项使用短名前缀（目录本身真实存在；文件可不存在——存在性前置
                // 会先给 Missing，但核心是绝不能放行系统目录前缀的写）
                let p = Path::new(short_root).join("anyfile.tmp");
                match g.check_for_write(&p) {
                    Err(GuardError::Missing) | Err(GuardError::SystemDir) => {}
                    Ok(()) => panic!("check_for_write 放行了系统短名前缀子项: {}", p.display()),
                    Err(e) => panic!("unexpected: {e}"),
                }
            }
        }
    }

    /// 只读守卫（check_read）语义：盘根放行、系统目录/主目录根仍拒绝。
    /// 这是「AI 列盘根」功能的安全边界——只读枚举盘根不修改任何东西，
    /// 但系统目录内容泄漏照样敏感。
    #[test]
    fn check_read_allows_drive_root_but_blocks_system() {
        #[cfg(windows)]
        let g = PathGuard;
        // 盘根：只读放行（路径不存在时按 Missing 处理，属存在性前置而非守卫拒绝）
        #[cfg(windows)]
        for root in ["C:\\", "C:", "D:\\"] {
            let r = g.check_read(Path::new(root));
            assert!(
                matches!(r, Ok(()) | Err(GuardError::Missing)),
                "只读盘根 {root} 应放行或仅报不存在，got: {r:?}"
            );
        }
        // 系统目录：只读也不放行（防系统文件内容泄漏）
        #[cfg(windows)]
        for sys in ["C:\\Windows", "C:\\Windows\\System32"] {
            if !Path::new(sys).exists() {
                continue;
            }
            let r = g.check_read(Path::new(sys));
            assert!(
                matches!(r, Err(GuardError::SystemDir)),
                "只读系统目录 {sys} 应拒绝，got: {r:?}"
            );
        }
    }

    /// 写操作确认层回归：`require_confirmed` 必须挡住「未过主进程确认门」的
    /// 直接调用（DISKPILOT_CONFIRMED 缺失），并在令牌存在时放行。
    /// 这是进程级纵深的第二道防线——第一道是主进程 `confirmed==Some(true)` 硬门，
    /// 这层兜住任意 MCP 客户端直连 agent-server 绕过主项目的路径。
    #[test]
    fn require_confirmed_guards_write_tools() {
        let prev = std::env::var_os("DISKPILOT_CONFIRMED");
        // 缺失令牌 → 拒绝（任意 MCP 客户端直连时的默认状态）
        std::env::remove_var("DISKPILOT_CONFIRMED");
        let e = require_confirmed().expect_err("无确认令牌必须拒绝");
        assert!(e.contains("confirm"), "拒绝文案应点明确认门: {e}");

        // 令牌存在（主进程已校验 confirmed=true 后注入）→ 放行
        std::env::set_var("DISKPILOT_CONFIRMED", "1");
        require_confirmed().expect("确认令牌存在应放行");

        match prev {
            Some(v) => std::env::set_var("DISKPILOT_CONFIRMED", v),
            None => std::env::remove_var("DISKPILOT_CONFIRMED"),
        }
    }

    /// undo 落盘回归：append_undo 必须把条目追加进 `DISKPILOT_UNDO_LOG`
    /// 指向的文件（纯追加，行尾换行），供主项目 list_undo 消费。
    #[test]
    fn append_undo_writes_line_to_env_log() {
        let dir = std::env::temp_dir().join(format!("dp-agent-undo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("undo.jsonl");
        let prev = std::env::var_os("DISKPILOT_UNDO_LOG");
        std::env::set_var("DISKPILOT_UNDO_LOG", &log);
        append_undo(&[UndoEntry {
            timestamp: "2026-09-20T00:00:00Z".into(),
            action: "recycle",
            source: "C:/tmp/victim.dat".into(),
            destination: None,
            reason: "AI 回收（已确认）".into(),
            bytes_freed: Some(4096),
        }]);
        append_undo(&[UndoEntry {
            timestamp: "2026-09-20T00:00:01Z".into(),
            action: "recycle",
            source: "C:/tmp/other.dat".into(),
            destination: None,
            reason: "AI 回收（已确认）".into(),
            bytes_freed: None,
        }]);
        match prev {
            Some(v) => std::env::set_var("DISKPILOT_UNDO_LOG", v),
            None => std::env::remove_var("DISKPILOT_UNDO_LOG"),
        }
        let raw = std::fs::read_to_string(&log).expect("undo 日志应存在");
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2, "应追加两行: {raw}");
        let first: serde_json::Value = serde_json::from_str(lines[0]).expect("首行应为合法 JSON");
        assert_eq!(first["action"], "recycle");
        assert_eq!(first["source"], "C:/tmp/victim.dat");
        assert_eq!(first["bytes_freed"], 4096);
        let second: serde_json::Value = serde_json::from_str(lines[1]).expect("次行应为合法 JSON");
        assert!(second.get("bytes_freed").is_none(), "None 字节应省略键");
    }

    /// read_text_file 截断回归：>256KB 文件必须在读取阶段截断（不整读进内存），
    /// truncated 标记为 true 且返回长度恰为 MAX_READ_BYTES。
    #[test]
    fn read_text_file_truncates_at_limit_without_full_read() {
        let dir = std::env::temp_dir().join(format!("dp-agent-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.txt");
        std::fs::write(&big, vec![b'a'; MAX_READ_BYTES + 4096]).unwrap();
        let (text, truncated) = read_text_file(&big).expect("读取应成功");
        assert!(truncated, "超限文件必须标记 truncated");
        assert_eq!(
            text.len(),
            MAX_READ_BYTES,
            "返回内容必须恰好截断到 MAX_READ_BYTES"
        );
        // 恰好等于上限：不截断
        let exact = dir.join("exact.txt");
        std::fs::write(&exact, vec![b'b'; MAX_READ_BYTES]).unwrap();
        let (_, truncated2) = read_text_file(&exact).expect("读取应成功");
        assert!(!truncated2, "恰好等于上限不应标记截断");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// path_bytes 统计回归：单文件返回 len，目录递归求和。
    #[test]
    fn path_bytes_counts_file_and_dir() {
        let dir = std::env::temp_dir().join(format!("dp-agent-bytes-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.dat"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("sub/b.dat"), vec![0u8; 250]).unwrap();
        assert_eq!(path_bytes(&dir.join("a.dat")), Some(100));
        assert_eq!(path_bytes(&dir), Some(350));
        assert_eq!(path_bytes(&dir.join("nope.dat")), None);
    }
}
