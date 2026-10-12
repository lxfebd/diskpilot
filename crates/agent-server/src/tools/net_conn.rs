//! TCP / UDP 连接 + PID→进程名。
//!
//! from tools/net.rs 拆分（2026-10-12）。

use serde_json::Value;

use crate::tools::net_util::{addr_port, get_int, ipv4_str, ipv6_str, json_arr_of};
#[cfg(windows)]
use crate::tools::process;
use crate::tools::v_str;

// ── 2. TCP / UDP 连接 + PID→进程名 ──────────────────────────────────────

#[cfg(windows)]
pub(crate) struct ConnRow {
    pub(crate) proto: &'static str,
    pub(crate) local: String,
    pub(crate) remote: String,
    pub(crate) state: &'static str,
    pub(crate) pid: u32,
    pub(crate) is_time_wait: bool,
}

/// `MIB_TCP_STATE` 码 → 中文 + 是否 TimeWait。
#[cfg(windows)]
pub(crate) fn mib_tcp_state_cn(state: u32) -> (&'static str, bool) {
    match state {
        1 => ("已关闭", false),
        2 => ("监听", false),
        3 => ("SYN 已发送", false),
        4 => ("SYN 已接收", false),
        5 => ("已建立", false),
        6 => ("FinWait1", false),
        7 => ("FinWait2", false),
        8 => ("CloseWait", false),
        9 => ("LastAck", false),
        10 => ("Closing", false),
        11 => ("TimeWait", true),
        12 => ("已删除", false),
        _ => ("未知", false),
    }
}

#[cfg(windows)]
pub(crate) fn collect_tcp(family: u32) -> Vec<ConnRow> {
    use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    };

    let mut size: u32 = 0;
    let rc = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            1,
            family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if rc != ERROR_INSUFFICIENT_BUFFER || size == 0 {
        return Vec::new();
    }
    let mut buf: Vec<u8> = vec![0; size as usize];
    let rc = unsafe {
        GetExtendedTcpTable(
            buf.as_mut_ptr() as *mut _,
            &mut size,
            1,
            family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if rc != ERROR_SUCCESS {
        return Vec::new();
    }

    let mut out = Vec::new();
    if family == 2 {
        let tbl = unsafe { &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID) };
        let n = tbl.dwNumEntries;
        let stride = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
        let base = tbl.table.as_ptr() as *const u8;
        for i in 0..n as usize {
            let r = unsafe { &*(base.add(i * stride) as *const MIB_TCPROW_OWNER_PID) };
            let (state, is_tw) = mib_tcp_state_cn(r.dwState);
            let remote = if state == "监听" || (r.dwRemoteAddr == 0 && r.dwRemotePort == 0) {
                "-".to_string()
            } else {
                addr_port(ipv4_str(r.dwRemoteAddr), r.dwRemotePort)
            };
            out.push(ConnRow {
                proto: "TCP",
                local: addr_port(ipv4_str(r.dwLocalAddr), r.dwLocalPort),
                remote,
                state,
                pid: r.dwOwningPid,
                is_time_wait: is_tw,
            });
        }
    } else {
        let tbl = unsafe { &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID) };
        let n = tbl.dwNumEntries;
        let stride = std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();
        let base = tbl.table.as_ptr() as *const u8;
        for i in 0..n as usize {
            let r = unsafe { &*(base.add(i * stride) as *const MIB_TCP6ROW_OWNER_PID) };
            let (state, is_tw) = mib_tcp_state_cn(r.dwState);
            let is_zero_remote = r.ucRemoteAddr == [0u8; 16] && r.dwRemotePort == 0;
            let remote = if state == "监听" || is_zero_remote {
                "-".to_string()
            } else {
                addr_port(ipv6_str(&r.ucRemoteAddr), r.dwRemotePort)
            };
            out.push(ConnRow {
                proto: "TCP",
                local: addr_port(ipv6_str(&r.ucLocalAddr), r.dwLocalPort),
                remote,
                state,
                pid: r.dwOwningPid,
                is_time_wait: is_tw,
            });
        }
    }
    out
}

#[cfg(windows)]
pub(crate) fn collect_udp(family: u32) -> Vec<ConnRow> {
    use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedUdpTable, MIB_UDP6ROW_OWNER_PID, MIB_UDP6TABLE_OWNER_PID, MIB_UDPROW_OWNER_PID,
        MIB_UDPTABLE_OWNER_PID, UDP_TABLE_OWNER_PID,
    };

    let mut size: u32 = 0;
    let rc = unsafe {
        GetExtendedUdpTable(
            std::ptr::null_mut(),
            &mut size,
            1,
            family,
            UDP_TABLE_OWNER_PID,
            0,
        )
    };
    if rc != ERROR_INSUFFICIENT_BUFFER || size == 0 {
        return Vec::new();
    }
    let mut buf: Vec<u8> = vec![0; size as usize];
    let rc = unsafe {
        GetExtendedUdpTable(
            buf.as_mut_ptr() as *mut _,
            &mut size,
            1,
            family,
            UDP_TABLE_OWNER_PID,
            0,
        )
    };
    if rc != ERROR_SUCCESS {
        return Vec::new();
    }

    let mut out = Vec::new();
    if family == 2 {
        let tbl = unsafe { &*(buf.as_ptr() as *const MIB_UDPTABLE_OWNER_PID) };
        let n = tbl.dwNumEntries;
        let stride = std::mem::size_of::<MIB_UDPROW_OWNER_PID>();
        let base = tbl.table.as_ptr() as *const u8;
        for i in 0..n as usize {
            let r = unsafe { &*(base.add(i * stride) as *const MIB_UDPROW_OWNER_PID) };
            out.push(ConnRow {
                proto: "UDP",
                local: addr_port(ipv4_str(r.dwLocalAddr), r.dwLocalPort),
                remote: "-".to_string(),
                state: "无连接状态",
                pid: r.dwOwningPid,
                is_time_wait: false,
            });
        }
    } else {
        let tbl = unsafe { &*(buf.as_ptr() as *const MIB_UDP6TABLE_OWNER_PID) };
        let n = tbl.dwNumEntries;
        let stride = std::mem::size_of::<MIB_UDP6ROW_OWNER_PID>();
        let base = tbl.table.as_ptr() as *const u8;
        for i in 0..n as usize {
            let r = unsafe { &*(base.add(i * stride) as *const MIB_UDP6ROW_OWNER_PID) };
            out.push(ConnRow {
                proto: "UDP",
                local: addr_port(ipv6_str(&r.ucLocalAddr), r.dwLocalPort),
                remote: "-".to_string(),
                state: "无连接状态",
                pid: r.dwOwningPid,
                is_time_wait: false,
            });
        }
    }
    out
}

/// PID → 进程名。`process.rs` 没有单条 `process_name_of`，
/// 用 `list_processes` 全量枚举建 HashMap（一次调用，不在热路径反复快照）。
#[cfg(windows)]
pub(crate) fn process_name_map() -> std::collections::HashMap<u32, String> {
    process::list_processes(20_000)
        .into_iter()
        .map(|p| (p.pid, p.name))
        .collect()
}

/// 远端是否回环（127.x.x.x / ::1）。
#[cfg(windows)]
pub(crate) fn is_loopback_remote(remote: &str) -> bool {
    remote.starts_with("127.") || remote.starts_with("[::1]") || remote == "::1"
}

/// TCP/UDP 连接清单，含 PID→进程名。
/// - `pid`：指定 PID 则只列该进程的连接；`None` = 全量。
/// - `keyword`：按进程名 / 本地端点 / 远端端点 / PID 过滤（大小写不敏感）；`None` = 不过滤。
/// - `top_n`：默认 30，上限 100；`0` 视为「用默认值」。
/// - 默认过滤 `TimeWait`（约占 43%）与回环对端；`Listen` 行与 UDP 全部保留。
#[cfg(windows)]
pub fn collect_net_connections(
    pid: Option<u32>,
    keyword: Option<String>,
    top_n: usize,
) -> Result<String, String> {
    let limit = if top_n < 1 { 30 } else { top_n.min(100) };
    let kw = keyword.as_deref().unwrap_or("").trim().to_ascii_lowercase();

    let mut rows: Vec<ConnRow> = Vec::new();
    rows.extend(collect_tcp(2));
    rows.extend(collect_tcp(23));
    rows.extend(collect_udp(2));
    rows.extend(collect_udp(23));

    if rows.is_empty() {
        return ps_connections_fallback(pid, &kw, limit);
    }

    let names = process_name_map();
    let before = rows.len();

    let mut kept: Vec<ConnRow> = rows
        .into_iter()
        .filter(|r| !r.is_time_wait)
        // Listen 行与 UDP 本地监听（含 0.0.0.0 / :: 通配）全部保留；
        // 其余连接过滤回环对端
        .filter(|r| {
            if r.state == "监听" || r.proto == "UDP" || r.remote == "-" {
                true
            } else {
                !is_loopback_remote(&r.remote)
            }
        })
        .filter(|r| match pid {
            Some(p) => r.pid == p,
            None => true,
        })
        .filter(|r| {
            if kw.is_empty() {
                return true;
            }
            let pname = names
                .get(&r.pid)
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            pname.contains(&kw)
                || r.local.to_ascii_lowercase().contains(&kw)
                || r.remote.to_ascii_lowercase().contains(&kw)
                || r.pid.to_string().contains(&kw)
        })
        .collect();

    // 稳定排序：先已建立 → 监听 → 其余，再按协议/本地端点/状态
    kept.sort_by(|a, b| {
        let rank = |s: &str| -> u8 {
            match s {
                "已建立" => 0,
                "监听" => 1,
                _ => 2,
            }
        };
        rank(a.state)
            .cmp(&rank(b.state))
            .then_with(|| a.proto.cmp(b.proto))
            .then_with(|| a.local.cmp(&b.local))
            .then_with(|| a.state.cmp(b.state))
    });

    let total = kept.len();
    kept.truncate(limit);

    let mut out = String::from("连接与端口归属：\n");
    let skipped = before.saturating_sub(total);
    if kw.is_empty() && pid.is_none() {
        out.push_str(&format!(
            "- 已默认过滤 TimeWait 与回环对端（跳过 {skipped} 条）\n"
        ));
    } else {
        if let Some(p) = pid {
            out.push_str(&format!("- 过滤条件：PID {p}"));
            if !kw.is_empty() {
                out.push_str(&format!(" + 关键字 {kw}"));
            }
            out.push('\n');
        } else {
            out.push_str(&format!("- 过滤条件：关键字 {kw}\n"));
        }
    }
    if kept.is_empty() {
        out.push_str("- （无匹配连接）\n");
        return Ok(out);
    }
    for r in &kept {
        let pname = names
            .get(&r.pid)
            .map(|s| s.as_str())
            .unwrap_or("(未知进程)");
        out.push_str(&format!(
            "- {} {} → {} · {} · {} (PID {})\n",
            r.proto, r.local, r.remote, r.state, pname, r.pid
        ));
    }
    if total > limit {
        out.push_str(&format!(
            "- （已达 {limit} 条上限，还有 {} 条未显示）\n",
            total - limit
        ));
    }
    Ok(out)
}

#[cfg(windows)]
pub(crate) fn ps_connections_fallback(
    pid: Option<u32>,
    kw: &str,
    limit: usize,
) -> Result<String, String> {
    let script = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@()
foreach($c in (Get-NetTCPConnection -ErrorAction SilentlyContinue)){
    $p=Get-Process -Id $c.OwningProcess -ErrorAction SilentlyContinue
    $rm=if($c.State -eq 'Listen' -or $c.RemoteAddress -eq '0.0.0.0' -or $c.RemoteAddress -eq '::'){'-'}else{[string]$c.RemoteAddress + ':' + [string]$c.RemotePort}
    $out += [ordered]@{ Proto='TCP'; Local=([string]$c.LocalAddress + ':' + [string]$c.LocalPort); Remote=$rm; State=[string]$c.State; Name=if($p){[string]$p.ProcessName}else{''}; Pid=$c.OwningProcess }
}
foreach($c in (Get-NetUDPEndpoint -ErrorAction SilentlyContinue)){
    $p=Get-Process -Id $c.OwningProcess -ErrorAction SilentlyContinue
    $out += [ordered]@{ Proto='UDP'; Local=([string]$c.LocalAddress + ':' + [string]$c.LocalPort); Remote='-'; State='无连接状态'; Name=if($p){[string]$p.ProcessName}else{''}; Pid=$c.OwningProcess }
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;
    let arr = json_arr_of(script, "网络连接")?;
    let names = process_name_map();
    let mut kept: Vec<&Value> = arr
        .iter()
        .filter(|r| !v_str(r, "State").eq_ignore_ascii_case("TIMEWAIT"))
        .filter(|r| {
            let st = v_str(r, "State");
            let remote = v_str(r, "Remote");
            if st == "Listen" || v_str(r, "Proto") == "UDP" || remote == "-" {
                return true;
            }
            let rl = remote.to_ascii_lowercase();
            !(rl.starts_with("127.") || rl.starts_with("::1") || rl == "0.0.0.0")
        })
        .filter(|r| match pid {
            Some(p) => get_int(r, "Pid") == Some(p as i64),
            None => true,
        })
        .filter(|r| {
            if kw.is_empty() {
                return true;
            }
            let pname = v_str(r, "Name").to_ascii_lowercase();
            let local = v_str(r, "Local").to_ascii_lowercase();
            let remote = v_str(r, "Remote").to_ascii_lowercase();
            let pids = get_int(r, "Pid").unwrap_or(0).to_string();
            pname.contains(kw) || local.contains(kw) || remote.contains(kw) || pids.contains(kw)
        })
        .collect();

    let total = kept.len();
    kept.truncate(limit);
    let mut out = String::from("连接与端口归属（PowerShell 兜底路径）：\n");
    if kept.is_empty() {
        out.push_str("- （无匹配连接）\n");
        return Ok(out);
    }
    for r in &kept {
        let pname = v_str(r, "Name");
        let pidv = get_int(r, "Pid").unwrap_or(0);
        let disp = if pname.is_empty() {
            names
                .get(&(pidv as u32))
                .cloned()
                .unwrap_or_else(|| "(未知进程)".to_string())
        } else {
            pname
        };
        out.push_str(&format!(
            "- {} {} → {} · {} · {} (PID {})\n",
            v_str(r, "Proto"),
            v_str(r, "Local"),
            v_str(r, "Remote"),
            v_str(r, "State"),
            disp,
            pidv
        ));
    }
    if total > limit {
        out.push_str(&format!(
            "- （已达 {limit} 条上限，还有 {} 条未显示）\n",
            total - limit
        ));
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_net_connections(
    _pid: Option<u32>,
    _keyword: Option<String>,
    _top_n: usize,
) -> Result<String, String> {
    Ok("当前平台不是 Windows，网络信息不可用".into())
}
