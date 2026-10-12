//! 网卡清单（原生 iphlpapi + PowerShell 兜底）。
//!
//! from tools/net.rs 拆分（2026-10-12）。collect_net_status / collect_adapter_detail 的数据来源。

use serde_json::Value;

use crate::tools::net_util::{get_int, json_arr_of, ptr_wide_to_string, sockaddr_to_text};
use crate::tools::v_str;

/// 网卡清单行（原生/PS 两条路径共用）。
#[cfg(windows)]
pub(crate) struct AdapterRow {
    pub(crate) name: String,
    pub(crate) desc: String,
    pub(crate) status: i32,
    pub(crate) speed_bps: u64, // bytes/sec * 8，0 = 未上报
    pub(crate) mac: String,
    pub(crate) ips: Vec<String>,
    pub(crate) gateway: Vec<String>,
    pub(crate) dns: Vec<String>,
    pub(crate) is_virtual: bool,
}

/// 虚拟网卡识别：名称/描述命中关键字 → 标注「（虚拟）」而非丢弃。
/// 虚拟网卡链路速率恒 0，不标注容易被误判成「网卡坏了」。
#[cfg(windows)]
pub(crate) fn is_virtual_adapter(name: &str, desc: &str) -> bool {
    let hay = format!("{} {}", name, desc).to_ascii_lowercase();
    const KEYS: [&str; 13] = [
        "hyper-v",
        "vmware",
        "vbox",
        "virtualbox",
        "bluetooth",
        "loopback",
        "vethernet",
        "virtual",
        "virtualization",
        "vpn",
        "tap-win",
        "wsl",
        "tunnel",
    ];
    KEYS.iter().any(|k| hay.contains(k))
}

/// NDIS filter 层虚拟接口识别（`GetAdaptersAddresses` 的
/// `GAA_FLAG_INCLUDE_ALL_INTERFACES` 会把网卡下的 filter 层接口也列出来，
/// 名称形如「以太网-WFP Native MAC Layer LightWeight Filter-0000」）。
/// 它们不是用户视角的网卡：无 unicast 地址、链路速率恒 `u64::MAX`，
/// 应直接跳过而不是展示。
#[cfg(windows)]
pub(crate) fn is_ndis_filter_virtual(name: &str, desc: &str, has_unicast: bool) -> bool {
    if has_unicast {
        return false;
    }
    let hay = format!("{} {}", name, desc).to_ascii_lowercase();
    const FILTER_KEYS: [&str; 8] = [
        "wfp",
        "ndis lightweight",
        "lightweight filter",
        "filter driver",
        "qos packet scheduler",
        "virtual switch extension",
        "ndis filter",
        "-0000",
    ];
    FILTER_KEYS.iter().any(|k| hay.contains(k))
}

/// 静默隐藏接口识别：Windows 的 `GetAdaptersAddresses` 会把大量用户看不见的
/// 系统虚拟接口也列出来（WAN Miniport / Teredo / 6to4 / IP-HTTPS / 内核调试器 /
/// 隐藏的 WLAN 副卡等）。它们要么已禁用、要么是隧道占位，用户视角下是一堆噪音。
/// 判定：未连接 / 未插入 且 无 IP 且 名称/描述命中特征 → 视为隐藏接口直接跳过。
#[cfg(windows)]
pub(crate) fn is_hidden_system_iface(name: &str, desc: &str, status: i32, has_ip: bool) -> bool {
    if has_ip {
        return false;
    }
    // 只有「未连接 / 未插入 / 已禁用」的静默接口才可能被折叠
    let inactive = matches!(status, 2 | 6 | 7 | 8 | 9);
    if !inactive {
        return false;
    }
    let hay = format!("{} {}", name, desc).to_ascii_lowercase();
    const HIDDEN_KEYS: [&str; 9] = [
        "wan miniport",
        "teredo",
        "6to4",
        "ip-https",
        "kernel debug",
        "内核调试",
        "isatap",
        "microsoft wi-fi direct virtual",
        "pseudo-interface",
    ];
    HIDDEN_KEYS.iter().any(|k| hay.contains(k))
}

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

/// PS 兜底：`Get-NetIPConfiguration` + `Get-NetAdapter`。普通权限可跑。
#[cfg(windows)]
pub(crate) fn ps_adapters_fallback() -> Result<Vec<AdapterRow>, String> {
    let script = r#"
# PS 输出必须 @() 强制数组，否则单网卡会被折叠成对象
$ErrorActionPreference='SilentlyContinue'
$ns4=[System.Net.Sockets.AddressFamily]::InterNetwork
$ns6=[System.Net.Sockets.AddressFamily]::InterNetworkV6
$cfg=@(Get-NetIPConfiguration | ForEach-Object {
    [ordered]@{
        Alias=[string]$_.InterfaceAlias
        Ip=@(($_.IPv4Address | ForEach-Object { [string]$_.IPAddress + '/' + [string]$_.PrefixLength }) + ($_.IPv6Address | Where-Object { $_.AddressFamily -eq $ns6 -and $_.IPAddress -ne '::' } | ForEach-Object { [string]$_.IPAddress }))
        Gateway=@((($_.IPv4DefaultGateway | Select -Expand NextHop) -join ',') -split ',' + (($_.IPv6DefaultGateway | Select -Expand NextHop) -join ','))
        Dns=@((($_.DNSServer | Where-Object { $_.AddressFamily -eq $ns4 -or $_.AddressFamily -eq $ns6 } | Select -Expand ServerAddresses) -join ','))
    }
})
$ns=@{}
foreach($c in $cfg){ if($c.Alias){ $ns[[string]$c.Alias]=$c } }
$out=@(Get-NetAdapter | ForEach-Object {
    $a=$_
    $c=$ns[[string]$a.Name]
    $mac=[string]$a.MacAddress
    $status=switch($a.Status){'Up'{1}'Disabled'{6}'Unavailable'{6}'Disconnected'{2}default{2}}
    [ordered]@{
        Name=[string]$a.Name
        Description=[string]$a.Description
        Status=$status
        LinkSpeed=if($a.LinkSpeed){
            # LinkSpeed 是带单位字符串（如 "3 Mbps" / "1 Gbps"），需解析成 Mbps 数值
            $ls=[string]$a.LinkSpeed
            $lsm=if($ls -match '([\d.]+)\s*Mbps'){ [double]$matches[1] }
                 elseif($ls -match '([\d.]+)\s*Gbps'){ [double]$matches[1]*1000 }
                 elseif($ls -match '([\d.]+)\s*Kbps'){ [double]$matches[1]/1000 }
                 else { 0 }
            [uint64]$lsm
        }else{0}
        Mac=$mac
        IsVirtual=($a.Name -match 'Hyper-V|VMware|Bluetooth|Loopback|vEthernet|Virtual|VirtualBox|VPN|TAP-Win|WSL|Tunnel' -or $a.Description -match 'Hyper-V|VMware|Bluetooth|Loopback|vEthernet|Virtual|VirtualBox|VPN|TAP-Win|WSL|Tunnel')
        Ip=if($c){@($c.Ip)}else{@()}
        Gateway=if($c){@($c.Gateway)}else{@()}
        Dns=if($c){@($c.Dns)}else{@()}
    }
})
$out | ConvertTo-Json -Depth 5 -Compress
"#;
    let arr = json_arr_of(script, "网卡清单")?;
    let mut rows = Vec::new();
    for v in arr {
        let name = v_str(&v, "Name");
        if name.is_empty() {
            continue;
        }
        let desc = v_str(&v, "Description");
        let status = get_int(&v, "Status").unwrap_or(2) as i32;
        // PS 的 LinkSpeed 已是 Mbps，转 bps（×1e6，不是 ×8）
        let link_mbps = get_int(&v, "LinkSpeed").unwrap_or(0) as u64;
        rows.push(AdapterRow {
            name,
            desc,
            status,
            speed_bps: link_mbps.saturating_mul(1_000_000),
            mac: v_str(&v, "Mac"),
            ips: json_list_to_vec(&v, "Ip"),
            gateway: json_list_to_vec(&v, "Gateway"),
            dns: json_list_to_vec(&v, "Dns"),
            is_virtual: matches!(v.get("IsVirtual"), Some(Value::Bool(true))),
        });
    }
    Ok(rows)
}

#[cfg(windows)]
pub(crate) fn json_list_to_vec(v: &Value, k: &str) -> Vec<String> {
    v.get(k)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}
