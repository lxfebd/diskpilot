//! 已安装程序枚举域（从 apps.rs 拆分，2026-10-12）。
//! InstalledApp / wide / enum_uninstall_key / list_installed_apps。

use serde::Serialize;

#[cfg(windows)]
use crate::tools::{reg_read_dword, reg_read_str};

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
pub(crate) fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
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
        let sys_comp = reg_read_dword(sub_key, &wide("SystemComponent")).unwrap_or(0);
        let parent = reg_read_str(sub_key, &wide("ParentKeyName"));
        let name = reg_read_str(sub_key, &wide("DisplayName"));
        if sys_comp == 0 && parent.is_empty() && !name.is_empty() {
            out.push(InstalledApp {
                version: reg_read_str(sub_key, &wide("DisplayVersion")),
                publisher: reg_read_str(sub_key, &wide("Publisher")),
                install_location: reg_read_str(sub_key, &wide("InstallLocation")),
                estimated_size_mb: reg_read_dword(sub_key, &wide("EstimatedSize"))
                    .map(|kb| (kb / 1024) as u64),
                uninstall_string: reg_read_str(sub_key, &wide("UninstallString")),
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
