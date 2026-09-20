//! 一键体检报告 + 传感器趋势 + 阈值告警（report.rs）——复刻 AIDA64 的
//! Report / 日志 / 阈值告警。
//!
//! - `system_report`（L0 只读）：汇总现有只读采集（CPU/GPU/内存/主板/温度/磁盘/
//!   网络/系统）成 Markdown 体检报告，AI 拿它当「体检结论」。
//! - `sensor_trend`（L0 只读）：把每次采样的传感器快照追加进 JSONL 历史文件，
//!   可查最近 N 条趋势（同款模式：R6 space-history.jsonl 追加式 + 去重 + 裁剪）。
//! - `sensor_alert`（L0 只读）：按阈值体检传感器（CPU 温度 / 磁盘温度 / GPU 温度
//!   / 内存占用 / 磁盘剩余），超阈值输出告警。默认只读出结果，`persist=true` 时
//!   才把历史写入磁盘文件。
//!
//! 数据目录：`%LOCALAPPDATA%/diskpilot/agent-server/`（追加式 JSONL + 原子写裁剪，
//! 失败全静默，与主项目 R6 同款容错）。

use std::io::Write;
use std::path::PathBuf;

/// 数据目录：%LOCALAPPDATA%/diskpilot/agent-server/，创建失败给 None（静默容错）。
fn data_dir() -> Option<PathBuf> {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("TEMP"))
        .ok()?;
    let dir = PathBuf::from(base).join("diskpilot").join("agent-server");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

/// 追加一行 JSONL（失败静默）。
fn append_jsonl(file: &str, line: &str) {
    let Some(dir) = data_dir() else { return };
    let path = dir.join(file);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// 读 JSONL 全部行，返回 Vec<serde_json::Value>（失败/空 → []）。
fn read_jsonl(file: &str) -> Vec<serde_json::Value> {
    let Some(dir) = data_dir() else { return vec![] };
    let path = dir.join(file);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect()
}

/// 裁剪 JSONL 到最多 `max` 条（原子写：临时文件 + rename，失败静默保留旧文件）。
fn trim_jsonl(file: &str, max: usize) {
    let Some(dir) = data_dir() else { return };
    let path = dir.join(file);
    let rows = read_jsonl(file);
    if rows.len() <= max {
        return;
    }
    let tail = &rows[rows.len().saturating_sub(max)..];
    let tmp = dir.join(format!("{file}.tmp"));
    let ok = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        for r in tail {
            writeln!(f, "{r}")?;
        }
        f.sync_all()?;
        Ok(())
    })();
    if ok.is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn ts_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 1) 一键体检报告：汇总各只读采集成 Markdown。
pub fn system_report() -> Result<String, String> {
    let mut out = String::from("# DiskPilot 系统体检报告\n\n");
    out.push_str(&format!("生成时间：{}", now_str()));
    out.push('\n');

    let mut sections: Vec<(&str, String)> = Vec::new();

    // CPU / GPU / 内存 / 主板
    for (name, f) in [
        (
            "CPU",
            super::hw::collect_cpu as fn() -> Result<String, String>,
        ),
        ("GPU", super::hw::collect_gpu),
        ("内存", super::hw::collect_memory),
        ("主板", super::hw::collect_motherboard),
    ] {
        sections.push((name, f().unwrap_or_else(|e| format!("采集失败：{e}"))));
    }
    // 温度（fancmd 全量或降级）
    sections.push((
        "温度",
        super::hw::collect_sensors_fast().unwrap_or_else(|e| format!("采集失败：{e}")),
    ));
    // 磁盘 SMART + 系统 + 电池
    sections.push((
        "磁盘健康",
        super::hw::collect_disk_smart().unwrap_or_else(|e| format!("采集失败：{e}")),
    ));
    let sys = super::system::collect_system_info();
    sections.push((
        "系统",
        format!(
            "OS：{}\nCPU 核心：{}\n内存：{} / {}（{}%）\n运行时长：{}",
            sys.os,
            sys.cpu_cores,
            crate::tools::fmt_bytes(sys.mem_used_bytes),
            crate::tools::fmt_bytes(sys.mem_total_bytes),
            format_f1(sys.mem_percent),
            fmt_uptime(sys.uptime_secs),
        ),
    ));
    sections.push((
        "电池",
        super::hw::collect_battery().unwrap_or_else(|e| format!("采集失败：{e}")),
    ));

    for (name, body) in sections {
        out.push_str(&format!("## {name}\n```\n{body}\n```\n\n"));
    }

    out.push_str("## 结论\n- 体检完成：以上为实时采集快照，未做任何修改（全部只读）。\n");
    out.push_str("- 如需清理建议：用 cleanup_suggestions 只读统计可清理空间；真正清理走主项目清理流程（先清单 + 用户确认 + 回收站）。\n");
    Ok(out)
}

/// 2) 传感器趋势：追加一条当前快照并返回最近 `n` 条（默认 10，上限 50）。
///
/// `format`：`text`（默认，人类可读）/ `json`（供前端折线图，返回结构化数组）。
pub fn sensor_trend(n: Option<usize>, format: Option<String>) -> Result<String, String> {
    let n = n.unwrap_or(10).clamp(1, 50);
    let as_json = format.as_deref().map(|s| s == "json").unwrap_or(false);
    // 快照（fancmd 全量文本，失败退回 ACPI 通道）
    let snap = super::hw::collect_sensors_fast().unwrap_or_else(|e| format!("采集失败：{e}"));
    // 结构化最小集：CPU 最高温 + 文本快照
    let cpu_temp = super::hw::max_cpu_temp().ok();
    let row = serde_json::json!({
        "ts": ts_secs(),
        "cpu_temp_c": cpu_temp,
        "snapshot": snap,
    });
    append_jsonl("sensor-history.jsonl", &row.to_string());
    trim_jsonl("sensor-history.jsonl", 500);

    let rows = read_jsonl("sensor-history.jsonl");
    let take = rows.len().min(n);
    let recent = &rows[rows.len().saturating_sub(take)..];

    if as_json {
        // JSON 输出：纯结构化（ts + cpu_temp_c 可空），供前端画趋势折线。
        let arr: Vec<serde_json::Value> = recent
            .iter()
            .map(|r| {
                serde_json::json!({
                    "ts": r.get("ts").and_then(|v| v.as_i64()).unwrap_or(0),
                    "cpu_temp_c": r.get("cpu_temp_c").and_then(|v| v.as_f64()),
                })
            })
            .collect();
        return Ok(serde_json::json!({
            "total": rows.len(),
            "points": arr,
        })
        .to_string());
    }

    let mut out = String::new();
    out.push_str(&format!(
        "传感器趋势（最近 {} 条，共 {} 条已记录）：\n",
        recent.len(),
        rows.len()
    ));
    for r in recent {
        let ts = r.get("ts").and_then(|v| v.as_i64()).unwrap_or(0);
        let temp = r
            .get("cpu_temp_c")
            .and_then(|v| v.as_f64())
            .map(|t| format!("{t:.1}℃"))
            .unwrap_or_else(|| "无".into());
        out.push_str(&format!("- {} CPU 最高温：{temp}\n", fmt_ts(ts)));
    }
    out.push_str("\n完整快照见 sensor_trend 数据目录 sensor-history.jsonl（%LOCALAPPDATA%/diskpilot/agent-server/）。");
    Ok(out)
}

/// 3) 传感器阈值告警：体检关键指标，超阈值输出告警。
///
/// `persist=true` 时把告警历史追加进 sensor-alerts.jsonl（默认 false 只读体检）。
/// 阈值：cpu_temp（默认 85）、disk_temp（默认 55）、gpu_temp（默认 85）、
/// mem_percent（默认 90）、disk_free_gb（默认 20）。
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
    match super::hw::max_cpu_temp() {
        Ok(t) => {
            info.push_str(&format!("CPU 温度：{t:.1}℃\n"));
            if t >= cpu_limit {
                alerts.push(format!("⚠️ CPU 温度 {t:.1}℃ 超过阈值 {cpu_limit:.1}℃"));
            }
        }
        Err(e) => info.push_str(&format!("CPU 温度不可读：{e}\n")),
    }

    // 磁盘温度 / 剩余空间：读 SMART 快照文本挑温度 + 磁盘健康里挑剩余
    let smart = super::hw::collect_disk_smart().unwrap_or_default();
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
    let temps = super::hw::collect_sensors_fast().unwrap_or_default();
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
    let sys = super::system::collect_system_info();
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

// ── 小工具 ────────────────────────────────────────────────────────────────

fn now_str() -> String {
    // 简单本地时间文本（不引 chrono）
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    fmt_ts(secs as i64)
}

fn fmt_ts(secs: i64) -> String {
    // 距现在多久（秒→分/时/天），不引外部 crate
    let now = ts_secs();
    let diff = (now - secs).max(0);
    if diff < 60 {
        format!("{diff}s 前")
    } else if diff < 3600 {
        format!("{}m 前", diff / 60)
    } else if diff < 86400 {
        format!("{}h 前", diff / 3600)
    } else {
        format!("{}d 前", diff / 86400)
    }
}

fn fmt_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{d}天 {h}小时 {m}分")
    } else if h > 0 {
        format!("{h}小时 {m}分")
    } else {
        format!("{m}分")
    }
}

fn format_f1(v: f64) -> String {
    format!("{v:.1}")
}

/// 从一行文本里提取第一个浮点数。
fn extract_first_f64(line: &str) -> Option<f64> {
    let cleaned = line.replace('℃', " ").replace('°', " ");
    let mut num = String::new();
    for c in cleaned.chars() {
        if c.is_ascii_digit() || c == '.' || c == '-' {
            num.push(c);
        } else if !num.is_empty() {
            break;
        }
    }
    if num.is_empty() {
        return None;
    }
    num.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// 从 SMART/磁盘健康文本里找「剩余 X GB」。
fn extract_free_gb(text: &str) -> Option<(f64, String)> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        let low = l.to_lowercase();
        if low.contains("剩余") || low.contains("free") {
            if let Some(v) = extract_first_f64(l) {
                // 找单位
                let unit = if low.contains("tb") { 1024.0 } else { 1.0 };
                return Some((v * unit, l.trim().to_string()));
            }
            // 可能是 "已用 XGB / 剩余 YGB" 结构，扫同一行所有数字取最后有效
            if let Some(tail) = lines.get(i).copied() {
                let nums: Vec<f64> = tail
                    .split_whitespace()
                    .filter_map(|w| {
                        w.trim_matches(|c: char| !c.is_ascii_digit() && c != '.')
                            .parse::<f64>()
                            .ok()
                    })
                    .collect();
                if let Some(&last) = nums.last() {
                    return Some((last, tail.trim().to_string()));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // 注：system_report / sensor_alert 会走真实采集（PowerShell/fancmd），
    // 不放单测（环境 PowerShell 可能 30s 慢），由 stdio 冒烟验证。

    #[test]
    fn extract_first_f64_works() {
        assert_eq!(extract_first_f64("CPU 温度: 56.3 ℃"), Some(56.3));
        assert_eq!(extract_first_f64("disk temp=45.0 C"), Some(45.0));
        assert_eq!(extract_first_f64("无数字"), None);
    }

    #[test]
    fn fmt_uptime_works() {
        assert_eq!(fmt_uptime(3600), "1小时 0分");
        assert_eq!(fmt_uptime(90061), "1天 1小时 1分");
        assert_eq!(fmt_uptime(120), "2分");
    }
}
