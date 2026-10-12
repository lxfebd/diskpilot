//! 风扇建议档位推导：build_fan_advice（温度/负载 → 建议曲线，纯规则可单测）。
//! 从 hw_report.rs 拆分（2026-10-12）。只读传感器推导，落地调速走 fan_control。

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
