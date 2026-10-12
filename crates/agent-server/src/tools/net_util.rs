//! 网络采集通用小工具（不依赖平台）：速率/JSON/地址/字符串转换。
//!
//! from tools/net.rs 拆分（2026-10-12），供 net_adapter/net_status/net_conn/net_speed/net_share/net_wifi 复用。

use serde_json::Value;

use crate::ps::{json_array_of, ps_capture};

// ── 通用小工具（不依赖平台）────────────────────────────────────────────
// 消费方均为 cfg(windows) 网络采集实现，Linux 下无调用方 → 豁免 dead_code。

/// 位速率（bps）人类可读：`1.2 Gbps` / `800 Mbps` / `未上报`。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn fmt_bps(bps: u64) -> String {
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
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn fmt_rate(bytes_per_sec: f64) -> String {
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

/// 可选整型：兼容 `i64`/`u64`，缺失 → `None`。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn get_int(v: &Value, k: &str) -> Option<i64> {
    v.get(k)
        .and_then(Value::as_i64)
        .or_else(|| v.get(k).and_then(Value::as_u64).map(|n| n as i64))
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn get_f64(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

/// 跑输出「数组」的 PowerShell 脚本并解析成 `Vec<Value>`
/// （`json_array_of` 兜底单元素被 `ConvertTo-Json` 折叠成对象的坑）。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn json_arr_of(script: &str, what: &str) -> Result<Vec<Value>, String> {
    let raw = ps_capture(script)?;
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let arr = json_array_of(trimmed);
    serde_json::from_str(&arr).map_err(|e| format!("解析{what}查询结果失败：{e}"))
}

/// 跑输出「对象」的脚本并解析成 `Value`（不做数组折叠）。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn json_of(script: &str, what: &str) -> Result<Value, String> {
    let raw = ps_capture(script)?;
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    serde_json::from_str(trimmed).map_err(|e| format!("解析{what}查询结果失败：{e}"))
}

/// 「无值」判定（PS 常返回 `"N/A"` 占位）。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn is_emptyish(s: &str) -> bool {
    s.is_empty() || s.eq_ignore_ascii_case("N/A")
}

// ── Windows 专用小工具 ─────────────────────────────────────────────────

#[cfg(windows)]
pub(crate) fn trim_wide(s: &[u16]) -> &[u16] {
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    &s[..end]
}

#[cfg(windows)]
pub(crate) fn wide_to_string(s: &[u16]) -> String {
    String::from_utf16_lossy(trim_wide(s))
}

/// 裸 `PWSTR` → 文本。加 512 上限防野指针越界扫内存。
#[cfg(windows)]
pub(crate) fn ptr_wide_to_string(p: *mut u16) -> String {
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
pub(crate) fn ipv4_str(addr_be: u32) -> String {
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
pub(crate) fn format_ipv6(groups: &[u16; 8]) -> String {
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
pub(crate) fn ipv6_str(bytes: &[u8; 16]) -> String {
    let groups: [u16; 8] =
        std::array::from_fn(|i| u16::from_be_bytes([bytes[i * 2], bytes[i * 2 + 1]]));
    format_ipv6(&groups)
}

/// 网络序端口（u32，高 16 位为 0）→ 十进制端口号文本。
#[cfg(windows)]
pub(crate) fn port_str(p: u32) -> String {
    u16::from_be((p & 0xFFFF) as u16).to_string()
}

/// 地址 + 端口拼接；端口为 0 时不拼（未绑定/无意义）。
#[cfg(windows)]
pub(crate) fn addr_port(addr: String, port: u32) -> String {
    if port == 0 {
        addr
    } else {
        format!("{addr}:{}", port_str(port))
    }
}

/// 把 `SOCKET_ADDRESS` 解析成点分/冒号地址文本；非 IPv4/IPv6 或空指针返回 `None`。
#[cfg(windows)]
pub(crate) fn sockaddr_to_text(
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
