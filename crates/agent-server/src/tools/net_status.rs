//! 网卡状态 / IP / MAC / 网关 / DNS / 速率 + 适配器详情。
//!
//! from tools/net.rs 拆分（2026-10-12）。

// net_adapter 三符号仅 Windows 有实现，随 cfg(windows) 的 collect_* 编译。
#[cfg(windows)]
use crate::tools::net_adapter::{is_virtual_adapter, native_adapters, ps_adapters_fallback};
// fmt_bps 仅被 cfg(windows) 的采集实现使用，Linux 下 collect_net_status 降级 fn 不依赖它。
#[cfg(windows)]
use crate::tools::net_util::fmt_bps;

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
        out.push_str(&render_adapter_row(r));
    }
    Ok(out)
}

/// 单张网卡 → 一段文本（主行 + 网关 + DNS），**每段都以换行结尾**。
///
/// 抽成纯函数是因为这里出过一次实测 bug：DNS 行漏了 `\n`，14 张网卡的输出
/// 全糊成一行（`…::3- 以太网（Realtek…`），人和模型都没法读。
#[cfg(windows)]
fn render_adapter_row(r: &crate::tools::net_adapter::AdapterRow) -> String {
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
    let gw = if r.gateway.is_empty() {
        "未配置".to_string()
    } else {
        r.gateway.join("、")
    };
    let dns = if r.dns.is_empty() {
        "未配置".to_string()
    } else {
        r.dns.join("、")
    };
    format!(
        "- {}{} · {} · {}{}{}\n  - 网关：{}\n  - DNS：{}\n",
        name_disp, desc_part, status_txt, ip_txt, mac_part, speed_part, gw, dns
    )
}

#[cfg(not(windows))]
pub fn collect_net_status() -> Result<String, String> {
    Ok("当前平台不是 Windows，网络信息不可用".into())
}

// ── 1.5 网卡明细（MAC / 子网掩码 / 网关 / DNS / 速率）───────────────────

/// IPv4 前缀长度 → 子网掩码文本（高位连续 1）。
#[cfg(windows)]
pub(crate) fn prefix_to_mask(prefix: u8) -> String {
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
pub(crate) fn adapter_status_cn(status: i32) -> String {
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::tools::net_adapter::AdapterRow;

    fn row(name: &str, dns: Vec<String>) -> AdapterRow {
        AdapterRow {
            name: name.to_string(),
            desc: name.to_string(),
            status: 1,
            speed_bps: 8_000_000_000,
            mac: "D8-5E-D3-0D-F6-51".to_string(),
            ips: vec!["192.168.31.34/24".to_string()],
            gateway: vec!["192.168.31.1".to_string()],
            dns,
            is_virtual: false,
        }
    }

    #[test]
    fn every_adapter_block_ends_with_a_newline() {
        // 实测缺陷：DNS 行漏了 \n，14 张网卡糊成一行
        // （「…::3- 以太网（Realtek…」），人和模型都没法读。
        let a = render_adapter_row(&row("以太网", vec!["192.168.31.1".to_string()]));
        let b = render_adapter_row(&row("WLAN", vec![]));
        assert!(a.ends_with('\n'), "{a}");
        let joined = format!("{a}{b}");
        assert!(joined.contains("\n- WLAN"), "{joined}");
        assert_eq!(
            joined.lines().filter(|l| l.starts_with("- ")).count(),
            2,
            "{joined}"
        );
        assert_eq!(
            joined
                .lines()
                .filter(|l| l.starts_with("  - DNS："))
                .count(),
            2,
            "{joined}"
        );
    }

    #[test]
    fn desc_equal_to_name_is_not_repeated() {
        let a = render_adapter_row(&row("以太网", vec![]));
        assert!(a.starts_with("- 以太网 · 已连接"), "{a}");
        assert!(!a.contains("（以太网"), "{a}");
        assert!(a.contains("DNS：未配置"), "{a}");
    }
}
