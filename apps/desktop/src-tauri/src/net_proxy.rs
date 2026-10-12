//! 系统 HTTPS 代理读取与规范化（Windows 注册表 → reqwest URL）。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：读取 IE/Windows 的 `ProxyServer` 配置
//! 并归一化为 reqwest 可用的 URL。仅依赖 std/windows_sys，无 tauri 依赖。

/// Read the system HTTPS proxy if one is configured, normalized into a URL
/// reqwest can consume. Windows-only — on mac/linux reqwest already honors
/// `HTTPS_PROXY` env var by default, which is the standard convention.
#[cfg(windows)]
pub(crate) fn system_https_proxy() -> Option<String> {
    let raw = read_windows_proxy_server()?;
    parse_proxy_server(&raw)
}

#[cfg(not(windows))]
pub(crate) fn system_https_proxy() -> Option<String> {
    None
}

/// Read `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`
/// and return the `ProxyServer` REG_SZ value when `ProxyEnable` is non-zero.
/// Returns the raw string Steam-side parsing will normalize.
#[cfg(windows)]
fn read_windows_proxy_server() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
        REG_SZ,
    };

    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings\0"
        .encode_utf16()
        .collect();

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey)
            != ERROR_SUCCESS
        {
            return None;
        }

        // ProxyEnable (DWORD) — 0 = disabled.
        let mut proxy_enable: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let mut ty: u32 = 0;
        let name: Vec<u16> = "ProxyEnable\0".encode_utf16().collect();
        let res = RegQueryValueExW(
            hkey,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            &mut proxy_enable as *mut _ as *mut u8,
            &mut size,
        );
        if res != ERROR_SUCCESS || ty != REG_DWORD || proxy_enable == 0 {
            let _ = RegCloseKey(hkey);
            return None;
        }

        // ProxyServer (REG_SZ).
        let mut buf = [0u16; 512];
        let mut len = (buf.len() * 2) as u32;
        let mut ty: u32 = 0;
        let name: Vec<u16> = "ProxyServer\0".encode_utf16().collect();
        let res = RegQueryValueExW(
            hkey,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        );
        let _ = RegCloseKey(hkey);
        if res != ERROR_SUCCESS || ty != REG_SZ {
            return None;
        }
        let u16_len = (len as usize / 2).saturating_sub(1);
        let s = OsString::from_wide(&buf[..u16_len]);
        s.to_str().map(|s| s.to_string())
    }
}

/// Normalize the IE `ProxyServer` value into a single URL reqwest can use.
/// Accepts the two formats Windows writes:
///   "127.0.0.1:7890"                                 → http://127.0.0.1:7890
///   "http=127.0.0.1:7890;https=127.0.0.1:7890;..."   → pick the https= entry
#[cfg(windows)]
pub(crate) fn parse_proxy_server(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let with_scheme = |s: &str| -> String {
        if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("socks5://") {
            s.to_string()
        } else {
            format!("http://{s}")
        }
    };
    if raw.contains('=') {
        // Per-protocol form. Prefer https=, fall back to http=.
        let parts: Vec<&str> = raw.split(';').map(|p| p.trim()).collect();
        if let Some(p) = parts.iter().find_map(|p| p.strip_prefix("https=")) {
            return Some(with_scheme(p));
        }
        if let Some(p) = parts.iter().find_map(|p| p.strip_prefix("http=")) {
            return Some(with_scheme(p));
        }
        return None;
    }
    Some(with_scheme(raw))
}

#[cfg(all(test, windows))]
mod proxy_parse_tests {
    use super::parse_proxy_server;

    #[test]
    fn single_value_gets_http_scheme() {
        assert_eq!(
            parse_proxy_server("127.0.0.1:7890").as_deref(),
            Some("http://127.0.0.1:7890"),
        );
    }

    #[test]
    fn already_scheme_kept_as_is() {
        assert_eq!(
            parse_proxy_server("http://10.0.0.1:8080").as_deref(),
            Some("http://10.0.0.1:8080"),
        );
    }

    #[test]
    fn per_protocol_prefers_https() {
        assert_eq!(
            parse_proxy_server("http=127.0.0.1:7890;https=127.0.0.1:7891;ftp=127.0.0.1:7892")
                .as_deref(),
            Some("http://127.0.0.1:7891"),
        );
    }

    #[test]
    fn per_protocol_falls_back_to_http_only() {
        assert_eq!(
            parse_proxy_server("http=127.0.0.1:7890;ftp=127.0.0.1:7891").as_deref(),
            Some("http://127.0.0.1:7890"),
        );
    }

    #[test]
    fn empty_returns_none() {
        assert_eq!(parse_proxy_server(""), None);
        assert_eq!(parse_proxy_server("   "), None);
    }
}
