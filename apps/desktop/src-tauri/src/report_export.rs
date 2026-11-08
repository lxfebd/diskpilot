//! R526 定时自动导出扫描报告（health-butler 补充能力）：按配置间隔把
//! **已有扫描缓存**的路径导出为报告文件到用户指定目录。
//!
//! 铁律对齐 weekly_report：只读内存缓存树（scan_tree），**绝不为未扫描的
//! 路径触发全盘 walk**——没缓存的盘直接跳过并在通知里说明；不进 nonce 闸
//! （导出是只读渲染 + 写一份报告文件到用户选的目录，不是清理写路径）。
//!
//! - 配置：`report_export_enabled` / `report_export_interval_days` /
//!   `report_export_dir`（空 = 未设置，不跑）/ `report_export_format`（md|csv|json）。
//! - 文件名：`diskpilot-report-<根路径sanitized>-<yyyymmdd>.<ext>`，同一天已存在则跳过。
//! - 完成后 `app.emit("report-exported", …)` + 审计日志落盘。

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, Manager};

use crate::reminder::ReminderConfig;
use crate::AppState;

/// 把扫描根路径变成安全文件名段（盘符 C:\ → C，目录名去掉非法字符）。
pub(crate) fn sanitize_root_tag(root: &str) -> String {
    let t = root.trim_end_matches(['\\', '/']).trim();
    // 盘符根（"C:" / "C"）直接用盘符字母，别留下划线尾巴
    let t = if t.len() == 2 && t.as_bytes()[1] == b':' {
        &t[..1]
    } else {
        t
    };
    let t = t.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_");
    let t = t.trim_matches('_');
    if t.is_empty() {
        "root".to_string()
    } else {
        t.chars().take(40).collect()
    }
}

fn today_tag() -> String {
    // 无 chrono 依赖：用 SystemTime 手算 yyyymmdd（UTC 偏移对文件名不敏感）。
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    // 1970-01-01 起的天数 → 年月日（民用历换算，足够文件名用）。
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}{:02}{:02}", y, m, d)
}

/// 跑一次自动导出。返回成功写入的文件数。
/// 只导出已有扫描缓存的根；没缓存的跳过（零额外扫描铁律）。
pub(crate) fn run_report_export(app: &AppHandle, cfg: &ReminderConfig) -> (usize, usize) {
    let dir = cfg.report_export_dir.trim();
    if !cfg.report_export_enabled || dir.is_empty() {
        return (0, 0);
    }
    let out_dir = PathBuf::from(dir);
    if std::fs::create_dir_all(&out_dir).is_err() {
        return (0, 0);
    }
    let Some(fmt) = diskpilot_scanner::ReportFormat::parse(&cfg.report_export_format) else {
        return (0, 0);
    };
    let ext = match fmt {
        diskpilot_scanner::ReportFormat::Markdown => "md",
        diskpilot_scanner::ReportFormat::Csv => "csv",
        diskpilot_scanner::ReportFormat::Json => "json",
    };
    let state = app.state::<AppState>();
    let keys: Vec<String> = state.scan_tree.lock().unwrap().keys().cloned().collect();
    let stats_map = state.scan_stats.lock().unwrap().clone();
    let mut written = 0usize;
    let mut skipped = 0usize;
    let tag = today_tag();
    for key in keys {
        // key 是 norm_path_key 后的形式；还原成显示用路径
        let tree = match state.scan_tree.lock().unwrap().get(&key).cloned() {
            Some(t) => t,
            None => continue,
        };
        let display = tree.path.clone();
        let fname = format!(
            "diskpilot-report-{}-{}.{}",
            sanitize_root_tag(&display),
            tag,
            ext
        );
        let dest = out_dir.join(fname);
        if dest.exists() {
            skipped += 1;
            continue;
        }
        let stats = stats_map.get(&key).cloned();
        let content = diskpilot_scanner::render(&tree, stats.as_ref(), fmt, 50);
        if std::fs::write(&dest, content).is_ok() {
            written += 1;
        }
    }
    let _ = app.emit(
        "report-exported",
        serde_json::json!({
            "written": written,
            "skipped_existing": skipped,
            "dir": dir,
            "format": cfg.report_export_format,
        }),
    );
    // 审计：自动导出也要留痕（路径+件数），与其它后台动作同款
    crate::mcp::mcp_audit::append_audit(
        app,
        serde_json::json!({
            "op": "report_auto_export",
            "dir": dir,
            "format": cfg.report_export_format,
            "written": written,
            "skipped_existing": skipped,
        }),
    );
    (written, skipped)
}

pub(crate) fn report_export_dir_valid(p: &str) -> bool {
    !p.trim().is_empty() && Path::new(p.trim()).is_absolute()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_root_tag_strips_illegal() {
        assert_eq!(sanitize_root_tag("C:\\"), "C");
        assert_eq!(sanitize_root_tag("D:\\My Files\\"), "D__My Files");
        assert_eq!(sanitize_root_tag(""), "root");
        assert_eq!(sanitize_root_tag("   "), "root");
        // 超长截断到 40 字
        let long = "x".repeat(100);
        assert_eq!(sanitize_root_tag(&long).len(), 40);
    }

    #[test]
    fn report_export_dir_valid_requires_absolute() {
        assert!(report_export_dir_valid("C:\\reports"));
        // 跨平台：Unix 风格路径是否算绝对路径跟随平台判断
        assert_eq!(
            report_export_dir_valid("/home/u/reports"),
            std::path::Path::new("/home/u/reports").is_absolute()
        );
        assert!(!report_export_dir_valid("reports"));
        assert!(!report_export_dir_valid("   "));
    }
}
