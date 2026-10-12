//! CPU 温度域（从 hw_thermal.rs 拆分，2026-10-12）。
//! max_cpu_temp + collect_temperature_fast_parse：熔断用的快速读数。

use crate::ps::{json_array_of, ps_capture};
use crate::tools::hw_common::{get_f64, thermal_ready};
use crate::tools::v_str;
use serde_json::Value;

use super::therm_sensors::run_fancmd_sensors;
use super::therm_temp::THERMAL_PS;

#[cfg(windows)]
pub fn max_cpu_temp() -> Result<f64, String> {
    let mut best = f64::NAN;
    // 0) 自研 SuperIO 直读（inpoutx64 驱动 + 环境寄存器，毫秒级、普通权限即可）：
    //    本机 ITE 0x8689 直读 CPU 42℃，无需 HWiNFO/LHM/管理员。放最前是为了
    //    stress_test 每 1s 采样不被 fancmd 的 4-5s spawn 拖垮。
    if let Ok(temps) = crate::tools::superio::read_temperatures() {
        for (label, v) in &temps {
            if label.contains("CPU") && v.is_finite() && (best.is_nan() || *v > best) {
                best = *v;
            }
        }
    }
    // 1) fancmd sensors（LHM 全量 CPU 核心温度，快通道 <1s）
    if best.is_nan() {
        if let Some(exe) = crate::tools::selfheal::find_fancmd() {
            if let Ok(raw) = run_fancmd_sensors(&exe) {
                if let Ok(groups) = serde_json::from_str::<Vec<Value>>(raw.trim()) {
                    for g in &groups {
                        let hw_name = v_str(g, "hardware");
                        if !hw_name.contains("CPU") && !hw_name.contains("Core") {
                            continue;
                        }
                        let Some(gm) = g.get("groups").and_then(Value::as_object) else {
                            continue;
                        };
                        if let Some(items) = gm.get("Temperature").and_then(Value::as_array) {
                            for i in items {
                                if let Some(v) = i.get("value").and_then(Value::as_f64) {
                                    if v.is_finite() && (best.is_nan() || v > best) {
                                        best = v;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // 2) SuperIO / fancmd 都不可用时，快速回退 ACPI 热区 + SMART 磁盘温度（~3s），
    //    保证压测熔断护栏仍可用。
    if best.is_nan() {
        if let Some(t) = collect_temperature_fast_parse().ok().flatten() {
            best = t;
        }
    }
    if !best.is_nan() {
        Ok(best)
    } else {
        // fancmd 输出里 CPU Temperature 全 null / ACPI 被拒，都是权限不足的典型表现
        //（LibreHardwareMonitor 读 CPU 温度依赖内核驱动，非管理员被拒；ACPI 热区
        // 非管理员返回 0x80041003）。提示提权而非「不可用」，误导排查方向。
        Err("未读到 CPU 温度：fancmd 的 CPU 温度读数为空或 ACPI 热区被拒（常见于非管理员运行，可提权后重试）；stress_test 将无温度熔断护栏运行".into())
    }
}

/// 快速温度数值：ACPI 热区 + SMART 磁盘温度里取最高（fancmd 慢时的降级温度源）。
/// 返回 `Ok(Some(℃))` 有值 / `Ok(None)` 无有效温度 / `Err` 通道不可用。
#[cfg(windows)]
fn collect_temperature_fast_parse() -> Result<Option<f64>, String> {
    let raw = ps_capture(THERMAL_PS)?;
    let arr: Vec<Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析温度查询结果失败：{e}"))?;
    let mut best = f64::NAN;
    for t in &arr {
        let raw = get_f64(t, "CurrentTemperature").unwrap_or(0.0);
        let c = raw / 10.0 - 273.15;
        if thermal_ready(c) && c > best {
            best = c;
        }
    }
    if best.is_nan() {
        Ok(None)
    } else {
        Ok(Some(best))
    }
}

#[cfg(not(windows))]
pub fn max_cpu_temp() -> Result<f64, String> {
    Err("max_cpu_temp 仅支持 Windows".into())
}
