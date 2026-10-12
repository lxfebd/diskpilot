//! 开机自启项枚举的 Windows 注册表/启动文件夹辅助域：Run 键枚举
//! （enum_run_key）、启动文件夹（enum_startup_folder）、环境变量展开、
//! exe token 解析、RunEntry 结构。从 sys_boot.rs 拆分（2026-10-12）。
//! 只读注册表，绝不写。

use crate::tools::sys::sys_services::wide;
/// 手写展开 `%VAR%`——`Win32_System_Environment` feature 未开，无法调 `ExpandEnvironmentStringsW`。
/// 只展开 4 个常用变量；其他 `%VAR%` 原样透传。
#[cfg(windows)]
fn expand_env_simple(s: &str) -> String {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let local_appdata = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let programfiles = std::env::var("ProgramFiles").unwrap_or_default();
    let programfiles_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();
    let systemroot = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());

    // 替换顺序：长变量名先换，避免 `%AppData%` 被 `%App` 误吃（虽然这里没 `%App`，但保险）。
    let subs: [(&str, &str); 6] = [
        ("%LocalAppData%", local_appdata.as_str()),
        ("%AppData%", appdata.as_str()),
        ("%ProgramFiles(x86)%", programfiles_x86.as_str()),
        ("%ProgramFiles%", programfiles.as_str()),
        ("%SystemRoot%", systemroot.as_str()),
        ("%USERPROFILE%", home.as_str()),
    ];
    let mut out = s.to_string();
    for (from, to) in subs {
        out = out.replace(from, to);
    }
    out
}

/// 解析命令行第一个 token 作为 exe 路径（去引号、简单 `%VAR%` 展开）。
///
/// 例：`"C:\...\svc.exe" --flag` → `C:\...\svc.exe`
///     `%SystemRoot%\System32\foo.exe` → `C:\Windows\System32\foo.exe`
#[cfg(windows)]
fn parse_exe_token(cmd: &str) -> String {
    let t = cmd.trim();
    if t.is_empty() {
        return String::new();
    }
    let first = t.split_whitespace().next().unwrap_or("").to_string();
    // 展开环境变量
    let expanded = expand_env_simple(&first);
    // 去首尾引号
    let stripped = expanded.trim_matches('"');
    stripped.trim().to_string()
}

#[cfg(windows)]
pub(crate) struct RunEntry {
    pub(crate) source: String, // 前缀标签，如 "[HKCU\Run]"
    pub(crate) name: String,   // Run 值名
    pub(crate) command: String,
    pub(crate) exe_exists: bool, // 指向的文件是否存在（false = 僵尸启动项）
}

/// 枚举一个 Run 注册表键下的所有子值（`RegEnumValueW` 递归）。
#[cfg(windows)]
pub(crate) fn enum_run_key(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
    flags: windows_sys::Win32::System::Registry::REG_SAM_FLAGS,
    prefix: &str,
    out: &mut Vec<RunEntry>,
) {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };
    let mut key: HKEY = std::ptr::null_mut();
    let rc = unsafe { RegOpenKeyExW(root, wide(subkey).as_ptr(), 0, KEY_READ | flags, &mut key) };
    if rc != 0 {
        return;
    }
    let mut idx = 0u32;
    loop {
        let mut name_buf = [0u16; 512];
        let mut name_len = name_buf.len() as u32;
        let mut type_buf = 0u32;
        let mut data_buf = [0u16; 2048];
        let mut data_len = data_buf.len() as u32;
        let r = unsafe {
            RegEnumValueW(
                key,
                idx,
                name_buf.as_mut_ptr(),
                &mut name_len,
                std::ptr::null(),
                &mut type_buf,
                data_buf.as_mut_ptr() as *mut u8,
                &mut data_len,
            )
        };
        if r != 0 {
            break;
        }
        idx += 1;
        // RegEnumValueW 的 name_len / data_len 不含 NUL，手动处理。
        let nname = (name_len as usize).min(name_buf.len());
        let name = String::from_utf16_lossy(&name_buf[..nname]);
        if name.is_empty() || name == "(Default)" {
            continue;
        }
        if type_buf != REG_SZ && type_buf != REG_EXPAND_SZ {
            continue;
        }
        let n_data = if data_len >= 2 {
            (data_len as usize / 2 - 1).min(data_buf.len())
        } else {
            0
        };
        let command_raw = String::from_utf16_lossy(&data_buf[..n_data]);
        if command_raw.trim().is_empty() {
            continue;
        }
        // REG_EXPAND_SZ 含 % 变量展开
        let expanded = if type_buf == REG_EXPAND_SZ {
            expand_env_simple(&command_raw)
        } else {
            command_raw.clone()
        };
        let exe = parse_exe_token(&expanded);
        let exists = !exe.is_empty() && std::path::Path::new(&exe).exists();
        out.push(RunEntry {
            source: prefix.to_string(),
            name: name.clone(),
            command: expanded,
            exe_exists: exists,
        });
    }
    unsafe { RegCloseKey(key) };
}

/// 枚举一个 Startup 文件夹（用户/机器），只列文件、过滤 `desktop.ini` 和隐藏。
#[cfg(windows)]
pub(crate) fn enum_startup_folder(dir: &std::path::Path, out: &mut Vec<RunEntry>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(x) => x,
        Err(_) => return,
    };
    for ent in rd.flatten() {
        let file_name = ent.file_name();
        let name_str = file_name.to_string_lossy();
        // 过滤隐藏 + desktop.ini + 目录
        if name_str.starts_with('.') {
            continue;
        }
        if name_str.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        let ft = match ent.file_type() {
            Ok(x) => x,
            Err(_) => continue,
        };
        if ft.is_dir() {
            continue;
        }
        let full = ent.path().display().to_string();
        out.push(RunEntry {
            source: "[启动文件夹]".to_string(),
            name: name_str.to_string(),
            command: full.clone(),
            exe_exists: true, // read_dir 能列出来就一定存在
        });
    }
}
