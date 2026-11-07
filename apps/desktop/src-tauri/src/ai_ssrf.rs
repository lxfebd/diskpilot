//! SSRF 私网目标判定（从 ai.rs 拆分，2026-10-12）。
//! blocked_private_target 供 ai_proxy / net_http / plugin_registry /
//! scaffold_registry 复用同一套「出网请求同一把尺」；IP 变体解析（十进制/
//! 八进制/十六进制/短写）对齐 WHATWG URL 语义。
/// 判断 URL 是否指向私网/环回/链路本地地址（SSRF 攻击面）。
///
/// 只按字面量解析 host：`127.0.0.1`、`localhost`、`10.x`、`172.16-31.x`、
/// `192.168.x`、`169.254.x`、`0.0.0.0`、`[::1]`、`[fc00::/7]`、`[fe80::/10]`、
/// v4-mapped `[::ffff:127.0.0.1]` 等直接拒绝；十进制/八进制/十六进制 IP 变体
/// （`2130706433`、`0177.0.0.1`、`0x7f.0.0.1`、`127.1`）也识别并拒绝。
/// 域名字面量不在私网段内的放行（不做 DNS 反查，避免代理 DNS 时被重绑定绕过）。
/// pub(crate)：插件下载（plugin_remote / plugin_registry / scaffold_registry）
/// 复用同一套私网判定，保证「所有出网请求同一把尺」。
pub(crate) fn blocked_private_target(url: &str) -> Option<&'static str> {
    let host = url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.find('/').map(|i| &rest[..i]).or(Some(rest)))
        .unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or("").trim();
    // 去端口：方括号 IPv6 取 `[...]` 内内容，普通 host 取第一个 `:` 之前
    let hostname = if host.starts_with('[') {
        host.find(']').map(|i| &host[..=i]).unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or("")
    };
    let lower = hostname
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase();
    if lower == "localhost" || lower == "::1" {
        return Some("localhost");
    }
    // IPv6 检查（含 v4-mapped / v4-compatible / 内嵌 IPv4 的形式）
    if lower.contains(':') {
        if let Some(ip) = parse_ipv6(&lower) {
            if ipv6_is_private(&ip) {
                return Some("private/loopback address");
            }
        }
        return None;
    }
    if let Some(ip) = parse_ipv4(&lower) {
        let [a, b, _, _] = ip;
        let is_private = a == 0
            || a == 10
            || (a == 127)
            || (a == 169 && b == 254)
            || (a == 172 && (16..=31).contains(&b))
            || (a == 192 && b == 168)
            || (a == 100 && (64..=127).contains(&b));
        if is_private {
            return Some("private/loopback address");
        }
    }
    None
}

/// 极简 IPv6 解析（SSRF 用）：支持 `::1`、`fc00::/7`、`fe80::/10`、
/// v4-mapped `::ffff:1.2.3.4`、内嵌 IPv4 `::127.0.0.1`。返回 16 字节
/// 大端地址；解析不出返回 None（域名/非法，交由上层处理）。
pub(crate) fn parse_ipv6(s: &str) -> Option<[u8; 16]> {
    let s = s.trim();
    if s.is_empty() || s.len() > 64 {
        return None;
    }
    // 每个 u16 段的 16-bit 值，含 v4 末段拆出的两个值
    let mut segs: Vec<u16> = Vec::new();
    // 是否出现过 `::`（出现过则整体必须能压缩到 ≤16 字节）
    let (head, tail) = match s.split_once("::") {
        Some((h, t)) => (h, t),
        None => (s, ""),
    };
    let has_compress = s.contains("::");
    let push_seg = |seg: &str, out: &mut Vec<u16>| -> Option<()> {
        if seg.contains('.') {
            let v4 = parse_ipv4(seg)?;
            out.push(((v4[0] as u16) << 8) | v4[1] as u16);
            out.push(((v4[2] as u16) << 8) | v4[3] as u16);
        } else {
            let v = u16::from_str_radix(seg, 16).ok()?;
            out.push(v);
        }
        Some(())
    };
    for seg in head.split(':').filter(|x| !x.is_empty()) {
        push_seg(seg, &mut segs)?;
    }
    if !tail.is_empty() {
        let mut tail_segs: Vec<u16> = Vec::new();
        for seg in tail.split(':').filter(|x| !x.is_empty()) {
            push_seg(seg, &mut tail_segs)?;
        }
        if has_compress {
            // 压缩：head 在前、tail 顶到末尾，中间补零
            let head_len = segs.len();
            let total = head_len + tail_segs.len();
            if total > 8 {
                return None;
            }
            // RFC 4291：`::` 必须至少压缩一个 16-bit 组——head+tail 凑满 8 段
            // 且不是纯 `::`（`::` 表示全零）时，压缩 0 段是非法地址。
            if total == 8 && !(head.is_empty() && tail.is_empty()) {
                return None;
            }
            // 直接构造最终段序列
            segs = {
                let mut all = vec![0u16; 8];
                if head_len > 0 {
                    all[..head_len].copy_from_slice(&segs);
                }
                let tail_start = 8 - tail_segs.len();
                if !tail_segs.is_empty() {
                    all[tail_start..].copy_from_slice(&tail_segs);
                }
                all
            };
        } else {
            // 无压缩但出现 head+tail（不会发生——无 :: 时 tail 必空）
            segs.extend(tail_segs);
        }
    } else if has_compress {
        // `::` 在末尾（如 `fe80::`）：head 后补零到 8 段
        if segs.len() > 8 {
            return None;
        }
        segs.resize(8, 0);
    }
    // 无压缩时必须是完整 8 段
    if !has_compress && segs.len() != 8 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, v) in segs.iter().take(8).enumerate() {
        out[i * 2] = (v >> 8) as u8;
        out[i * 2 + 1] = *v as u8;
    }
    Some(out)
}

/// IPv6 私网判定：::1 环回、fc00::/7（ULA）、fe80::/10（链路本地）、
/// ::ffff:0:0/96（v4-mapped）内嵌的 IPv4 再按 v4 私网规则判。
pub(crate) fn ipv6_is_private(ip: &[u8; 16]) -> bool {
    // ::1
    if ip[..15] == [0u8; 15] && ip[15] == 1 {
        return true;
    }
    // fc00::/7（ULA）
    if (ip[0] & 0xfe) == 0xfc {
        return true;
    }
    // fe80::/10（链路本地）
    if ip[0] == 0xfe && (ip[1] & 0xc0) == 0x80 {
        return true;
    }
    // ::ffff:0:0/96 v4-mapped：内嵌 IPv4 按 v4 私网规则判
    if ip[..10] == [0u8; 10] && ip[10] == 0xff && ip[11] == 0xff {
        let [a, b, c, d] = [ip[12], ip[13], ip[14], ip[15]];
        return ipv4_is_private(a, b, c, d);
    }
    // ::/96 v4-compatible（已废弃但仍是内网 IPv4 的映射）
    if ip[..12] == [0u8; 12] {
        return ipv4_is_private(ip[12], ip[13], ip[14], ip[15]);
    }
    false
}

/// IPv4 私网判定（0.x / 10.x / 127.x / 169.254.x / 172.16-31.x / 192.168.x / 100.64-127.x）。
pub(crate) fn ipv4_is_private(a: u8, b: u8, _c: u8, _d: u8) -> bool {
    a == 0
        || a == 10
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 100 && (64..=127).contains(&b))
}

/// 极简 IPv4 解析（SSRF 用）：`1.2.3.4` → [1,2,3,4]，非法返回 None。
/// 同时识别 WHATWG URL 解析器接受的整型变体（与 reqwest 用的 url crate 同语义，
/// 否则 `http://2130706433/` 这类会被 url crate 归一化为 127.0.0.1 而这里放行）：
/// - 十进制整数：`2130706433` = 127.0.0.1
/// - 八进制：`0177.0.0.1`、`017700000001`
/// - 十六进制：`0x7f.0.0.1`、`0x7f000001`
/// - 短写：`127.1` = 127.0.0.1（缺段从左补 0，首段之后是低位——WHATWG 语义
///   `192.168` = 192.0.0.168，不是 192.168.0.0）
pub(crate) fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let s = s.trim();
    if s.is_empty() || s.len() > 32 {
        return None;
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 4 {
        return None;
    }
    let mut out = [0u8; 4];
    // 前 parts.len()-1 段按顺序放高位字节
    for idx in 0..parts.len().saturating_sub(1) {
        let v: u64 = parse_ip_part(parts[idx])?;
        if v > 255 {
            return None;
        }
        out[idx] = v as u8;
    }
    // 末段覆盖剩余低位字节（WHATWG 语义：128.1 → 128.0.0.1）
    let last = parts.last()?;
    let first_n = parts.len() - 1;
    let low_bytes = 4 - first_n;
    let v = parse_ip_part(last)?;
    if v >= 256u64.pow(low_bytes as u32) {
        return None;
    }
    for i in 0..low_bytes {
        let shift = (low_bytes - 1 - i) as u32;
        out[first_n + i] = ((v >> (8 * shift)) & 0xff) as u8;
    }
    Some(out)
}

/// 解析单个 IPv4 数字段：十进制 / 0x 十六进制 / 0 前缀八进制，上限 u32。
pub(crate) fn parse_ip_part(part: &str) -> Option<u64> {
    let part = part.trim();
    if part.is_empty() || part.len() > 16 {
        return None;
    }
    // 前缀 0x / 0X → 十六进制
    if let Some(hex) = part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
        if hex.is_empty() || hex.len() > 8 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        return u64::from_str_radix(hex, 16).ok();
    }
    // 前导 0（且长度 > 1）→ 八进制
    if part.len() > 1 && part.starts_with('0') {
        if !part.chars().all(|c| ('0'..='7').contains(&c)) {
            return None;
        }
        return u64::from_str_radix(part.trim_start_matches('0'), 8)
            .ok()
            .or(Some(0)); // 全 0 的情况
    }
    part.parse::<u64>().ok()
}

/// 从 URL 提取 host 名（去端口/方括号/userinfo），供 DNS 二次校验用。
pub(crate) fn url_host(url: &str) -> String {
    let host = url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.find('/').map(|i| &rest[..i]).or(Some(rest)))
        .unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or("").trim();
    let hostname = if host.starts_with('[') {
        host.find(']').map(|i| &host[..=i]).unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or("")
    };
    hostname
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase()
}

/// R505：DNS 二次校验——域名解析出的**全部** A/AAAA 记录都必须是公网 IP，
/// 任一记录落在私网/环回/链路本地即拒绝（防 nip.io / localtest.me /
/// 攻击者控权 DNS 重绑定到 169.254.169.254、127.0.0.1 等）。
/// 解析失败一律拒绝（fail-closed）。`localhost.` 尾点域名与 IPv6 zone-id
/// （`fe80::1%25eth0`，解析会失败或语义不明）直接拒绝。
pub(crate) fn dns_resolve_blocked(host: &str) -> Option<String> {
    let h = host.trim().trim_end_matches('.');
    if h.is_empty() {
        return Some("empty host".into());
    }
    if h == "localhost" || h.ends_with(".localhost") {
        return Some("localhost".into());
    }
    // zone-id 形式（fe80::1%eth0）拒绝——含 % 的都不是合法全局单播域名。
    if h.contains('%') {
        return Some("zone-id/IPv6 scope 拒绝".into());
    }
    // 已是 IP 字面量：交给字面量判定（blocked_private_target 已覆盖）。
    if h.contains(':') {
        if let Some(ip) = parse_ipv6(h) {
            return if ipv6_is_private(&ip) {
                Some("private/loopback address (DNS)".into())
            } else {
                None
            };
        }
        return Some("非法 IPv6 字面量".into());
    }
    if parse_ipv4(h).is_some() {
        return None; // 字面量 IPv4 由 blocked_private_target 统一判
    }
    // 域名 → 解析全部地址
    use std::net::ToSocketAddrs;
    match (h, 80u16).to_socket_addrs() {
        Ok(addrs) => {
            let mut any = false;
            for a in addrs {
                any = true;
                match a.ip() {
                    std::net::IpAddr::V4(v4) => {
                        let [a, b, c, d] = v4.octets();
                        if ipv4_is_private(a, b, c, d) {
                            return Some(format!("域名 {h} 解析到私网/环回地址 {v4}"));
                        }
                    }
                    std::net::IpAddr::V6(v6) => {
                        let ip = v6.octets();
                        if ipv6_is_private(&ip) {
                            return Some(format!("域名 {h} 解析到私网/环回地址 {v6}"));
                        }
                    }
                }
            }
            if any {
                None
            } else {
                Some(format!("域名 {h} 解析不到任何地址"))
            }
        }
        Err(_) => Some(format!("域名 {h} 无法解析（fail-closed 拒绝出站）")),
    }
}

/// R505：出站 SSRF 总闸 = 字面量判定 + DNS 二次校验。
/// 所有出网路径（ai_proxy / ssrf_safe_get / MCP bridge / 注册表 URL）统一走这里。
pub(crate) fn ssrf_block_reason(url: &str) -> Option<String> {
    if let Some(r) = blocked_private_target(url) {
        return Some(r.to_string());
    }
    let host = url_host(url);
    dns_resolve_blocked(&host)
}

/// 异步版总闸：DNS 解析丢到 blocking 线程池，避免卡住 async executor。
pub(crate) async fn ssrf_block_reason_async(url: &str) -> Option<String> {
    let u = url.to_string();
    tokio::task::spawn_blocking(move || ssrf_block_reason(&u))
        .await
        .ok()
        .flatten()
}
