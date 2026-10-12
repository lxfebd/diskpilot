//! 阈值告警域（从 report.rs 拆分，2026-10-12）。
//! sensor_alert。

use crate::tools::{disk, hw, system};

use super::rep_storage::{append_jsonl, trim_jsonl, ts_secs};
use super::rep_util::extract_first_f64;

/// 1 GiB（字节）。
const GB: f64 = 1024.0 * 1024.0 * 1024.0;

/// SMART 文本 → 每块盘的「当前温度」读数（纯解析，可单测）。
/// 盘名取自 `- 盘名：…` 段头；只认「当前温度」行——「历史最高温度」是
/// 过去值，不作为实时告警依据。多盘机器逐块返回，而不是只取第一块。
pub(crate) fn disk_current_temps(smart: &str) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    let mut current_disk = String::new();
    for line in smart.lines() {
        // 段头无缩进（"- 盘名：…"），读数行有缩进（"  - 当前温度：…"）——
        // 必须按原始缩进区分：trim 后两者都以 "- " 开头，会被混判成段头。
        if let Some(rest) = line.strip_prefix("- ") {
            current_disk = rest.split('：').next().unwrap_or("").trim().to_string();
            continue;
        }
        let t = line.trim_start();
        if !t.starts_with("- 当前温度") {
            continue;
        }
        if let Some(v) = extract_first_f64(t) {
            let name = if current_disk.is_empty() {
                "磁盘".to_string()
            } else {
                current_disk.clone()
            };
            out.push((name, v));
        }
    }
    out
}

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

    // 磁盘温度：逐块检查而不是只取第一块——多盘机器里第二块盘过热同样必须告警。
    let smart = hw::collect_disk_smart().unwrap_or_default();
    for (name, v) in disk_current_temps(&smart) {
        info.push_str(&format!("磁盘温度（{name}）：{v:.1}℃\n"));
        if v >= disk_limit {
            alerts.push(format!(
                "⚠️ 磁盘 {name} 温度 {v:.1}℃ 超过阈值 {disk_limit:.1}℃"
            ));
        }
    }

    // 磁盘剩余：SMART 是硬件健康文本，里面没有剩余容量行——旧实现从 SMART
    // 文本里挑「剩余 X GB」几乎永远挑不到，这条告警实际是死的。改用
    // collect_disks()（GetDiskFreeSpaceExW）逐分区检查：任何分区低于阈值
    // 都告警，而不是只盯一块盘。空光盘/读卡器（total=0）跳过，避免假告警。
    let disks = disk::collect_disks();
    let mut tightest: Option<&disk::DiskInfo> = None;
    for d in &disks {
        if d.total_bytes == 0 {
            continue;
        }
        let free_gb = d.free_bytes as f64 / GB;
        if tightest.is_none_or(|t| d.free_bytes < t.free_bytes) {
            tightest = Some(d);
        }
        if free_gb <= disk_min_gb {
            alerts.push(format!(
                "⚠️ {} 剩余 {free_gb:.1}GB 低于阈值 {disk_min_gb:.1}GB",
                d.drive
            ));
        }
    }
    if let Some(t) = tightest {
        info.push_str(&format!(
            "磁盘剩余最低：{} {:.1}GB\n",
            t.drive,
            t.free_bytes as f64 / GB
        ));
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
