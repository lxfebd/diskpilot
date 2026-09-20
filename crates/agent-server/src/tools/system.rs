//! 系统信息 / 运行状态（纯逻辑；Windows 原生 API，非 Windows 降级返回默认值）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    pub os: String,
    pub cpu_cores: usize,
    pub mem_total_bytes: u64,
    pub mem_used_bytes: u64,
    pub mem_percent: f64,
    pub uptime_secs: u64,
}

#[cfg(windows)]
fn os_version() -> String {
    // GetVersionEx 在 Win8.1+ 返回 6.2，故用注册表 ProductName 更准确。
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ,
    };

    const PATH: &[u16] = &[
        b'S' as u16,
        b'O' as u16,
        b'F' as u16,
        b'T' as u16,
        b'W' as u16,
        b'A' as u16,
        b'R' as u16,
        b'E' as u16,
        b'\\' as u16,
        b'M' as u16,
        b'i' as u16,
        b'c' as u16,
        b'r' as u16,
        b'o' as u16,
        b's' as u16,
        b'o' as u16,
        b'f' as u16,
        b't' as u16,
        b'\\' as u16,
        b'W' as u16,
        b'i' as u16,
        b'n' as u16,
        b'd' as u16,
        b'o' as u16,
        b'w' as u16,
        b's' as u16,
        b' ' as u16,
        b'N' as u16,
        b'T' as u16,
        b'\\' as u16,
        b'C' as u16,
        b'u' as u16,
        b'r' as u16,
        b'r' as u16,
        b'e' as u16,
        b'n' as u16,
        b't' as u16,
        b'V' as u16,
        b'e' as u16,
        b'r' as u16,
        b's' as u16,
        b'i' as u16,
        b'o' as u16,
        b'n' as u16,
        0,
    ];
    const VALUE: &[u16] = &[
        b'P' as u16,
        b'r' as u16,
        b'o' as u16,
        b'd' as u16,
        b'u' as u16,
        b'c' as u16,
        b't' as u16,
        b'N' as u16,
        b'a' as u16,
        b'm' as u16,
        b'e' as u16,
        0,
    ];
    let mut key: HKEY = std::ptr::null_mut();
    let rc = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, PATH.as_ptr(), 0, KEY_READ, &mut key) };
    if rc != 0 {
        return "Windows (unknown version)".into();
    }
    let mut buf = [0u16; 128];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            key,
            std::ptr::null(),
            VALUE.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut _,
            &mut len,
        )
    };
    unsafe { RegCloseKey(key) };
    if r == 0 && len > 0 {
        let s = OsString::from_wide(&buf[..(len as usize / 2 - 1).min(buf.len())])
            .to_string_lossy()
            .into_owned();
        if !s.is_empty() {
            return s;
        }
    }
    // 兜底：NT 版本号
    let mut info = windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<
            windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW,
        >() as u32,
        dwMajorVersion: 0,
        dwMinorVersion: 0,
        dwBuildNumber: 0,
        dwPlatformId: 0,
        szCSDVersion: [0; 128],
    };
    unsafe {
        windows_sys::Win32::System::SystemInformation::GetVersionExW(&mut info);
    }
    format!(
        "Windows {}.{}.{}",
        info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
    )
}

#[cfg(not(windows))]
fn os_version() -> String {
    std::env::consts::OS.to_string()
}

#[cfg(windows)]
fn memory_info() -> (u64, u64) {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut st: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    st.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut st) };
    if ok == 0 {
        return (0, 0);
    }
    (st.ullTotalPhys, st.ullTotalPhys - st.ullAvailPhys)
}

#[cfg(not(windows))]
fn memory_info() -> (u64, u64) {
    (0, 0)
}

#[cfg(windows)]
fn uptime_secs() -> u64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    unsafe { GetTickCount64() / 1000 }
}

#[cfg(not(windows))]
fn uptime_secs() -> u64 {
    0
}

/// 汇总系统信息。CPU 核心数用逻辑核（std::thread::available_parallelism 跨平台）。
pub fn collect_system_info() -> SystemInfo {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    let (total, used) = memory_info();
    let percent = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    SystemInfo {
        os: os_version(),
        cpu_cores: cores,
        mem_total_bytes: total,
        mem_used_bytes: used,
        mem_percent: percent,
        uptime_secs: uptime_secs(),
    }
}
