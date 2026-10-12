//! 硬件检测报告格式化（纯函数，便于测试）。
//!
//! 从 `hw.rs` 拆分（2026-10-12）：报告 Markdown/HTML 生成与风扇建议规则
//! 均为无状态纯函数——只依赖 `serde_json` / `chrono` / `std`，不接触
//! AppHandle/State/tauri 命令层，独立成模块便于单测与维护。

/// HTML 转义（防注入：报告内容含外部数据，必须转义后进 HTML）。
pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 纯文本 → 自包含 HTML 页（内联样式，可直接打开/打印）。
pub(crate) fn html_wrap(md: &str) -> String {
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>DiskPilot 硬件检测报告</title>\n<style>body{{font:14px/1.6 system-ui,-apple-system,Segoe UI,sans-serif;max-width:860px;margin:40px auto;padding:0 16px;color:#222}}h1,h2,h3{{border-bottom:1px solid #ddd;padding-bottom:4px}}code{{background:#f4f4f4;padding:1px 4px;border-radius:3px}}pre{{background:#f6f8fa;padding:12px;border-radius:6px;overflow:auto}}table{{border-collapse:collapse;margin:8px 0}}td,th{{border:1px solid #ccc;padding:4px 8px}}</style>\n<pre>{}</pre>",
        html_escape(md)
    )
}

/// WMI `Win32_VideoController.AdapterRAM` 是否落在 32 位溢出饱和区。
/// AdapterRAM 是 UINT32：显存 ≥4GiB 时读数饱和到 ~u32::MAX（4294967294/…295），
/// 不是真实容量；阈值取 4GiB-1MiB（4293918720，含 u32 读数整个上界窗口）。
/// 原实现 `vram >= 4GiB || vram <= 4GiB-1MiB` 第二支写反（`<=`），把 2GB 卡
/// 标成「≥4GB 溢出」、真正的溢出值反落进 GB 分支——除 1MiB 窗口外全判错
/// （2026-10-11 修复，与前端 HwPanels.vramStr 同源同修）。
pub(crate) fn is_vram_overflow(vram: i64) -> bool {
    vram >= 4_293_918_720
}

pub(crate) fn fmt_gb(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.*} GB", 1, b / 1024.0 / 1024.0 / 1024.0)
    } else if b >= 1024.0 * 1024.0 {
        format!("{:.*} MB", 1, b / 1024.0 / 1024.0)
    } else if b >= 1024.0 {
        format!("{:.*} KB", 0, b / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn v_str(v: &serde_json::Value, k: &str) -> String {
    match v.get(k) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

fn arr<'a>(v: &'a serde_json::Value, k: &str) -> Vec<&'a serde_json::Value> {
    v.get(k)
        .and_then(|x| x.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

/// 硬件快照 → Markdown 报告（纯文本，经 html_wrap 包装可导出 HTML）。
pub(crate) fn build_hw_markdown(
    info: &serde_json::Value,
    health: Option<&serde_json::Value>,
    health_note: &str,
) -> String {
    let mut out = String::new();
    out.push_str("# DiskPilot 硬件检测报告\n\n");
    out.push_str(&format!(
        "- 生成时间：{}\n\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    ));

    let os = arr(info, "os")
        .first()
        .copied()
        .unwrap_or(&serde_json::Value::Null);
    let system = arr(info, "system")
        .first()
        .copied()
        .unwrap_or(&serde_json::Value::Null);
    out.push_str("## 系统\n\n");
    out.push_str(&format!("- 系统：{}\n", v_str(os, "Caption")));
    out.push_str(&format!(
        "- 版本：{} / Build {}\n",
        v_str(os, "Version"),
        v_str(os, "BuildNumber")
    ));
    out.push_str(&format!("- 制造商：{}\n", v_str(system, "Manufacturer")));
    out.push_str(&format!("- 机型：{}\n\n", v_str(system, "Model")));

    let cpus = arr(info, "cpu");
    if let Some(c) = cpus.first() {
        out.push_str("## CPU\n\n");
        out.push_str(&format!("- 型号：{}\n", v_str(c, "Name")));
        out.push_str(&format!(
            "- 核心/线程：{} / {}\n",
            v_str(c, "NumberOfCores"),
            v_str(c, "NumberOfLogicalProcessors")
        ));
        let maxmhz: i64 = v_str(c, "MaxClockSpeed").parse().unwrap_or(0);
        let curmhz: i64 = v_str(c, "CurrentClockSpeed").parse().unwrap_or(0);
        out.push_str(&format!(
            "- 频率：标称 {} MHz，当前 {} MHz\n",
            maxmhz, curmhz
        ));
        out.push_str(&format!(
            "- 缓存：L2 {} KB / L3 {} KB\n",
            v_str(c, "L2CacheSize"),
            v_str(c, "L3CacheSize")
        ));
        out.push_str(&format!("- 制造商：{}\n", v_str(c, "Manufacturer")));
        let virt = v_str(c, "VirtualizationFirmwareEnabled");
        out.push_str(&format!(
            "- 虚拟化：{}\n\n",
            if virt == "True" {
                "已开启"
            } else if virt == "False" {
                "未开启"
            } else {
                "未知"
            }
        ));
    }

    let gpus = arr(info, "gpu");
    if !gpus.is_empty() {
        out.push_str("## 显卡\n\n");
        for g in gpus {
            let vram: i64 = v_str(g, "AdapterRAM").parse().unwrap_or(0);
            let vram_str = if vram > 0 {
                if is_vram_overflow(vram) {
                    "≥4 GB（WMI 32 位溢出，实际可能更大）".to_string()
                } else {
                    fmt_gb(vram as u64)
                }
            } else {
                "未知".to_string()
            };
            out.push_str(&format!("- {} · 显存 {}\n", v_str(g, "Name"), vram_str));
            out.push_str(&format!(
                "  - 驱动 {} · {} · {}x{}\n",
                v_str(g, "DriverVersion"),
                v_str(g, "DriverDate"),
                v_str(g, "CurrentHorizontalResolution"),
                v_str(g, "CurrentVerticalResolution")
            ));
            out.push_str(&format!(
                "  - 刷新率 {} Hz\n",
                v_str(g, "CurrentRefreshRate")
            ));
        }
        out.push('\n');
    }

    let mems = arr(info, "memory");
    let total: i64 = mems
        .iter()
        .filter_map(|m| v_str(m, "Capacity").parse::<i64>().ok())
        .sum();
    if total > 0 || !mems.is_empty() {
        out.push_str("## 内存\n\n");
        out.push_str(&format!("- 总容量：{}\n", fmt_gb(total as u64)));
        let mut speeds: Vec<i64> = Vec::new();
        for m in &mems {
            let cap = v_str(m, "Capacity");
            let speed = v_str(m, "Speed");
            speeds.push(speed.parse().unwrap_or(0));
            out.push_str(&format!(
                "- {} {} · {} · {} · {}\n",
                v_str(m, "DeviceLocator"),
                fmt_gb(cap.parse::<i64>().unwrap_or(0) as u64),
                speed,
                v_str(m, "Manufacturer"),
                v_str(m, "PartNumber")
            ));
        }
        if let Some(&s) = speeds.iter().max() {
            let confs: Vec<i64> = mems
                .iter()
                .filter_map(|m| v_str(m, "ConfiguredClockSpeed").parse().ok())
                .collect();
            let max_conf = confs.iter().max().copied().unwrap_or(0);
            if max_conf > 0 {
                out.push_str(&format!(
                    "- 频率：标称 {} MHz，当前运行 {} MHz{}\n",
                    s,
                    max_conf,
                    if max_conf < s {
                        "（XMP/EXPO 可能未生效）"
                    } else {
                        "（XMP/EXPO 已生效）"
                    }
                ));
            }
        }
        out.push('\n');
    }

    let board = arr(info, "board")
        .first()
        .copied()
        .unwrap_or(&serde_json::Value::Null);
    let bios = arr(info, "bios")
        .first()
        .copied()
        .unwrap_or(&serde_json::Value::Null);
    out.push_str("## 主板 / BIOS\n\n");
    out.push_str(&format!(
        "- 主板：{} {}\n",
        v_str(board, "Manufacturer"),
        v_str(board, "Product")
    ));
    out.push_str(&format!(
        "- BIOS：{} · {}\n\n",
        v_str(bios, "SMBIOSBIOSVersion"),
        v_str(bios, "ReleaseDate")
    ));

    let disks = arr(info, "disk");
    if !disks.is_empty() {
        out.push_str("## 磁盘\n\n");
        for d in disks {
            let size: i64 = v_str(d, "Size").parse().unwrap_or(0);
            out.push_str(&format!(
                "- {} · {} · {} · {}\n",
                v_str(d, "Model"),
                fmt_gb(size as u64),
                v_str(d, "InterfaceType"),
                v_str(d, "Status")
            ));
        }
        out.push('\n');
    }

    // 硬盘健康小节：数据读得到时列明细；读不到但带失败原因时也要可见，
    // 不能静默消失（health_note 非空时单独出小节）。
    if health.is_some() || !health_note.is_empty() {
        let smart = health
            .and_then(|h| h.get("reliability"))
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        out.push_str(&format!(
            "## 硬盘健康（SMART 可用：{smart}）{health_note}\n\n"
        ));
    }
    if let Some(h) = health {
        if let Some(disks_h) = h.get("disks").and_then(|x| x.as_array()) {
            for d in disks_h {
                out.push_str(&format!(
                    "- {} · {} · {}\n",
                    v_str(d, "friendly"),
                    v_str(d, "media"),
                    v_str(d, "bus")
                ));
                out.push_str(&format!(
                    "  - 健康：{} · 状态：{}\n",
                    v_str(d, "health"),
                    v_str(d, "operational")
                ));
                if let Some(ph) = v_str(d, "power_on_hours")
                    .parse::<i64>()
                    .ok()
                    .filter(|v| *v > 0)
                {
                    out.push_str(&format!("  - 通电时间：{} 小时\n", ph));
                }
                if let Some(tc) = v_str(d, "temperature_c")
                    .parse::<f64>()
                    .ok()
                    .filter(|v| *v > 0.0)
                {
                    let c = if tc > 60.0 {
                        (tc - 32.0) * 5.0 / 9.0
                    } else {
                        tc
                    };
                    out.push_str(&format!("  - 温度：{:.*} ℃\n", 0, c));
                }
                if let Some(w) = v_str(d, "wear_pct")
                    .parse::<f64>()
                    .ok()
                    .filter(|v| *v >= 0.0)
                {
                    let life = 100.0 - w;
                    out.push_str(&format!(
                        "  - SSD 寿命剩余：{:.*}%（磨损度 {:.*}%）\n",
                        0, life, 0, w
                    ));
                }
                let re = v_str(d, "read_errors");
                if !re.is_empty() {
                    out.push_str(&format!(
                        "  - 读错误：{} · 写错误：{}\n",
                        re,
                        v_str(d, "write_errors")
                    ));
                }
            }
        }
        if let Some(cdis) = h.get("cdi_report").and_then(|x| x.as_array()) {
            if !cdis.is_empty() {
                out.push_str("\n### CrystalDiskInfo 补充字段（原始标签）\n\n");
                for c in cdis {
                    out.push_str(&format!("- {}\n", v_str(c, "device")));
                    if let Some(obj) = c.as_object() {
                        for (k, _val) in obj {
                            if k == "device" {
                                continue;
                            }
                            out.push_str(&format!("  - {k}：{}\n", v_str(c, k)));
                        }
                    }
                }
            }
        }
    }

    let thermal = arr(info, "thermal");
    if !thermal.is_empty() {
        out.push_str("## 温度传感器\n\n");
        for t in thermal {
            let raw: f64 = v_str(t, "CurrentTemperature").parse().unwrap_or(0.0);
            let c = raw / 10.0 - 273.15;
            if (0.0..=120.0).contains(&c) {
                out.push_str(&format!("- {}：{:.0} ℃\n", v_str(t, "InstanceName"), c));
            }
        }
        out.push('\n');
    }

    out.push_str("---\n*数据来源：WMI CIM 查询 + 图吧工具箱（CrystalDiskInfo /CopyExit）。读不到的字段留空，不编造。*\n");
    out
}

mod fan;
pub(crate) use fan::build_fan_advice;

#[cfg(test)]
#[path = "hw_report_tests.rs"]
mod tests;
