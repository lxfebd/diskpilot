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

/// 从温度/负载推导建议档位（纯规则，可单测）：
/// info = hw_info 缓存结果（cpu[].LoadPercentage + thermal[].CurrentTemperature）。
/// 返回 { level, level_label, reason, curve: [{load_pct, fan_pct}], sensors, note }。
/// 这里是「建议」层：只读传感器推导档位；落地调速走 fan_control（L2 权限，多通道探测）。
pub(crate) fn build_fan_advice(info: &serde_json::Value) -> serde_json::Value {
    // 最高温度传感器（MSAcpi 单位：十分之一开尔文）。
    let mut temps: Vec<f32> = Vec::new();
    if let Some(arr) = info.get("thermal").and_then(|v| v.as_array()) {
        for t in arr {
            if let Some(raw) = t.get("CurrentTemperature").and_then(|v| v.as_f64()) {
                let c = (raw / 10.0 - 273.15) as f32;
                if (0.0..=120.0).contains(&c) {
                    temps.push(c);
                }
            }
        }
    }
    let temp_max = temps.iter().cloned().fold(f32::MIN, f32::max);
    let temp_max = if temp_max == f32::MIN {
        None
    } else {
        Some(temp_max)
    };

    // CPU 负载（LoadPercentage 可能缺失）。
    let mut load: Option<f64> = None;
    if let Some(arr) = info.get("cpu").and_then(|v| v.as_array()) {
        if let Some(first) = arr.first() {
            load = first.get("LoadPercentage").and_then(|v| v.as_f64());
        }
    }

    // 档位规则：温度优先（散热是结果），负载其次（负载是原因）。
    let (level, level_label, reason) = match (temp_max, load) {
        (Some(t), _) if t >= 90.0 => (
            "urgent",
            "过热风险",
            format!("温度已达 {t:.0}℃，接近降频阈值。建议立即清灰/改善进风，并把风扇拉满跑几分钟压温。"),
        ),
        (Some(t), _) if t >= 80.0 => (
            "high",
            "偏高",
            format!("温度 {t:.0}℃，持续高负载下偏热。建议把曲线中段（60-80% 负载）抬高 10-15%。"),
        ),
        (Some(t), Some(l)) if t >= 70.0 && l >= 60.0 => (
            "balanced_plus",
            "略偏高",
            format!("负载 {l:.0}% 时温度 {t:.0}℃，正常偏高。建议中段风扇 +10%，或改用「高性能」电源计划让风扇更早介入。"),
        ),
        (Some(t), Some(l)) if t < 55.0 && l < 25.0 => (
            "quiet_ok",
            "安静档可行",
            format!("负载 {l:.0}%、温度 {t:.0}℃，散热余量大。低负载段（<30%）可把风扇压到 30-40% 换安静。"),
        ),
        (Some(t), Some(l)) => (
            "balanced",
            "均衡",
            format!("负载 {l:.0}%、温度 {t:.0}℃，散热正常。保持默认曲线即可。"),
        ),
        (Some(t), None) => (
            "temp_only",
            "温度可读",
            format!("温度 {t:.0}℃，CPU 负载传感器读不到。按温度给建议：{}{}。",
                if t >= 80.0 { "偏高，建议清灰/改善散热" } else { "散热正常" },
                if t >= 90.0 { "，并立即检查散热" } else { "" }
            ),
        ),
        (None, Some(l)) => (
            "unknown_temp",
            "温度不可读",
            format!("温度传感器读不到（ACPI 热区未暴露），只能按负载 {l:.0}% 给通用建议：低负载安静、高负载均衡。"),
        ),
        (None, None) => (
            "unknown",
            "数据不足",
            "温度与负载传感器都读不到，无法给出针对性建议。常见原因：主板未暴露 ACPI 热区/需要管理员权限。".to_string(),
        ),
    };

    // 建议曲线锚点（负载% → 风扇%），按档位微调默认 OEM 曲线形态。
    let curve: Vec<serde_json::Value> = match level {
        "urgent" | "high" => [(20u32, 55u32), (40, 70), (60, 85), (80, 100), (100, 100)],
        "balanced_plus" => [(20, 40), (40, 55), (60, 70), (80, 85), (100, 100)],
        "quiet_ok" => [(20, 30), (40, 40), (60, 60), (80, 75), (100, 90)],
        _ => [(20, 35), (40, 50), (60, 65), (80, 80), (100, 100)],
    }
    .iter()
    .map(|(l, f)| serde_json::json!({ "load_pct": l, "fan_pct": f }))
    .collect();

    serde_json::json!({
        "level": level,
        "level_label": level_label,
        "reason": reason,
        "temp_max_c": temp_max,
        "cpu_load_pct": load,
        "curve": curve,
        "note": "这是基于温度/负载的建议曲线。落地调速由「风扇控制」（fan.control 权限，多通道探测：内置 WMI + 厂商/第三方 CLI 通道）执行；本建议的锚点可直接作为控制面板与自定义曲线编辑的起点。"
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fmt_gb_handles_binary_units() {
        assert_eq!(fmt_gb(0), "0 B");
        assert_eq!(fmt_gb(1023), "1023 B");
        assert_eq!(fmt_gb(1536), "2 KB"); // 1.5 KB 四舍五入到 0 位小数
        assert_eq!(fmt_gb(1024 * 1024), "1.0 MB");
        assert_eq!(fmt_gb(1024 * 1024 * 1024), "1.0 GB");
        // 恰好 4 GiB 走 GB 分支
        let gb = 4u64 * 1024 * 1024 * 1024;
        assert_eq!(fmt_gb(gb), "4.0 GB");
    }

    /// AdapterRAM 溢出判定回归：原条件第二支 `<=` 写反（2026-10-11 修复）。
    #[test]
    fn vram_overflow_detects_saturation_not_normal_cards() {
        // 正常显存绝不能被标成溢出（这正是原 bug：2GB 卡被误标）
        assert!(
            !is_vram_overflow(2 * 1024 * 1024 * 1024),
            "2GB 卡不得标溢出"
        );
        assert!(!is_vram_overflow(1024 * 1024 * 1024), "1GB 卡不得标溢出");
        // u32 饱和区与 ≥4GiB-1MiB 的读数必须标溢出
        assert!(is_vram_overflow(4_294_967_295), "u32::MAX 必须标溢出");
        assert!(is_vram_overflow(4_294_967_294), "u32 饱和值必须标溢出");
        assert!(
            is_vram_overflow(4_293_918_720),
            "4GiB-1MiB 阈值本身必须标溢出"
        );
        assert!(is_vram_overflow(4_294_967_000), "溢出窗口内读数必须标溢出");
    }

    #[test]
    fn v_str_reads_string_number_bool_and_missing() {
        let v = json!({ "s": "abc", "n": 42, "b": true });
        assert_eq!(v_str(&v, "s"), "abc");
        assert_eq!(v_str(&v, "n"), "42");
        assert_eq!(v_str(&v, "b"), "true");
        assert_eq!(v_str(&v, "missing"), "");
        assert_eq!(v_str(&serde_json::Value::Null, "x"), "");
    }

    #[test]
    fn arr_extracts_array_or_empty() {
        let v = json!({ "list": [1, 2, 3], "nope": "x" });
        assert_eq!(arr(&v, "list").len(), 3);
        assert_eq!(arr(&v, "nope").len(), 0);
        assert_eq!(arr(&v, "missing").len(), 0);
    }

    #[test]
    fn build_hw_markdown_includes_health_failure_note() {
        let info = json!({
            "os": [{ "Caption": "Windows 11 Pro", "Version": "10.0.22631", "BuildNumber": "22631" }],
            "system": [{ "Manufacturer": "LENOVO", "Model": "Y9000P" }],
            "cpu": [], "gpu": [], "memory": [], "board": [], "bios": [], "disk": [], "thermal": []
        });
        let md = build_hw_markdown(&info, None, "（硬盘健康读取失败：WMI 拒绝访问）");
        assert!(md.contains("Windows 11 Pro"));
        assert!(md.contains("硬盘健康（SMART 可用：false）"));
        assert!(md.contains("WMI 拒绝访问"));
    }

    #[test]
    fn build_hw_markdown_lists_health_details_when_available() {
        let info = json!({
            "os": [], "system": [], "cpu": [], "gpu": [], "memory": [], "board": [],
            "bios": [], "disk": [], "thermal": []
        });
        let health = json!({
            "reliability": true,
            "disks": [{
                "friendly": "NVMe SSD", "media": "SSD", "bus": "NVMe",
                "health": "Healthy", "operational": "OK", "power_on_hours": 1234,
                "temperature_c": 45.0, "wear_pct": 3.0,
                "read_errors": 0, "write_errors": 0
            }]
        });
        let md = build_hw_markdown(&info, Some(&health), "");
        assert!(md.contains("SMART 可用：true"));
        assert!(md.contains("NVMe SSD"));
        assert!(md.contains("通电时间：1234 小时"));
        assert!(md.contains("SSD 寿命剩余：97%"));
        assert!(md.contains("读错误：0 · 写错误：0"));
    }

    #[test]
    fn build_hw_markdown_omits_health_section_when_no_health_and_no_note() {
        let info = json!({
            "os": [], "system": [], "cpu": [], "gpu": [], "memory": [], "board": [],
            "bios": [], "disk": [], "thermal": []
        });
        let md = build_hw_markdown(&info, None, "");
        assert!(!md.contains("硬盘健康"));
    }

    #[test]
    fn build_hw_markdown_formats_thermal_sensor() {
        let info = json!({
            "os": [], "system": [], "cpu": [], "gpu": [], "memory": [], "board": [],
            "bios": [], "disk": [],
            "thermal": [{ "InstanceName": "ACPI\\ThermalZone\\TZ00_0", "CurrentTemperature": 2992 }]
        });
        let md = build_hw_markdown(&info, None, "");
        assert!(md.contains("## 温度传感器"));
        assert!(md.contains("26 ℃")); // 2992/10 - 273.15 = 26.05 → "26 ℃"
    }

    #[test]
    fn fan_advice_urgent_when_temp_high() {
        let info = json!({
            "cpu": [{ "LoadPercentage": 40 }],
            "thermal": [{ "InstanceName": "ACPI\\ThermalZone\\TZ00_0", "CurrentTemperature": 3652 }]
        });
        let advice = build_fan_advice(&info);
        assert_eq!(advice["level"], "urgent"); // 3652/10-273.15 ≈ 92℃
        let t = advice["temp_max_c"].as_f64().unwrap();
        assert!((t - 92.05).abs() < 0.01);
        // 紧急档曲线拉满
        let curve = advice["curve"].as_array().unwrap();
        assert_eq!(curve.last().unwrap()["fan_pct"], 100);
    }

    #[test]
    fn fan_advice_quiet_when_cool_and_idle() {
        let info = json!({
            "cpu": [{ "LoadPercentage": 5 }],
            "thermal": [{ "InstanceName": "ACPI\\ThermalZone\\TZ00_0", "CurrentTemperature": 2982 }]
        });
        let advice = build_fan_advice(&info);
        assert_eq!(advice["level"], "quiet_ok"); // 2982/10-273.15 ≈ 25℃
        assert!(advice["reason"].as_str().unwrap().contains("安静"));
    }

    #[test]
    fn fan_advice_data_missing_degrades_gracefully() {
        let advice = build_fan_advice(&json!({ "cpu": [], "thermal": [] }));
        assert_eq!(advice["level"], "unknown");
        assert!(advice["temp_max_c"].is_null());
        assert!(advice["cpu_load_pct"].is_null());
    }

    #[test]
    fn html_wrap_escapes_user_content() {
        let wrapped = html_wrap("<script>alert(1)</script>");
        assert!(wrapped.contains("&lt;script&gt;"));
        assert!(!wrapped.contains("<script>alert"));
    }
}
