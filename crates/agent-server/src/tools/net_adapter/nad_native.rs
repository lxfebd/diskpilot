//! 原生 iphlpapi 枚举域（从 net_adapter.rs 拆分，2026-10-12）。
//! native_adapters。

use crate::tools::net_util::{ptr_wide_to_string, sockaddr_to_text};

use super::nad_ident::{is_hidden_system_iface, is_ndis_filter_virtual, AdapterRow};
// ── 1. 网卡状态 / IP / MAC / 网关 / DNS ────────────────────────────────

/// 原生路径：`GetAdaptersAddresses(AF_UNSPEC=0)` 一次拿 IPv4+IPv6 + MAC + 速率 + 前缀。
#[cfg(windows)]
pub(crate) fn native_adapters() -> Option<Vec<AdapterRow>> {
    use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_ALL_INTERFACES, GAA_FLAG_INCLUDE_GATEWAYS,
        GAA_FLAG_INCLUDE_PREFIX, GAA_FLAG_INCLUDE_TUNNEL_BINDINGORDER, IP_ADAPTER_ADDRESSES_LH,
    };

    const AF_UNSPEC: u32 = 0;
    let flags: u32 = GAA_FLAG_INCLUDE_ALL_INTERFACES
        | GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_INCLUDE_PREFIX
        | GAA_FLAG_INCLUDE_TUNNEL_BINDINGORDER;

    // 首调 size=0 拿所需大小。
    // 注意：Windows 这里可能返回 ERROR_BUFFER_OVERFLOW(111) 而非
    // ERROR_INSUFFICIENT_BUFFER(122)，两者都是「缓冲区不够、size 已填」，
    // 只要 size > 0 就继续，不能只认 122。
    let mut size: u32 = 0;
    let rc = unsafe {
        GetAdaptersAddresses(
            AF_UNSPEC,
            flags,
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if !(rc == ERROR_INSUFFICIENT_BUFFER || rc == 111/* ERROR_BUFFER_OVERFLOW */) || size == 0 {
        return None;
    }
    let mut buf: Vec<u8> = vec![0; size as usize];
    let rc = unsafe {
        GetAdaptersAddresses(
            AF_UNSPEC,
            flags,
            std::ptr::null(),
            buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
            &mut size,
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }

    let mut out = Vec::new();
    let mut cur = buf.as_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
    let buf_end = unsafe { buf.as_ptr().add(buf.len()) } as usize;
    // IP_ADAPTER_ADDRESSES_LH 是 packed，必须读值拷贝；链表步进取 Next
    let mut guard = 0usize;
    while !cur.is_null() && (cur as usize) < buf_end && guard < 256 {
        guard += 1;
        let a = unsafe { std::ptr::read_unaligned(cur) };

        let name = ptr_wide_to_string(a.FriendlyName);
        let desc = ptr_wide_to_string(a.Description);
        let display_name = if name.is_empty() { desc.clone() } else { name };

        // MAC：按 PhysicalAddressLength 截断（缓冲只有 8 字节，可能是 5 字节非标准 MAC）
        let mac_len = a.PhysicalAddressLength.min(8) as usize;
        let mac = if mac_len >= 3 {
            (0..mac_len)
                .map(|i| format!("{:02X}", a.PhysicalAddress[i]))
                .collect::<Vec<_>>()
                .join("-")
        } else {
            String::new()
        };

        // unicast 链表：IPv4 + IPv6 一起收，带上前缀长度
        let mut ips: Vec<String> = Vec::new();
        let mut p = a.FirstUnicastAddress;
        let mut uguard = 0usize;
        while !p.is_null() && (p as usize) < buf_end && uguard < 64 {
            uguard += 1;
            let un = unsafe { std::ptr::read_unaligned(p) };
            if let Some(s) = sockaddr_to_text(&un.Address) {
                // 链路本地 fe80:: 与 ::ffff: IPv4 映射地址标注，避免误导
                let txt = if s.starts_with("fe80:") {
                    format!("{s}（链路本地）")
                } else {
                    format!("{s}/{}", un.OnLinkPrefixLength)
                };
                ips.push(txt);
            }
            p = un.Next;
        }

        // 网关：全部收（多网卡常有 IPv4 + IPv6 各一个）
        let mut gateway: Vec<String> = Vec::new();
        let mut p = a.FirstGatewayAddress;
        let mut gguard = 0usize;
        while !p.is_null() && (p as usize) < buf_end && gguard < 32 {
            gguard += 1;
            let g = unsafe { std::ptr::read_unaligned(p) };
            if let Some(s) = sockaddr_to_text(&g.Address) {
                gateway.push(s);
            }
            p = g.Next;
        }

        // DNS：全部收（flags 未跳过 DNS）
        let mut dns: Vec<String> = Vec::new();
        let mut p = a.FirstDnsServerAddress;
        let mut dguard = 0usize;
        while !p.is_null() && (p as usize) < buf_end && dguard < 32 {
            dguard += 1;
            let d = unsafe { std::ptr::read_unaligned(p) };
            if let Some(s) = sockaddr_to_text(&d.Address) {
                dns.push(s);
            }
            p = d.Next;
        }

        // NDIS filter 层接口（无 IP、名称/描述命中 filter 特征）→ 跳过，不是真实网卡
        if is_ndis_filter_virtual(&display_name, &desc, !ips.is_empty()) {
            let next = a.Next;
            if next.is_null() {
                break;
            }
            cur = next;
            continue;
        }
        // 静默隐藏系统接口（WAN Miniport / Teredo / 6to4 / 内核调试器等，
        // 未连接且无 IP）→ 跳过，用户视角是噪音
        if is_hidden_system_iface(&display_name, &desc, a.OperStatus as i32, !ips.is_empty()) {
            let next = a.Next;
            if next.is_null() {
                break;
            }
            cur = next;
            continue;
        }

        // 速率：bytes/sec * 8 → bps；收发速率可能不同（全双工对称时取其一）。
        // NDIS filter 层常上报 u64::MAX（=未报告），视为 0。
        let tx = a.TransmitLinkSpeed;
        let rx = a.ReceiveLinkSpeed;
        let speed_bps = tx.max(rx).saturating_mul(8);
        let speed_bps = if speed_bps == u64::MAX { 0 } else { speed_bps };

        out.push(AdapterRow {
            name: display_name,
            desc,
            status: a.OperStatus as i32,
            speed_bps,
            mac,
            ips,
            gateway,
            dns,
            is_virtual: false,
        });

        let next = a.Next;
        if next.is_null() {
            break;
        }
        cur = next;
    }
    Some(out)
}
