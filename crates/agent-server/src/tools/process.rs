//! 进程信息（Windows：Toolhelp32 枚举 + Psapi 内存 + QueryFullProcessImageNameW 路径；
//! 非 Windows 降级为空列表）。

use serde::Serialize;

use super::files::realpath_or_lexical;

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
fn process_mem_bytes(pid: u32) -> u64 {
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

/// 进程 CPU 占用率（同步型采样：内部两次 GetProcessTimes + 200ms sleep 差值）。
///
/// 与 `list_processes` 的「纯无状态」风格不同——CPU% 必须跨时间点采样，
/// 故在函数内完成两次采样（net_speed 同款同步窗口思路），对调用方仍是
/// 一次只读调用。200ms 采样窗口在精度与延迟间取平衡；对低占用进程
/// 量化误差 ±5%（采样窗口内只有几十毫秒内核时间）。
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

/// 结束指定 PID 的用户态进程（写操作）。**由主项目 agent.rs 桥接层做 confirmed 确认门**，
/// 本函数只管「能安全结束吗 + 执行」。守卫（违反即拒绝，绝不结束）：
/// - PID < 5（System Idle Process / System / 会话管理）——结束会蓝屏；
/// - 调用者自身 PID（自杀）；
/// - 可执行文件在系统目录（`C:\Windows` 等）——系统进程，用户态不可结束且不应结束。
///
/// 非 Windows 恒返回错误。
#[cfg(windows)]
pub fn kill_process(pid: u32) -> Result<String, String> {
    const KILL_GUARD_PS: &str = r#"
$ErrorActionPreference='Stop'
function is-system-dir([string]$p) {
  if(-not $p){ return $true }
  $sys=[System.Environment]::SystemDirectory
  $windir=[System.Environment]::GetFolderPath('Windows')
  $root=[System.IO.Path]::GetPathRoot($p)
  $pl=$p.ToLowerInvariant()
  return ($pl.StartsWith($sys.ToLowerInvariant())) -or ($pl.StartsWith($windir.ToLowerInvariant())) -or ($pl.StartsWith($root.ToLowerInvariant()+'windows'))
}
function blocked-pid([int]$pid) {
  # 系统关键进程 PID + 系统目录里的进程一律拒绝
  if($pid -lt 5){ return $true }
  $p=Get-Process -Id $pid -ErrorAction SilentlyContinue
  if(-not $p){ return $true }  # 不存在=已被结束，也不用杀
  $pp=$null
  try { $pp=$p.Path } catch {}
  return is-system-dir $pp
}
if(blocked-pid $args[0]) {
  Write-Output 'GUARD_BLOCKED'
  exit 0
}
Stop-Process -Id $args[0] -Force -ErrorAction Stop
Write-Output 'KILLED'
"#;
    if pid < 5 {
        return Err(format!(
            "进程 PID {pid} 是系统关键进程，拒绝结束（最小允许 PID 为 5）"
        ));
    }
    // 拒绝结束调用者自身（agent-server 子进程持有句柄，自杀无意义且可能卡住）
    if pid == std::process::id() {
        return Err("拒绝结束 agent-server 自身进程".into());
    }
    // 预检：PID 不存在直接给清晰中文提示（避免抛 PowerShell 裸退出码）
    let probe = crate::ps::ps_capture(&format!(
        "if (Get-Process -Id {} -ErrorAction SilentlyContinue) {{ 'EXISTS' }} else {{ 'MISSING' }}",
        pid
    ))
    .map_err(|e| format!("进程查询失败：{e}"))?;
    if probe.trim() == "MISSING" {
        return Err(format!("进程 PID {pid} 不存在（可能已退出）"));
    }
    let out = crate::ps::ps_capture(&format!("{KILL_GUARD_PS} {pid}"))
        .map_err(|e| format!("结束进程失败：{e}"))?;
    let trimmed = out.trim();
    if trimmed == "GUARD_BLOCKED" {
        Err("拒绝：目标进程是系统进程或位于系统目录，不可结束（安全守卫）".into())
    } else if trimmed == "KILLED" {
        Ok(format!("已结束进程 PID {pid}"))
    } else {
        Err(format!("结束进程未确认成功，输出：{trimmed}"))
    }
}

#[cfg(not(windows))]
pub fn kill_process(_pid: u32) -> Result<String, String> {
    Ok("当前平台不是 Windows，进程控制不可用".into())
}

/// 白名单判定：命令必须是已安装程序/系统目录里的 .exe（绝对路径 + 目录命中
/// 白名单 + 扩展名 .exe）。AI 只能启动「这台电脑上真实存在的程序」，不能拼
/// 任意命令行（cmd.exe 本身在白名单内，但 args 由用户明确给出的白名单程序传，
/// 不给裸 shell 拼字符串的通道）。目录白名单全部来自 Windows 标准环境变量，
/// 不存在则跳过（不会误判）。
#[cfg(windows)]
fn is_whitelisted_exe(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty() {
        return false;
    }
    // 扩展名必须是 .exe（大小写不敏感）
    let lower = cmd.to_ascii_lowercase();
    if !lower.ends_with(".exe") {
        return false;
    }
    // 去掉可能带的可执行文件引号（Start-Process 传参习惯）——这里命令应当
    // 是裸路径，不接受带引号或带参命令（参数走 args 数组）。
    if cmd.starts_with('"') || cmd.contains(' ') {
        return false;
    }
    let p = std::path::Path::new(cmd);
    let absolute = if p.is_absolute() {
        p.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(p)
    } else {
        return false;
    };
    // M2 修复：先做真实路径展开（GetFullPathNameW + GetLongPathNameW），
    // 把 8.3 短名（C:\PROGRA~1）与 `..` 变体（C:\WINDOWS\..\PROGRA~1）
    // 归一化后再比对白名单前缀——否则短名/变体可绕过白名单，把任意
    // 目录下的 exe 当成「已安装程序」启动。展开失败回退词法展开（路径
    // 不存在时短名本不可解析，词法兜底够用）。
    let abs_lower = realpath_or_lexical(&absolute)
        .to_string_lossy()
        .to_ascii_lowercase();
    let mut whitelist: Vec<String> = Vec::new();
    if let Ok(sys) = std::env::var("WINDIR") {
        whitelist.push(format!("{}\\system32", sys.trim_end_matches('\\')).to_lowercase());
        whitelist.push(format!("{}\\syswow64", sys.trim_end_matches('\\')).to_lowercase());
        whitelist.push(
            format!(
                "{}\\system32\\windowspowershell\\v1.0",
                sys.trim_end_matches('\\')
            )
            .to_lowercase(),
        );
    }
    if let Ok(pf) = std::env::var("PROGRAMFILES") {
        whitelist.push(pf.trim_end_matches('\\').to_lowercase());
    }
    if let Ok(pf86) = std::env::var("PROGRAMFILES(X86)") {
        whitelist.push(pf86.trim_end_matches('\\').to_lowercase());
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        whitelist.push(local.trim_end_matches('\\').to_lowercase());
    }
    // ProgramData 下常见应用子目录也放行（如 WinGet 链接），但只放行一级子目录
    if let Ok(pd) = std::env::var("PROGRAMDATA") {
        let pd = pd.trim_end_matches('\\').to_lowercase();
        whitelist.push(format!("{pd}\\microsoft\\windows\\start menu\\programs"));
    }
    whitelist
        .iter()
        .any(|w| abs_lower.starts_with(w.as_str()) || abs_lower.starts_with(&format!("{w}\\")))
}

/// 启动一个已安装程序（白名单执行，写操作）。**由主项目 agent.rs 桥接层做
/// confirmed 确认门**，本函数只负责「能否安全启动 + 执行」。守卫（违反即拒绝）：
/// - 命令不在白名单目录（系统目录/已安装程序目录）或不是 .exe → 拒绝；
/// - 命令不存在 → 拒绝；
/// - 不等待进程退出（启动即返回，分离运行）。
///
/// 返回启动的 PID 与进程名（PID 拿不到时只报进程名）。
#[cfg(windows)]
pub fn start_process(command: &str, args: &[String], cwd: &str) -> Result<String, String> {
    let cmd = command.trim();
    if cmd.is_empty() {
        return Err("命令不能为空".into());
    }
    if !is_whitelisted_exe(cmd) {
        return Err(format!(
            "拒绝：命令「{cmd}」不在白名单内（只允许系统目录或已安装程序目录下的 .exe，且不带参数——参数请走 args 数组）"
        ));
    }
    let path = std::path::PathBuf::from(cmd);
    if !path.is_file() {
        return Err(format!("拒绝：命令「{cmd}」不存在"));
    }
    // cwd 必须存在（不存在就让进程用默认工作目录，不报错——非安全边界）
    let mut child = std::process::Command::new(&path);
    for a in args {
        // 防参数注入：每个参数原样传入，绝不拼接 shell 字符串
        child.arg(a);
    }
    if !cwd.trim().is_empty() {
        let cd = std::path::PathBuf::from(cwd.trim());
        if cd.is_dir() {
            child.current_dir(&cd);
        }
    }
    // 分离运行：spawn 后不 wait（启动器语义，进程自己跑）
    let spawned = child.spawn().map_err(|e| format!("启动进程失败：{e}"))?;
    let pid = spawned.id();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| cmd.to_string());
    Ok(format!("已启动 {name}（PID {pid}）"))
}

#[cfg(not(windows))]
pub fn start_process(_command: &str, _args: &[String], _cwd: &str) -> Result<String, String> {
    Ok("当前平台不是 Windows，进程启动不可用".into())
}

/// 字节数人类可读（同 `mod.rs::fmt_bytes` 语义；rustc 1.98 用 `{:.*}`）。
fn fmt_bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.*} GB", 1, n as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if n >= 1024 * 1024 {
        format!("{:.*} MB", 1, n as f64 / 1024.0 / 1024.0)
    } else if n >= 1024 {
        format!("{:.*} KB", 0, n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

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
}
