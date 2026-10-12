//! 格式化/解析小工具域（从 report.rs 拆分，2026-10-12）。
//! now_str / fmt_ts / fmt_uptime / format_f1 / extract_first_f64 / extract_free_gb。

use super::rep_storage::ts_secs;

// ── 小工具 ────────────────────────────────────────────────────────────────

pub(crate) fn now_str() -> String {
    // 简单本地时间文本（不引 chrono）
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    fmt_ts(secs as i64)
}

pub(crate) fn fmt_ts(secs: i64) -> String {
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

pub(crate) fn fmt_uptime(secs: u64) -> String {
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

pub(crate) fn format_f1(v: f64) -> String {
    format!("{v:.1}")
}

/// 从一行文本里提取第一个浮点数。
pub(crate) fn extract_first_f64(line: &str) -> Option<f64> {
    let cleaned = line.replace(['℃', '°'], " ");
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

/// 从 SMART/磁盘健康文本里找「剩余 X GB/TB」。
///
/// 三条防错规则（否则会把磨损度当成剩余容量、把已用值当成剩余值）：
/// 1. 只认「剩余/free + 容量单位（GB/TB/G）」的行；
/// 2. 跳过含 `%` 的行——`寿命剩余约 X%` 是磨损度百分比，不是剩余容量。
///    磨损行一旦被命中，健康 SSD 会收到虚假的「磁盘剩余低于阈值」告警；
/// 3. 「已用 XGB / 剩余 YGB」结构以关键词为锚取词**之后**的值（剩余），
///    不取行首数字（已用）。
pub(crate) fn extract_free_gb(text: &str) -> Option<(f64, String)> {
    for l in text.lines() {
        let low = l.to_lowercase();
        if !(low.contains("剩余") || low.contains("free")) || low.contains('%') {
            continue;
        }
        if !(low.contains("gb") || low.contains("tb") || low.contains("g ")) {
            continue;
        }
        let unit = if low.contains("tb") { 1024.0 } else { 1.0 };
        let nums = |s: &str| -> Vec<f64> {
            s.split_whitespace()
                .filter_map(|w| {
                    let n: String = w
                        .chars()
                        .filter(|c| c.is_ascii_digit() || *c == '.')
                        .collect();
                    if n.is_empty() {
                        return None;
                    }
                    n.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
                })
                .collect()
        };
        // 锚定关键词：只取「剩余/free」之后的数字（排除行首的已用值）
        let kw = low.find("剩余").or_else(|| low.find("free"))?;
        let after = nums(&low[kw..]);
        let v = if !after.is_empty() {
            after[0]
        } else {
            // 兜底：关键词在行尾（如 "900 GB free"），整行取最后一个有效数字
            *nums(l).last()?
        };
        return Some((v * unit, l.trim().to_string()));
    }
    None
}
