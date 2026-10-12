//! 阈值告警域（从 report.rs 拆分，2026-10-12）。
//! sensor_alert。

use crate::tools::{hw, system};

use super::rep_storage::{append_jsonl, trim_jsonl, ts_secs};
use super::rep_util::{extract_first_f64, extract_free_gb};

pub fn sensor_alert(
    cpu_temp: Option<f64>,
    disk_temp: Option<f64>,
    gpu_temp: Option<f64>,
    mem_percent: Option<f64>,
    disk_free_gb: Option<f64>,
    persist: Option<bool>,
) -> Result<String, String> {
    let cpu_limit = cpu_temp.unwrap_or(85.0).clamp(50.0, 110.0);
    let disk_limit = disk_temp.unwrap_or(55.0).clamp(30.0, 90.0);
    let gpu_limit = gpu_temp.unwrap_or(85.0).clamp(50.0, 110.0);
    let mem_limit = mem_percent.unwrap_or(90.0).clamp(50.0, 100.0);
    let disk_min_gb = disk_free_gb.unwrap_or(20.0).clamp(1.0, 500.0);
    let persist = persist.unwrap_or(false);

    let mut alerts: Vec<String> = Vec::new();
    let mut info = String::new();

    // CPU 温度
    match hw::max_cpu_temp() {
        Ok(t) => {
            info.push_str(&format!("CPU 温度：{t:.1}℃\n"));
            if t >= cpu_limit {
                alerts.push(format!("⚠️ CPU 温度 {t:.1}℃ 超过阈值 {cpu_limit:.1}℃"));
            }
        }
        Err(e) => info.push_str(&format!("CPU 温度不可读：{e}\n")),
    }

    // 磁盘温度 / 剩余空间：读 SMART 快照文本挑温度 + 磁盘健康里挑剩余
    let smart = hw::collect_disk_smart().unwrap_or_default();
    for line in smart.lines() {
        let low = line.to_lowercase();
        if low.contains("温度") || low.contains("temp") {
            if let Some(v) = extract_first_f64(line) {
                info.push_str(&format!("磁盘温度：{v:.1}℃\n"));
                if v >= disk_limit {
                    alerts.push(format!("⚠️ 磁盘温度 {v:.1}℃ 超过阈值 {disk_limit:.1}℃"));
                }
                break;
            }
        }
    }
    if let Some((free_gb, free_str)) = extract_free_gb(&smart) {
        info.push_str(&format!("磁盘剩余：{free_str}（{free_gb:.1} GB）\n"));
        if free_gb <= disk_min_gb {
            alerts.push(format!(
                "⚠️ 磁盘剩余 {free_gb:.1}GB 低于阈值 {disk_min_gb:.1}GB"
            ));
        }
    }

    // GPU 温度：温度快照文本挑 GPU 相关
    let temps = hw::collect_sensors_fast().unwrap_or_default();
    for line in temps.lines() {
        let low = line.to_lowercase();
        if (low.contains("gpu") || low.contains("显卡"))
            && (low.contains("温度") || low.contains("temp"))
        {
            if let Some(v) = extract_first_f64(line) {
                info.push_str(&format!("GPU 温度：{v:.1}℃\n"));
                if v >= gpu_limit {
                    alerts.push(format!("⚠️ GPU 温度 {v:.1}℃ 超过阈值 {gpu_limit:.1}℃"));
                }
                break;
            }
        }
    }

    // 内存占用
    let sys = system::collect_system_info();
    info.push_str(&format!(
        "内存占用：{:.1}%（{} / {}）\n",
        sys.mem_percent,
        crate::tools::fmt_bytes(sys.mem_used_bytes),
        crate::tools::fmt_bytes(sys.mem_total_bytes)
    ));
    if sys.mem_percent >= mem_limit {
        alerts.push(format!(
            "⚠️ 内存占用 {:.1}% 超过阈值 {mem_limit:.1}%",
            sys.mem_percent
        ));
    }

    let mut out = String::new();
    out.push_str("传感器阈值体检：\n");
    out.push_str(&info);
    if alerts.is_empty() {
        out.push_str("\n✅ 全部指标在阈值内，未见异常。\n");
    } else {
        out.push_str(&format!("\n{} 项告警：\n", alerts.len()));
        for a in &alerts {
            out.push_str(a);
            out.push('\n');
        }
    }

    if persist && !alerts.is_empty() {
        let row = serde_json::json!({
            "ts": ts_secs(),
            "alerts": alerts,
        });
        append_jsonl("sensor-alerts.jsonl", &row.to_string());
        trim_jsonl("sensor-alerts.jsonl", 200);
        out.push_str("\n（告警已追加到 sensor-alerts.jsonl 历史）");
    }
    Ok(out)
}
