//! CPU 使用率域（从 process.rs 拆分，2026-10-12）。

use super::proc_enum::process_mem_bytes;

use crate::tools::fmt_bytes;

#[cfg(windows)]
pub fn collect_cpu_usage() -> Result<String, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // 采样窗口：200ms。取进程内核+用户时间差值 / 窗口墙钟时长 = 单核占用率。
    const SAMPLE_MS: u64 = 200;

    #[derive(Clone)]
    struct P {
        pid: u32,
        name: String,
        // 内核 + 用户时间（100ns 单位）
        cpu_100ns: u64,
        mem_bytes: u64,
    }

    // 两次快照共用的枚举逻辑（Toolhelp32 + GetProcessTimes + 内存）。
    fn snapshot() -> Vec<P> {
        let mut out = Vec::new();
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap.is_null() || snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut e: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = unsafe { Process32FirstW(snap, &mut e) };
        while ok != 0 {
            let name = String::from_utf16_lossy(
                &e.szExeFile[..e
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(e.szExeFile.len())],
            );
            let pid = e.th32ProcessID;
            let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            let mut cpu_100ns = 0u64;
            if !h.is_null() {
                let mut ct = unsafe { std::mem::zeroed::<FILETIME>() };
                let mut et = unsafe { std::mem::zeroed::<FILETIME>() };
                let mut kt = unsafe { std::mem::zeroed::<FILETIME>() };
                let mut ut = unsafe { std::mem::zeroed::<FILETIME>() };
                if unsafe { GetProcessTimes(h, &mut ct, &mut et, &mut kt, &mut ut) } != 0 {
                    let ft100 =
                        |t: FILETIME| (t.dwLowDateTime as u64) | ((t.dwHighDateTime as u64) << 32);
                    cpu_100ns = ft100(kt).saturating_add(ft100(ut));
                }
                unsafe { CloseHandle(h) };
            }
            out.push(P {
                pid,
                name,
                cpu_100ns,
                mem_bytes: process_mem_bytes(pid),
            });
            ok = unsafe { Process32NextW(snap, &mut e) };
        }
        unsafe { CloseHandle(snap) };
        out
    }

    let first = snapshot();
    std::thread::sleep(std::time::Duration::from_millis(SAMPLE_MS));
    let second = snapshot();

    if first.is_empty() || second.is_empty() {
        return Err("无法枚举进程（非 Windows 环境或权限不足）。".into());
    }

    // 按 PID 匹配差值，除以采样窗口（墙钟，100ns 单位）→ 单核占用率。
    let window_100ns = SAMPLE_MS * 10_000;
    let mut rows: Vec<(u32, String, u64, u64)> = Vec::new(); // (pid, name, cpu_pct, mem)
    for p2 in &second {
        if let Some(p1) = first.iter().find(|x| x.pid == p2.pid) {
            let delta = p2.cpu_100ns.saturating_sub(p1.cpu_100ns);
            let pct = delta.saturating_mul(100).min(window_100ns * 100) / window_100ns;
            if pct > 0 || p2.mem_bytes > 0 {
                rows.push((p2.pid, p2.name.clone(), pct, p2.mem_bytes));
            }
        }
    }
    // CPU 占用降序。
    rows.sort_by_key(|x| std::cmp::Reverse(x.2));

    let mut s = String::from("进程 CPU 占用（采样窗口 200ms，单核百分比）：\n");
    for (pid, name, pct, mem) in rows.iter().take(30) {
        s.push_str(&format!(
            "- PID {pid}：{name} · CPU {pct}% · 内存 {0}\n",
            fmt_bytes(*mem)
        ));
    }
    s.push_str(
        "\n说明：占用率为单核百分比（多核满载 = 核数 × 100%）；\
         进程生命周期内的平均值（GetProcessTimes 差值 / 采样窗口）。",
    );
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_cpu_usage() -> Result<String, String> {
    Ok("当前平台不是 Windows，进程 CPU 占用不可用".into())
}
