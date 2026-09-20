//! 网络信息只读采集（Windows：原生 iphlpapi + wlanapi 首选，PowerShell 兜底；非 Windows 降级）。
//!
//! 纯工具文件，不接路由（由 `tools/mod.rs` 统一接 MCP tool 薄壳）。
//! **全部只读**：不改静态 IP、不开共享、不动防火墙、不调网口开关、不改 DHCP、
//! 不发起任何 TCP/UDP 连接；**绝不读 WiFi 密码**（不调 `WlanGetProfile`，
//! 即便技术上能取到明文）。全部走公开只读 API 或 CIM PowerShell，
//! 普通用户权限即可，不需要管理员、不触发 UAC。
//!
//! ## 本文件导出函数清单（供 `tools/mod.rs` 对接）
//! - `pub fn collect_net_status() -> Result<String, String>`
//! - `pub fn collect_net_connections(pid: Option<u32>, keyword: Option<String>, top_n: usize) -> Result<String, String>`
//! - `pub fn collect_net_speed(interval_ms: u64) -> Result<String, String>`
//! - `pub fn collect_net_share() -> Result<String, String>`
//! - `pub fn collect_net_wifi() -> Result<String, String>`
//!
//! ## 已实测核对过的坑（windows-sys 0.59.0 源码逐字段验证）
//! - **`IP_ADAPTER_ADDRESSES_LH` 是 `#[repr(C, packed(1))]`**：不能 `&*ptr` 取引用
//!   （会触发 "reference to packed field"），必须 `std::ptr::read_unaligned(ptr)` 取
//!   值拷贝后再用字段。子结构（unicast / gateway / dns）本身非 packed，可直接 `&*`。
//! - **`GetAdaptersAddresses` / `GetExtended*Table` 两次调用模式**：首调
//!   `size=0` 必须返回 `ERROR_INSUFFICIENT_BUFFER` (122)，其它值直接失败降级；
//!   不能用 `size==0` 判断「空表」（空表首调也会返回 122 并给 size = sizeof(header)）。
//! - **变长表（`table: [T; 1]` 的 C 尾数组）**：用 `table.as_ptr()` +
//!   `size_of::<T>() * i` 手工步进，**不能** `slice::from_raw_parts`（长度越界 UB）。
//!   `MIB_TCP6TABLE_OWNER_PID` / `MIB_UDP6TABLE_OWNER_PID` 均已导出，直接用。
//! - **端口是 u32 且为网络字节序**：`dwLocalPort` 高 16 位为 0，必须
//!   `u16::from_be((x & 0xFFFF) as u16)`；直接 `x as u16` 会字节序全错。
//! - **IPv4 地址是 u32 网络序**：`u32::from_be` 再转点分十进制。
//!   IPv6 在 `ucLocalAddr: [u8; 16]`（TCP6/UDP6）或 `IN6_ADDR.u.Byte`。
//! - **`SOCKADDR_IN.sin_port` / `SOCKADDR_IN6.sin6_port` 都是网络序**；
//!   `IN_ADDR.S_un.S_addr` 直接可当 u32 用；`SOCKADDR` 本体只有 16 字节
//!   （比 `SOCKADDR_IN6` 的 28 字节小），必须先读 `sa_family` 再按类型 cast。
//! - **`TransmitLinkSpeed` / `ReceiveLinkSpeed` 单位是 bytes/sec**（0 = 未报告），
//!   展示乘 8 转 bps。坑：把 `104857600`（100 MB/s = 800 Mbps，1G 口协商值）
//!   当 Mbps 直接显示会差 8 倍。
//! - **`GetExtendedTcpTable` 的 `ulaf` 参数是 `AF_INET`(2) / `AF_INET6`(23)**，
//!   不是 0；`tableclass` 是 i32，需 `as u32` 传参。
//! - **`WlanOpenHandle` 需 `WLAN_CLIENT_VERSION_2`（传 2）**；返回 0 表示成功。
//!   返回 `1168` (`ERROR_NOT_FOUND`) = 本机无 WiFi 适配器，属正常情况。
//! - **`WLAN_INTERFACE_STATE` / `WLAN_INTF_OPCODE` 无命名常量**：按 `wlan.h` 内联
//!   （STATE 0..=7；`GET_CURRENT_SETTINGS` = 11）。`WLAN_INTERFACE_CURRENT_SETTINGS`
//!   也未导出，用字节偏移解析（`dot11Ssid` 在偏移 5，`ucSSID` 在偏移 6）。
//! - **`WlanGetProfileList` 的返回内存必须 `WlanFreeMemory` 释放**（常见泄漏点）。
//! - **`MIB_TCP_STATE` 11 = TIME_WAIT** 约占连接表 43%，默认过滤；
//!   `Listen` 行与 UDP 的本地监听（含 `0.0.0.0` / `::` 通配）全部保留。
//! - **`Get-SmbShare` 普通权限可用**；`Get-SmbSession` / `net session` 普通权限必拒
//!   （Access Denied / 错误 5），必须 `try/catch` 后降级为中文说明，不能把 Err 抛上去。
//! - **`Get-NetAdapterStatistics` 字段是 `ReceivedBytes` / `SentBytes`**（不是 `BytesReceived`）。
//! - **PS 原始字符串 `r#"..."#` 内不得出现 `"#` 序列**；动态参数用 `__MS__` 占位后
//!   `replace` 注入，不用 `{SPEED_PS = ...}` 内联捕获。
//! - **rustc 1.98 拒绝 `{:.1f}`**：全部浮点用 `{:.*}`（`format!("{:.*}", 2, v)`）。

use serde_json::Value;

use crate::ps::{json_array_of, ps_capture};
#[cfg(windows)]
use crate::tools::process;

// ── 通用小工具（不依赖平台）────────────────────────────────────────────

/// 字节数人类可读。
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

/// 位速率（bps）人类可读：`1.2 Gbps` / `800 Mbps` / `未上报`。
fn fmt_bps(bps: u64) -> String {
    if bps == 0 {
        return "未上报".into();
    }
    let v = bps as f64;
    if v >= 1_000_000_000.0 {
        format!("{:.*} Gbps", 1, v / 1_000_000_000.0)
    } else if v >= 1_000_000.0 {
        format!("{:.*} Mbps", 0, v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.*} Kbps", 0, v / 1_000.0)
    } else {
        format!("{bps} bps")
    }
}

/// 字节/秒人类可读：`1.23 MB/s` / `340 KB/s` / `0 B/s`。
fn fmt_rate(bytes_per_sec: f64) -> String {
    if bytes_per_sec <= 0.0 {
        return "0 B/s".into();
    }
    let v = bytes_per_sec;
    if v >= 1024.0 * 1024.0 {
        format!("{:.*} MB/s", 2, v / 1024.0 / 1024.0)
    } else if v >= 1024.0 {
        format!("{:.*} KB/s", 0, v / 1024.0)
    } else {
        format!("{:.*} B/s", 0, v)
    }
}

/// 取 JSON 字段的可读文本：数字/布尔转字符串，缺失/异常一律空串。
fn v_str(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// 可选整型：兼容 `i64`/`u64`，缺失 → `None`。
fn get_int(v: &Value, k: &str) -> Option<i64> {
    v.get(k)
        .and_then(Value::as_i64)
        .or_else(|| v.get(k).and_then(Value::as_u64).map(|n| n as i64))
}

fn get_f64(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

/// 跑输出「数组」的 PowerShell 脚本并解析成 `Vec<Value>`
/// （`json_array_of` 兜底单元素被 `ConvertTo-Json` 折叠成对象的坑）。
fn json_arr_of(script: &str, what: &str) -> Result<Vec<Value>, String> {
    let raw = ps_capture(script)?;
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let arr = json_array_of(trimmed);
    serde_json::from_str(&arr).map_err(|e| format!("解析{what}查询结果失败：{e}"))
}

/// 跑输出「对象」的脚本并解析成 `Value`（不做数组折叠）。
fn json_of(script: &str, what: &str) -> Result<Value, String> {
    let raw = ps_capture(script)?;
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    serde_json::from_str(trimmed).map_err(|e| format!("解析{what}查询结果失败：{e}"))
}

/// 「无值」判定（PS 常返回 `"N/A"` 占位）。
fn is_emptyish(s: &str) -> bool {
    s.is_empty() || s.eq_ignore_ascii_case("N/A")
}

// ── Windows 专用小工具 ─────────────────────────────────────────────────

#[cfg(windows)]
fn trim_wide(s: &[u16]) -> &[u16] {
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    &s[..end]
}

#[cfg(windows)]
fn wide_to_string(s: &[u16]) -> String {
    String::from_utf16_lossy(trim_wide(s))
}

/// 裸 `PWSTR` → 文本。加 512 上限防野指针越界扫内存。
#[cfg(windows)]
fn ptr_wide_to_string(p: *mut u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut end = 0usize;
    while end < 512 && unsafe { *p.add(end) } != 0 {
        end += 1;
    }
    if end == 0 {
        return String::new();
    }
    unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(p, end)) }
}

/// 网络序 `u32` → `a.b.c.d`。
#[cfg(windows)]
fn ipv4_str(addr_be: u32) -> String {
    let ip = u32::from_be(addr_be);
    format!(
        "{}.{}.{}.{}",
        (ip >> 24) & 0xff,
        (ip >> 16) & 0xff,
        (ip >> 8) & 0xff,
        ip & 0xff
    )
}

/// IPv6 8 组 → RFC 5952 冒号压缩文本（最长连续零段压缩成 `::`）。
#[cfg(windows)]
fn format_ipv6(groups: &[u16; 8]) -> String {
    let mut best_start = 0usize;
    let mut best_len = 0usize;
    let mut i = 0usize;
    while i < 8 {
        if groups[i] == 0 {
            let start = i;
            while i < 8 && groups[i] == 0 {
                i += 1;
            }
            if i - start > best_len {
                best_len = i - start;
                best_start = start;
            }
        } else {
            i += 1;
        }
    }
    // 只有 ≥2 段连续零才压缩（单段零直接写 0 更清晰）
    let compress = best_len >= 2;
    let render = |lo: usize, hi: usize| -> String {
        groups[lo..hi]
            .iter()
            .map(|g| format!("{g:x}"))
            .collect::<Vec<_>>()
            .join(":")
    };
    if !compress {
        return render(0, 8);
    }
    if best_start == 0 && best_len == 8 {
        return "::".to_string();
    }
    if best_start == 0 {
        return format!("::{}", render(best_start + best_len, 8));
    }
    if best_start + best_len == 8 {
        return format!("{}::", render(0, best_start));
    }
    format!(
        "{}::{}",
        render(0, best_start),
        render(best_start + best_len, 8)
    )
}

#[cfg(windows)]
fn ipv6_str(bytes: &[u8; 16]) -> String {
    let groups: [u16; 8] =
        std::array::from_fn(|i| u16::from_be_bytes([bytes[i * 2], bytes[i * 2 + 1]]));
    format_ipv6(&groups)
}

/// 网络序端口（u32，高 16 位为 0）→ 十进制端口号文本。
#[cfg(windows)]
fn port_str(p: u32) -> String {
    u16::from_be((p & 0xFFFF) as u16).to_string()
}

/// 地址 + 端口拼接；端口为 0 时不拼（未绑定/无意义）。
#[cfg(windows)]
fn addr_port(addr: String, port: u32) -> String {
    if port == 0 {
        addr
    } else {
        format!("{addr}:{}", port_str(port))
    }
}

/// 把 `SOCKET_ADDRESS` 解析成点分/冒号地址文本；非 IPv4/IPv6 或空指针返回 `None`。
#[cfg(windows)]
fn sockaddr_to_text(
    sa: &windows_sys::Win32::Networking::WinSock::SOCKET_ADDRESS,
) -> Option<String> {
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, SOCKADDR_IN, SOCKADDR_IN6};
    let p = sa.lpSockaddr;
    if p.is_null() {
        return None;
    }
    // SOCKADDR 本体只有 16 字节，必须按 sa_family 判定后再 cast 读完整体
    let family = unsafe { (*p).sa_family };
    match family {
        AF_INET => {
            let sin = unsafe { &*(p as *const SOCKADDR_IN) };
            let addr = unsafe { sin.sin_addr.S_un.S_addr };
            Some(ipv4_str(addr))
        }
        AF_INET6 => {
            let sin = unsafe { &*(p as *const SOCKADDR_IN6) };
            let bytes = unsafe { &sin.sin6_addr.u.Byte };
            Some(ipv6_str(bytes))
        }
        _ => None,
    }
}

/// 网卡清单行（原生/PS 两条路径共用）。
#[cfg(windows)]
struct AdapterRow {
    name: String,
    desc: String,
    status: i32,
    speed_bps: u64, // bytes/sec * 8，0 = 未上报
    mac: String,
    ips: Vec<String>,
    gateway: Vec<String>,
    dns: Vec<String>,
    is_virtual: bool,
}

/// 虚拟网卡识别：名称/描述命中关键字 → 标注「（虚拟）」而非丢弃。
/// 虚拟网卡链路速率恒 0，不标注容易被误判成「网卡坏了」。
#[cfg(windows)]
fn is_virtual_adapter(name: &str, desc: &str) -> bool {
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
fn is_ndis_filter_virtual(name: &str, desc: &str, has_unicast: bool) -> bool {
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
fn is_hidden_system_iface(name: &str, desc: &str, status: i32, has_ip: bool) -> bool {
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
fn native_adapters() -> Option<Vec<AdapterRow>> {
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
fn ps_adapters_fallback() -> Result<Vec<AdapterRow>, String> {
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
fn json_list_to_vec(v: &Value, k: &str) -> Vec<String> {
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

/// 网卡状态 / IP / MAC / 网关 / DNS / 速率。
#[cfg(windows)]
pub fn collect_net_status() -> Result<String, String> {
    let mut rows = match native_adapters() {
        Some(r) if !r.is_empty() => r,
        _ => ps_adapters_fallback()?,
    };
    if rows.is_empty() {
        return Ok("未检测到网卡（Get-NetAdapter 返回空）".into());
    }
    for r in rows.iter_mut() {
        r.is_virtual = is_virtual_adapter(&r.name, &r.desc);
    }

    let mut out = format!("网卡状态（{} 张）：\n", rows.len());
    for r in &rows {
        let name_disp = if r.is_virtual {
            format!("{}（虚拟）", r.name)
        } else {
            r.name.clone()
        };
        let desc_part = if !r.desc.is_empty() && r.desc != r.name {
            format!("（{}）", r.desc)
        } else {
            String::new()
        };
        let status_txt = match r.status {
            1 => "已连接".to_string(),
            2 => "未连接".to_string(),
            3 => "测试中".to_string(),
            4 => "状态未知".to_string(),
            5 => "休眠".to_string(),
            6 => "未插入/已禁用".to_string(),
            7 => "下层链路断开".to_string(),
            8 => "低功率休眠".to_string(),
            9 => "暂停".to_string(),
            _ => format!("状态码 {}", r.status),
        };
        let ip_txt = if r.ips.is_empty() {
            String::from("未分配 IP")
        } else {
            format!("IP {}", r.ips.join("、"))
        };
        let mac_part = if r.mac.is_empty() {
            String::from(" · MAC 未上报（虚拟网卡常见）")
        } else {
            format!(" · MAC {}", r.mac)
        };
        let speed_part = format!(" · 速率 {}", fmt_bps(r.speed_bps));
        out.push_str(&format!(
            "- {}{} · {} · {}{}{}\n",
            name_disp, desc_part, status_txt, ip_txt, mac_part, speed_part
        ));
        let gw = if r.gateway.is_empty() {
            "未配置".to_string()
        } else {
            r.gateway.join("、")
        };
        out.push_str(&format!("  - 网关：{}\n", gw));
        let dns = if r.dns.is_empty() {
            "未配置".to_string()
        } else {
            r.dns.join("、")
        };
        out.push_str(&format!("  - DNS：{}", dns));
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_net_status() -> Result<String, String> {
    Ok("当前平台不是 Windows，网络信息不可用".into())
}

// ── 1.5 网卡明细（MAC / 子网掩码 / 网关 / DNS / 速率）───────────────────

/// IPv4 前缀长度 → 子网掩码文本（高位连续 1）。
#[cfg(windows)]
fn prefix_to_mask(prefix: u8) -> String {
    let mask: u32 = match prefix {
        0 => 0,
        32..=255 => u32::MAX,
        n => u32::MAX << (32 - n as u32),
    };
    format!(
        "{}.{}.{}.{}",
        (mask >> 24) & 0xff,
        (mask >> 16) & 0xff,
        (mask >> 8) & 0xff,
        mask & 0xff
    )
}

/// 网卡操作状态码 → 中文（与 `collect_net_status` 同一语义）。
#[cfg(windows)]
fn adapter_status_cn(status: i32) -> String {
    match status {
        1 => "已连接".to_string(),
        2 => "未连接".to_string(),
        3 => "测试中".to_string(),
        4 => "状态未知".to_string(),
        5 => "休眠".to_string(),
        6 => "未插入/已禁用".to_string(),
        7 => "下层链路断开".to_string(),
        8 => "低功率休眠".to_string(),
        9 => "暂停".to_string(),
        _ => format!("状态码 {status}"),
    }
}

/// 每张网卡的独立明细：状态/速率 + 逐 IP（IPv4 带子网掩码、IPv6 带前缀）
/// + MAC + 网关 + DNS。比 `collect_net_status` 更细，对齐 AIDA64「网络细节」。
#[cfg(windows)]
pub fn collect_adapter_detail() -> Result<String, String> {
    let mut rows = match native_adapters() {
        Some(r) if !r.is_empty() => r,
        _ => ps_adapters_fallback()?,
    };
    if rows.is_empty() {
        return Ok("未检测到网卡（Get-NetAdapter 返回空）".into());
    }
    for r in rows.iter_mut() {
        r.is_virtual = is_virtual_adapter(&r.name, &r.desc);
    }

    let mut out = format!("网卡明细（{} 张）：\n", rows.len());
    for r in &rows {
        let name_disp = if r.is_virtual {
            format!("{}（虚拟）", r.name)
        } else {
            r.name.clone()
        };
        let desc_part = if !r.desc.is_empty() && r.desc != r.name {
            format!("（{}）", r.desc)
        } else {
            String::new()
        };
        out.push_str(&format!("- {name_disp}{desc_part}\n"));
        out.push_str(&format!(
            "  状态：{} · 速率 {}\n",
            adapter_status_cn(r.status),
            fmt_bps(r.speed_bps)
        ));
        if r.ips.is_empty() {
            out.push_str("  IP：未分配\n");
        } else {
            for ip in &r.ips {
                if let Some((addr, prefix)) = ip.split_once('/') {
                    if let Ok(n) = prefix.parse::<u8>() {
                        if addr.contains(':') {
                            out.push_str(&format!("  IPv6：{addr}/{n}（前缀长度）\n"));
                        } else {
                            out.push_str(&format!(
                                "  IPv4：{addr} · 子网掩码 {}\n",
                                prefix_to_mask(n)
                            ));
                        }
                    } else {
                        out.push_str(&format!("  IP：{ip}\n"));
                    }
                } else {
                    out.push_str(&format!("  IPv6：{ip}\n"));
                }
            }
        }
        if r.mac.is_empty() {
            out.push_str("  MAC：未上报（虚拟网卡常见）\n");
        } else {
            out.push_str(&format!("  MAC：{}\n", r.mac));
        }
        out.push_str(&format!(
            "  网关：{}\n",
            if r.gateway.is_empty() {
                "未配置".to_string()
            } else {
                r.gateway.join("、")
            }
        ));
        out.push_str(&format!(
            "  DNS：{}",
            if r.dns.is_empty() {
                "未配置".to_string()
            } else {
                r.dns.join("、")
            }
        ));
        out.push('\n');
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_adapter_detail() -> Result<String, String> {
    Ok("当前平台不是 Windows，网络信息不可用".into())
}

// ── 2. TCP / UDP 连接 + PID→进程名 ──────────────────────────────────────

#[cfg(windows)]
struct ConnRow {
    proto: &'static str,
    local: String,
    remote: String,
    state: &'static str,
    pid: u32,
    is_time_wait: bool,
}

/// `MIB_TCP_STATE` 码 → 中文 + 是否 TimeWait。
#[cfg(windows)]
fn mib_tcp_state_cn(state: u32) -> (&'static str, bool) {
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
fn collect_tcp(family: u32) -> Vec<ConnRow> {
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
fn collect_udp(family: u32) -> Vec<ConnRow> {
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
fn process_name_map() -> std::collections::HashMap<u32, String> {
    process::list_processes(20_000)
        .into_iter()
        .map(|p| (p.pid, p.name))
        .collect()
}

/// 远端是否回环（127.x.x.x / ::1）。
#[cfg(windows)]
fn is_loopback_remote(remote: &str) -> bool {
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
fn ps_connections_fallback(pid: Option<u32>, kw: &str, limit: usize) -> Result<String, String> {
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

// ── 3. 实时上下行速率 ──────────────────────────────────────────────────

/// 每张网卡实时下载/上传速率。单次 PS 内双采样（`Get-NetAdapterStatistics` 字段是
/// `ReceivedBytes` / `SentBytes`），`interval_ms` 钳到 1000..=10000。
#[cfg(windows)]
pub fn collect_net_speed(interval_ms: u64) -> Result<String, String> {
    let ms = interval_ms.clamp(1000, 10000);
    let template = r#"
$ErrorActionPreference='SilentlyContinue'
$before=@{}
foreach($s in (Get-NetAdapterStatistics -ErrorAction SilentlyContinue)){
    if($s.Name){ $before[[string]$s.Name]=@([uint64]$s.ReceivedBytes,[uint64]$s.SentBytes) }
}
$start=Get-Date
Start-Sleep -Milliseconds __MS__
$elapsed=(Get-Date).Subtract($start).TotalMilliseconds/1000.0
$out=@(foreach($s in (Get-NetAdapterStatistics -ErrorAction SilentlyContinue)){
    $name=[string]$s.Name
    if(-not $name){ continue }
    $b=$before[[string]$s.Name]
    $dl=if($elapsed -gt 0){[math]::Max(0.0,[double]([uint64]$s.ReceivedBytes-[uint64]$b[0])/$elapsed)}else{0}
    $ul=if($elapsed -gt 0){[math]::Max(0.0,[double]([uint64]$s.SentBytes-[uint64]$b[1])/$elapsed)}else{0}
    [ordered]@{
        Name=$name
        Description=[string]$s.Description
        DownloadBps=$dl
        UploadBps=$ul
        TotalRecv=[uint64]$s.ReceivedBytes
        TotalSent=[uint64]$s.SentBytes
        IsVirtual=($name -match 'Hyper-V|VMware|Bluetooth|Loopback|vEthernet|Virtual|VirtualBox|VPN|TAP-Win|WSL|Tunnel' -or $s.Description -match 'Hyper-V|VMware|Bluetooth|Loopback|vEthernet|Virtual|VirtualBox|VPN|TAP-Win|WSL|Tunnel')
    }
})
$out | ConvertTo-Json -Depth 5 -Compress
"#;
    let script = template.replace("__MS__", &ms.to_string());
    let arr = json_arr_of(&script, "网卡速率")?;
    if arr.is_empty() {
        return Ok("未检测到网卡或无法读取流量计数器".into());
    }
    let mut out = format!("网卡实时速率（{} ms 采样）：\n", ms);
    let mut total_dl = 0.0f64;
    let mut total_ul = 0.0f64;
    let mut real_count = 0usize;
    for v in &arr {
        let name = v_str(v, "Name");
        if name.is_empty() {
            continue;
        }
        let desc = v_str(v, "Description");
        let label = if !desc.is_empty() && desc != name {
            format!("{}（{}）", name, desc)
        } else {
            name.clone()
        };
        if matches!(v.get("IsVirtual"), Some(Value::Bool(true))) {
            out.push_str(&format!("- {}（虚拟，无真实流量）\n", label));
            continue;
        }
        real_count += 1;
        let dl = get_f64(v, "DownloadBps").unwrap_or(0.0);
        let ul = get_f64(v, "UploadBps").unwrap_or(0.0);
        let recv = get_int(v, "TotalRecv").unwrap_or(0) as u64;
        let sent = get_int(v, "TotalSent").unwrap_or(0) as u64;
        total_dl += dl;
        total_ul += ul;
        out.push_str(&format!(
            "- {} · 下载 {} · 上传 {} · 累计收 {} / 发 {}\n",
            label,
            fmt_rate(dl),
            fmt_rate(ul),
            fmt_bytes(recv),
            fmt_bytes(sent)
        ));
    }
    if real_count > 1 {
        out.push_str(&format!(
            "- 合计 · 下载 {} · 上传 {}\n",
            fmt_rate(total_dl),
            fmt_rate(total_ul)
        ));
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_net_speed(_interval_ms: u64) -> Result<String, String> {
    Ok("当前平台不是 Windows，网络信息不可用".into())
}

// ── 4. 网络共享 ─────────────────────────────────────────────────────────

#[cfg(windows)]
const SHARE_PS: &str = r#"
# 用 @(...) 强制数组语义：否则单个共享会被 ConvertTo-Json 折叠成对象
$ErrorActionPreference='SilentlyContinue'
$shares=@(Get-SmbShare | Select-Object Name,Path,Description,IsSpecial,EnumerateOnly,ScopeName)
$sessions=$null
$sessionsErr=''
try{
    $sessions=@(Get-SmbSession -ErrorAction Stop | Select-Object ClientComputerName,ClientUserName,Dialect)
}catch{
    $sessionsErr=($_.Exception.Message)
}
[ordered]@{
    Shares=$shares
    HasSessions=if($null -ne $sessions){$true}else{$false}
    Sessions=if($null -ne $sessions){$sessions}else{@()}
    SessionsErr=$sessionsErr
} | ConvertTo-Json -Depth 6 -Compress
"#;

/// SMB 共享清单 + 会话。`Get-SmbShare` 普通权限可用；
/// 会话必拒（错误 5），`try/catch` 后降级为中文说明，不把 Err 抛上去。
#[cfg(windows)]
pub fn collect_net_share() -> Result<String, String> {
    let v = json_of(SHARE_PS, "共享文件夹")?;
    let mut out = String::new();

    let shares = v
        .get("Shares")
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    if shares.is_empty() {
        out.push_str("本机没有共享文件夹\n");
    } else {
        out.push_str(&format!("本机共享文件夹（{} 个）：\n", shares.len()));
        for s in shares {
            let name = v_str(s, "Name");
            let path = v_str(s, "Path");
            let desc = v_str(s, "Description");
            let is_special = matches!(s.get("IsSpecial"), Some(Value::Bool(true)));
            let scope = v_str(s, "ScopeName").to_ascii_uppercase();
            let tag = if is_special {
                "（特殊共享，系统保留名）"
            } else {
                "（普通共享）"
            };
            let scope_tag = match scope.as_str() {
                "LOCALHOST" => " · 仅本机访问",
                "ALL" => " · 局域网可见",
                _ => "",
            };
            let desc_part = if desc.is_empty() {
                String::new()
            } else {
                format!(" · 说明：{desc}")
            };
            out.push_str(&format!(
                "- {} → {} · {tag}{scope_tag}{desc_part}\n",
                name,
                if path.is_empty() {
                    "(无路径)"
                } else {
                    path.as_str()
                }
            ));
        }
    }

    let has_sessions = matches!(v.get("HasSessions"), Some(Value::Bool(true)));
    if has_sessions {
        let sessions = v
            .get("Sessions")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        out.push_str(&format!("SMB 会话（{} 个）：\n", sessions.len()));
        for s in sessions {
            out.push_str(&format!(
                "- {} · {} · {}\n",
                v_str(s, "ClientComputerName"),
                v_str(s, "ClientUserName"),
                v_str(s, "Dialect")
            ));
        }
    } else {
        out.push_str(
            "- 会话列表：需要管理员权限（Get-SmbSession 拒绝访问），无法获取当前有哪些远程主机连着这些共享",
        );
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_net_share() -> Result<String, String> {
    Ok("当前平台不是 Windows，网络信息不可用".into())
}

// ── 5. WiFi ─────────────────────────────────────────────────────────────

#[cfg(windows)]
const WLAN_STATUS_SUCCESS: u32 = 0;
#[cfg(windows)]
const WLAN_CLIENT_VERSION_2: u32 = 2;
// `wlan.h` 的 `WLAN_INTERFACE_STATE` 枚举（windows-sys 0.59 未导出命名常量）
#[cfg(windows)]
const WLAN_IS_UNKNOWN: i32 = 0;
#[cfg(windows)]
const WLAN_IS_DISCONNECTED: i32 = 1;
#[cfg(windows)]
const WLAN_IS_ADHOC_CLIENT: i32 = 2;
#[cfg(windows)]
const WLAN_IS_CONNECTED: i32 = 3;
#[cfg(windows)]
const WLAN_IS_NOT_ASSOCIATED: i32 = 4;
#[cfg(windows)]
const WLAN_IS_WFD_DEVICE: i32 = 5;
#[cfg(windows)]
const WLAN_IS_WFD_CLIENT: i32 = 6;
#[cfg(windows)]
const WLAN_IS_WFD_HOST: i32 = 7;

#[cfg(windows)]
fn wifi_state_cn(s: i32) -> &'static str {
    match s {
        WLAN_IS_DISCONNECTED => "已断开",
        WLAN_IS_ADHOC_CLIENT => "AdHoc 客户端",
        WLAN_IS_CONNECTED => "已连接",
        WLAN_IS_NOT_ASSOCIATED => "未关联",
        WLAN_IS_UNKNOWN => "未知",
        WLAN_IS_WFD_DEVICE => "WFD 设备",
        WLAN_IS_WFD_CLIENT => "WFD 客户端",
        WLAN_IS_WFD_HOST => "WFD 主机",
        _ => "其它",
    }
}

#[cfg(windows)]
struct NativeWifiInfo {
    interfaces: Vec<(String, i32)>, // (接口描述, state)
    profiles: Vec<String>,
}

/// 原生路径：`WlanOpenHandle(2)` + `WlanEnumInterfaces` + `WlanGetProfileList`。
/// 无适配器返回 `None`（调用方走 PS 兜底）；有适配器但 0 接口返回空 interfaces。
#[cfg(windows)]
fn native_wifi() -> Option<NativeWifiInfo> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::NetworkManagement::WiFi::{
        WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanGetProfileList, WlanOpenHandle,
        WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST, WLAN_PROFILE_INFO, WLAN_PROFILE_INFO_LIST,
    };

    let mut ver: u32 = 0;
    let mut handle: HANDLE = std::ptr::null_mut();
    let rc = unsafe {
        WlanOpenHandle(
            WLAN_CLIENT_VERSION_2,
            std::ptr::null(),
            &mut ver,
            &mut handle,
        )
    };
    if rc != WLAN_STATUS_SUCCESS || handle.is_null() {
        return None;
    }

    let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
    let rc = unsafe { WlanEnumInterfaces(handle, std::ptr::null(), &mut list_ptr) };
    let interfaces: Vec<(String, i32)> = if rc == WLAN_STATUS_SUCCESS && !list_ptr.is_null() {
        let n = unsafe { (*list_ptr).dwNumberOfItems } as usize;
        let base = unsafe { (*list_ptr).InterfaceInfo.as_ptr() as *const u8 };
        let stride = std::mem::size_of::<WLAN_INTERFACE_INFO>();
        let mut out = Vec::new();
        for i in 0..n {
            let info = unsafe { &*(base.add(i * stride) as *const WLAN_INTERFACE_INFO) };
            let name = wide_to_string(&info.strInterfaceDescription);
            if !name.is_empty() {
                out.push((name, info.isState as i32));
            }
        }
        out
    } else {
        Vec::new()
    };

    let mut profiles: Vec<String> = Vec::new();
    let mut plist_ptr: *mut WLAN_PROFILE_INFO_LIST = std::ptr::null_mut();
    let rc =
        unsafe { WlanGetProfileList(handle, std::ptr::null(), std::ptr::null(), &mut plist_ptr) };
    if rc == WLAN_STATUS_SUCCESS && !plist_ptr.is_null() {
        let n = unsafe { (*plist_ptr).dwNumberOfItems } as usize;
        let base = unsafe { (*plist_ptr).ProfileInfo.as_ptr() as *const u8 };
        let stride = std::mem::size_of::<WLAN_PROFILE_INFO>();
        for i in 0..n {
            let info = unsafe { &*(base.add(i * stride) as *const WLAN_PROFILE_INFO) };
            let name = wide_to_string(&info.strProfileName);
            if !name.is_empty() {
                profiles.push(name);
            }
        }
        // WlanGetProfileList 的返回内存必须由 WlanFreeMemory 释放
        unsafe { WlanFreeMemory(plist_ptr as *const _) };
    }

    let _ = unsafe { WlanCloseHandle(handle, std::ptr::null()) };
    Some(NativeWifiInfo {
        interfaces,
        profiles,
    })
}

/// PS 拼接当前 SSID / 信号 / 安全类型（原生 `WLAN_ASSOCIATION_INFO` 未在 0.59 导出）。
#[cfg(windows)]
fn ps_wifi_current() -> Vec<Value> {
    let script = r#"
$ErrorActionPreference='SilentlyContinue'
Get-NetWiFiInterface | Where-Object { $_.Name } | ForEach-Object {
    [ordered]@{
        Name=[string]$_.Name
        InterfaceDescription=[string]$_.InterfaceDescription
        Ssid=[string]$_.SSID
        Signal=[int64]$_.Signal
        SecurityType=[string]$_.SecurityType
        AuthenticationType=[string]$_.AuthenticationType
    }
} | ConvertTo-Json -Depth 5 -Compress
"#;
    json_arr_of(script, "WiFi 接口").unwrap_or_default()
}

/// 已保存 profile 的安全类型：`netsh wlan show profiles` 中英文列名都含
/// "Authentication/身份验证"。只读安全类型，**不读任何密码字段**。
#[cfg(windows)]
fn ps_wifi_profile_security(profiles: &[String]) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for p in profiles {
        let escaped = p.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("netsh wlan show profiles name=\"{escaped}\"");
        let out = match ps_capture(&script) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut auth = String::new();
        let mut enc = String::new();
        for line in out.lines() {
            let lower = line.to_ascii_lowercase();
            if lower.contains("authentication") || line.contains("身份验证") {
                if let Some(idx) = line.find(':') {
                    auth = line[idx + 1..].trim().to_string();
                }
            } else if lower.contains("encryption") || line.contains("加密") {
                if let Some(idx) = line.find(':') {
                    enc = line[idx + 1..].trim().to_string();
                }
            }
        }
        if auth.is_empty() && enc.is_empty() {
            continue;
        }
        let tag = if auth.eq_ignore_ascii_case("open") || enc.eq_ignore_ascii_case("none") {
            "open".to_string()
        } else {
            "encrypted".to_string()
        };
        map.insert(p.clone(), tag);
    }
    map
}

/// WiFi 接口 + 已保存网络列表。**安全红线：绝不读密码**（不调 `WlanGetProfile`）。
/// 无无线网卡返回中文说明而非 `Err`。
#[cfg(windows)]
pub fn collect_net_wifi() -> Result<String, String> {
    let native = native_wifi();
    let current = ps_wifi_current();

    match native {
        None => {
            if !current.is_empty() {
                let mut out = String::from("无线网卡接口：\n");
                for v in &current {
                    let d = v_str(v, "InterfaceDescription");
                    let n = v_str(v, "Name");
                    let disp = if d.is_empty() { n } else { d };
                    let ssid = v_str(v, "Ssid");
                    let suffix = if is_emptyish(&ssid) {
                        String::new()
                    } else {
                        format!("（当前连接：{ssid}）")
                    };
                    out.push_str(&format!("- 接口 {disp}{suffix}\n"));
                }
                out.push_str("- 已保存网络：读取失败（原生 wlanapi 不可用）\n");
                out.push_str("- 说明：本工具只读 SSID，绝不读取或输出任何 WiFi 密码");
                return Ok(out);
            }
            Ok("本机没有无线网卡（无 WiFi 适配器）".into())
        }
        Some(info) => {
            if info.interfaces.is_empty() {
                return Ok("本机没有无线网卡（无 WiFi 适配器）".into());
            }
            let sec_map = ps_wifi_profile_security(&info.profiles);
            let mut out = String::new();
            out.push_str(&format!("无线网卡接口（{} 个）：\n", info.interfaces.len()));
            for (iface, native_state) in &info.interfaces {
                let m = current.iter().find(|v| {
                    let d = v_str(v, "InterfaceDescription");
                    let n = v_str(v, "Name");
                    d == *iface || n == *iface
                });
                let (ssid, signal, sec) = match m {
                    Some(v) => (
                        v_str(v, "Ssid"),
                        get_int(v, "Signal").unwrap_or(-1),
                        v_str(v, "SecurityType"),
                    ),
                    None => (String::new(), -1, String::new()),
                };
                out.push_str(&format!("- 接口 {iface}\n"));
                let ssid_part = if is_emptyish(&ssid) {
                    " · 未连接".to_string()
                } else {
                    format!(" · 当前连接网络：{ssid}")
                };
                out.push_str(&format!(
                    "  - 状态 {}{ssid_part}\n",
                    wifi_state_cn(*native_state)
                ));
                let mut extra = Vec::new();
                if signal >= 0 {
                    extra.push(format!("信号 {signal}%"));
                }
                if !is_emptyish(&sec) {
                    extra.push(format!("安全 {sec}"));
                }
                if !extra.is_empty() {
                    out.push_str(&format!("  - {}\n", extra.join(" · ")));
                }
            }
            if info.profiles.is_empty() {
                out.push_str("已保存网络：无\n");
            } else {
                out.push_str(&format!("已保存网络（{} 个）：\n", info.profiles.len()));
                for p in &info.profiles {
                    let tag = sec_map.get(p).map(|s| s.as_str()).unwrap_or("(未知)");
                    out.push_str(&format!("- {} · 安全 {tag}\n", p));
                }
            }
            out.push_str("- 说明：本工具只读 SSID / 安全类型，绝不读取或输出任何 WiFi 密码");
            Ok(out)
        }
    }
}

#[cfg(not(windows))]
pub fn collect_net_wifi() -> Result<String, String> {
    Ok("当前平台不是 Windows，网络信息不可用".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn prefix_to_mask_known() {
        assert_eq!(prefix_to_mask(24), "255.255.255.0");
        assert_eq!(prefix_to_mask(16), "255.255.0.0");
        assert_eq!(prefix_to_mask(8), "255.0.0.0");
        assert_eq!(prefix_to_mask(0), "0.0.0.0");
        assert_eq!(prefix_to_mask(32), "255.255.255.255");
    }

    #[test]
    #[cfg(windows)]
    fn adapter_status_cn_known() {
        assert_eq!(adapter_status_cn(1), "已连接");
        assert_eq!(adapter_status_cn(2), "未连接");
        assert_eq!(adapter_status_cn(6), "未插入/已禁用");
        assert!(adapter_status_cn(99).contains("状态码"));
    }

    #[test]
    fn collect_adapter_detail_non_windows_degrade() {
        // 非 Windows 平台降级文案（纯函数，无 PS 依赖）
        let s = collect_adapter_detail();
        assert!(s.is_ok());
    }
}
