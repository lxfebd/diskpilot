//! hw_report.rs 单元测试（从 hw_report.rs 拆分，2026-10-12）。
//! 二进制单位格式化 / 显存溢出识别 / markdown 组装 / 风扇建议。

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
