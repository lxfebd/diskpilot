//! 进程枚举/详情域（从 process.rs 拆分，2026-10-12）。
//! ProcessInfo / process_exe_path / process_mem_bytes / list_processes / process_info。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub mem_bytes: u64,
    /// 进程可执行文件完整路径（取不到时为空串）
    pub exe_path: String,
    pub parent_pid: u32,
}

#[cfg(windows)]
fn process_exe_path(pid: u32) -> String {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) };
    unsafe { CloseHandle(h) };
    if ok == 0 || len == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())])
}

#[cfg(windows)]
pub(crate) fn process_mem_bytes(pid: u32) -> u64 {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return 0;
    }
    let mut pmc: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            h,
            &mut pmc,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    unsafe { CloseHandle(h) };
    if ok == 0 {
        return 0;
    }
    pmc.WorkingSetSize as u64
}

/// 枚举进程（Toolhelp32 快照），按内存占用降序（内存取不到的按 PID 兜底），
/// 可截断到 top_n。每个进程补 exe 路径与父 PID。
#[cfg(windows)]
pub fn list_processes(top_n: usize) -> Vec<ProcessInfo> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut out = Vec::new();
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        let name = String::from_utf16_lossy(
            &entry.szExeFile[..entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len())],
        );
        let pid = entry.th32ProcessID;
        out.push(ProcessInfo {
            pid,
            name,
            mem_bytes: process_mem_bytes(pid),
            exe_path: process_exe_path(pid),
            parent_pid: entry.th32ParentProcessID,
        });
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    unsafe { CloseHandle(snapshot) };
    // 内存降序；取不到的排后
    out.sort_by_key(|p| std::cmp::Reverse(p.mem_bytes));
    out.truncate(top_n.max(1));
    out
}

#[cfg(not(windows))]
pub fn list_processes(_top_n: usize) -> Vec<ProcessInfo> {
    Vec::new()
}

/// 单进程详情（指定 PID），查不到返回 None。
#[cfg(windows)]
pub fn process_info(pid: u32) -> Option<ProcessInfo> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut found = None;
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        if entry.th32ProcessID == pid {
            let name = String::from_utf16_lossy(
                &entry.szExeFile[..entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len())],
            );
            found = Some(ProcessInfo {
                pid,
                name,
                mem_bytes: process_mem_bytes(pid),
                exe_path: process_exe_path(pid),
                parent_pid: entry.th32ParentProcessID,
            });
            break;
        }
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    unsafe { CloseHandle(snapshot) };
    found
}

#[cfg(not(windows))]
pub fn process_info(_pid: u32) -> Option<ProcessInfo> {
    None
}
