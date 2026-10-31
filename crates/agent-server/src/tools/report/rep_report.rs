//! 体检报告/传感器趋势域（从 report.rs 拆分，2026-10-12）。
//! system_report + sensor_trend。

use crate::tools::{hw, system};

use super::rep_storage::{append_jsonl, data_dir_label, read_jsonl, trim_jsonl, ts_secs};
use super::rep_util::{fmt_ts, fmt_uptime, format_f1, now_str};

/// 一键体检报告：8 段串行只读采集（每段 PowerShell/CIM 1-5s，总 10-30s）。
/// `progress` 每完成一段回调一次 `(done, total)`——前端进度条按段推进，
/// 用户能看到「采到哪一段了」而不是干等 120s 超时。
pub fn system_report(progress: Option<&dyn Fn(usize, usize)>) -> Result<String, String> {
    let mut out = String::from("# DiskPilot 系统体检报告\n\n");
    out.push_str(&format!("生成时间：{}", now_str()));
    out.push('\n');

    let mut sections: Vec<(&str, String)> = Vec::new();
    const TOTAL: usize = 8;
    let report_progress = |done: usize| {
        if let Some(cb) = progress {
            cb(done, TOTAL);
        }
    };

    // CPU / GPU / 内存 / 主板
    for (name, f) in [
        ("CPU", hw::collect_cpu as fn() -> Result<String, String>),
        ("GPU", hw::collect_gpu),
        ("内存", hw::collect_memory),
        ("主板", hw::collect_motherboard),
    ] {
        sections.push((name, f().unwrap_or_else(|e| format!("采集失败：{e}"))));
        report_progress(sections.len());
    }
    // 温度（fancmd 全量或降级）
    sections.push((
        "温度",
        hw::collect_sensors_fast().unwrap_or_else(|e| format!("采集失败：{e}")),
    ));
    report_progress(sections.len());
    // 磁盘 SMART + 系统 + 电池
    sections.push((
        "磁盘健康",
        hw::collect_disk_smart().unwrap_or_else(|e| format!("采集失败：{e}")),
    ));
    report_progress(sections.len());
    let sys = system::collect_system_info();
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
    report_progress(sections.len());
    sections.push((
        "电池",
        hw::collect_battery().unwrap_or_else(|e| format!("采集失败：{e}")),
    ));
    report_progress(sections.len());

    for (name, body) in sections {
        out.push_str(&format!("## {name}\n```\n{body}\n```\n\n"));
    }

    out.push_str("## 结论\n- 体检完成：以上为实时采集快照，未做任何修改（全部只读）。\n");
    out.push_str("- 如需清理建议：用 cleanup_suggestions 只读统计可清理空间；真正清理走主项目清理流程（先清单 + 用户确认 + 回收站）。\n");
    Ok(out)
}

/// JSONL 行 → 折线图点（纯映射，可单测）。旧行没有 `mem_percent` / `gpu_temp_c`
/// 字段时该值为 null——前端折线图只画有值的曲线，不会把缺数据的点画成 0%。
pub(crate) fn trend_point(r: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "ts": r.get("ts").and_then(|v| v.as_i64()).unwrap_or(0),
        "cpu_temp_c": r.get("cpu_temp_c").and_then(|v| v.as_f64()),
        "gpu_temp_c": r.get("gpu_temp_c").and_then(|v| v.as_f64()),
        "mem_percent": r.get("mem_percent").and_then(|v| v.as_f64()),
    })
}

/// 从已读的 JSONL 行里取最近 `n` 条并转折线图点（纯函数，可单测）：
/// 按行序保持数组有序（旧→新），取尾部 n 条；n 缺失或 0=全部。
pub(crate) fn points_from_rows(
    rows: &[serde_json::Value],
    n: Option<usize>,
) -> Vec<serde_json::Value> {
    let take = n
        .filter(|v| *v > 0)
        .map(|v| v.clamp(1, 500))
        .unwrap_or(rows.len())
        .min(rows.len());
    rows.iter()
        .skip(rows.len().saturating_sub(take))
        .map(trend_point)
        .collect()
}

/// 2) 传感器趋势：追加一条当前快照并返回最近 `n` 条（默认 10，上限 50）。
///
/// `format`：`text`（默认，人类可读）/ `json`（供前端折线图，返回结构化数组）。
pub fn sensor_trend(n: Option<usize>, format: Option<String>) -> Result<String, String> {
    let n = n.unwrap_or(10).clamp(1, 50);
    let as_json = format.as_deref().map(|s| s == "json").unwrap_or(false);
    // 快照（fancmd 全量文本，失败退回 ACPI 通道）
    let snap = hw::collect_sensors_fast().unwrap_or_else(|e| format!("采集失败：{e}"));
    // 结构化最小集：CPU 最高温 + GPU 温度 + 内存占用% + 文本快照。
    // mem_percent 来自 GlobalMemoryStatusEx（一次 syscall）；非 Windows 或
    // 读数不可用时记 null——前端折线图只画有值的曲线，不会画出假 0%。
    // gpu_temp_c 走 nvidia-smi / ADL 双通道（hw::gpu_temp_c），无 GPU 或失败
    // 记 null，同样不虚构。
    let cpu_temp = hw::max_cpu_temp().ok();
    let gpu_temp = hw::gpu_temp_c().ok();
    let sys = system::collect_system_info();
    let mem_percent = if sys.mem_total_bytes > 0 {
        Some(sys.mem_percent)
    } else {
        None
    };
    let row = serde_json::json!({
        "ts": ts_secs(),
        "cpu_temp_c": cpu_temp,
        "gpu_temp_c": gpu_temp,
        "mem_percent": mem_percent,
        "snapshot": snap,
    });
    append_jsonl("sensor-history.jsonl", &row.to_string());
    trim_jsonl("sensor-history.jsonl", 500);

    let rows = read_jsonl("sensor-history.jsonl");
    let take = rows.len().min(n);
    let recent = &rows[rows.len().saturating_sub(take)..];

    if as_json {
        // JSON 输出：纯结构化（ts + cpu_temp_c 可空），供前端画趋势折线。
        let arr: Vec<serde_json::Value> = recent.iter().map(trend_point).collect();
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
        let gpu = r
            .get("gpu_temp_c")
            .and_then(|v| v.as_f64())
            .map(|t| format!(" / GPU {t:.1}℃"))
            .unwrap_or_default();
        let mem = r
            .get("mem_percent")
            .and_then(|v| v.as_f64())
            .map(|p| format!(" / 内存 {p:.0}%"))
            .unwrap_or_default();
        out.push_str(&format!("- {} CPU 最高温：{temp}{gpu}{mem}\n", fmt_ts(ts)));
    }
    out.push_str(&format!(
        "\n完整快照见 sensor_trend 数据目录 sensor-history.jsonl（{}）。",
        data_dir_label()
    ));
    Ok(out)
}

/// 3) 只读传感器历史。
///
/// 不给 sensor_trend 的默认行为——它每次调用都会追加一条新快照，不适合纯展示读取。
/// 本函数只读 `sensor-history.jsonl` 现有内容，不追加、不裁剪、不触发采集。
/// `n` 取最近 n 条（默认全部，上限 500，与文件裁剪上限一致）；
/// `format=json` 返回结构化数组（同 trend_point 契约）。
pub fn sensor_history(n: Option<usize>, format: Option<String>) -> Result<String, String> {
    let rows = read_jsonl("sensor-history.jsonl");
    let points = points_from_rows(&rows, n);
    if format.as_deref().map(|s| s == "json").unwrap_or(false) {
        return Ok(serde_json::json!({ "total": rows.len(), "points": points }).to_string());
    }
    let mut out = String::from("传感器历史（只读，不追加采集）：\n");
    out.push_str(&format!(
        "共 {} 条已记录，本次返回最近 {} 条：\n",
        rows.len(),
        points.len()
    ));
    for r in points {
        let ts = r.get("ts").and_then(|v| v.as_i64()).unwrap_or(0);
        let temp = r
            .get("cpu_temp_c")
            .and_then(|v| v.as_f64())
            .map(|t| format!("{t:.1}℃"))
            .unwrap_or_else(|| "无".into());
        let gpu = r
            .get("gpu_temp_c")
            .and_then(|v| v.as_f64())
            .map(|t| format!(" / GPU {t:.1}℃"))
            .unwrap_or_default();
        let mem = r
            .get("mem_percent")
            .and_then(|v| v.as_f64())
            .map(|p| format!(" / 内存 {p:.0}%"))
            .unwrap_or_default();
        out.push_str(&format!("- {} CPU 最高温：{temp}{gpu}{mem}\n", fmt_ts(ts)));
    }
    out.push_str(&format!(
        "\n数据源：{}/sensor-history.jsonl（sensor_trend 追加写入）。",
        data_dir_label()
    ));
    Ok(out)
}
