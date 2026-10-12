//! TCP/UDP 连接底层枚举域（从 net_conn.rs 拆分，2026-10-12）。
//! ConnRow / mib_tcp_state_cn / collect_tcp / collect_udp。

use crate::tools::net_util::{addr_port, ipv4_str, ipv6_str};
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
