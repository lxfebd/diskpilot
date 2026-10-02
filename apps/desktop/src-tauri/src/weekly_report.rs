//! 周巡检简报（health-butler P1）：每周一次只读打包体检，产出「本周健康简报」。
//!
//! 设计对齐 reminder（R3）：复用 `std::thread::spawn + loop + sleep` 调度、
//! `cleanup_suggestions` 引擎（cached_only=true，只读缓存树、**绝不为未扫描盘
//! 触发全盘 walk**，后台高频必须轻量）、`app.emit` 通知通道。巡检本身绝不
//! 写盘、绝不清理——写路径仍由总览确认窗（user_confirmed 硬校验）独占。
//!
//! - **数据**：各盘建议可清合计（cleanup_suggestions）+ 最高温（fan_curve_advice
//!   TTL 缓存）+ 本周硬件快照（hw_history 归档，首尾对比 temperature/通电）。
//! - **归档**：`app_data_dir()/weekly-report/*.json`，仿 hw-history 命名/裁剪
//!   （最多保留 12 份）。生成失败静默（后台线程不打扰）。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

/// 单份周报（归档到 weekly-report/*.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WeeklyReport {
    /// 生成时间（unix 秒）。
    pub ts: u64,
    /// 各盘建议可清合计（只读统计）。
    pub drives: Vec<DriveCleanupBrief>,
    /// 全部盘可清合计。
    pub total_bytes: u64,
    /// 最高温（本轮采样，若无则 null）。
    pub temp_max_c: Option<f32>,
    /// 本周有归档的硬件快照数。
    pub hw_snapshots: usize,
    /// 周内硬件变化简要（首尾温度差 / 通电差），无则空。
    pub hw_delta: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DriveCleanupBrief {
    pub path: String,
    pub bytes: u64,
}

fn reports_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .map(|d| d.join("weekly-report"))
        .unwrap_or_else(|_| PathBuf::from("weekly-report"))
}

fn next_report_path(app: &AppHandle) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // yyyyMMdd-HHmmss，与 hw-history 命名风格一致。
    let stamp = {
        let secs = ts % 86400;
        let days = ts / 86400;
        let (y, m, d) = civil_from_days(days as i64);
        format!("{:04}{:02}{:02}-{:02}{:02}{:02}", y, m, d, secs / 3600, (secs % 3600) / 60, secs % 60)
    };
    reports_dir(app).join(format!("weekly-{stamp}.json"))
}

/// 天数 → 年月日（无 chrono 依赖的简单儒略日转换，测试覆盖）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 生成一份周报（手动触发 + 后台线程共用）：只读聚合，绝不清理。
/// 返回最新周报；后台线程里调用时用 `tauri::async_runtime::block_on` 包 async 调用。
pub(crate) fn generate_weekly_report(app: &AppHandle) -> Result<WeeklyReport, String> {
    let state = app.state::<AppState>();
    // 1) 各盘建议可清合计（复用 cleanup_suggestions，cached_only=false 真算）。
    let mut drives: Vec<DriveCleanupBrief> = Vec::new();
    let mut total: u64 = 0;
    for letter in b'C'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if crate::volume_info(root.clone()).is_err() {
            continue;
        }
        let suggs = tauri::async_runtime::block_on(crate::cleanup::cleanup_suggestions(
            state.clone(),
            root.clone(),
            None,
            Some(true),
        ))?;
        let bytes: u64 = suggs.iter().map(|s| s.bytes).sum();
        if bytes > 0 {
            drives.push(DriveCleanupBrief { path: root, bytes });
            total = total.saturating_add(bytes);
        }
    }

    // 2) 最高温（fan_curve_advice 带 TTL 缓存，失败降级 null）。
    let temp_max_c = tauri::async_runtime::block_on(crate::hw::fan_curve_advice(None))
        .ok()
        .and_then(|v| v.get("temp_max_c").and_then(|x| x.as_f64()).map(|x| x as f32));

    // 3) 本周硬件快照（hw_history 归档只读），首尾对比温度/通电。
    let snaps = crate::hw_history::get_hw_history(app.clone());
    let hw_snapshots = snaps.len();
    let hw_delta = if snaps.len() >= 2 {
        // 取最近两份归档对比（周首 vs 周末）：compare_hw_snapshots 只读。
        crate::hw_history::compare_hw_snapshots(app.clone(), None, None)
            .ok()
            .and_then(|v| v.get("disks").cloned())
            .and_then(|d| d.as_array().cloned())
            .map(|disks| {
                let mut lines: Vec<String> = Vec::new();
                for disk in &disks {
                    let device = disk.get("device").and_then(|x| x.as_str()).unwrap_or("");
                    if let Some(fields) = disk.get("fields").and_then(|f| f.as_array()) {
                        for f in fields {
                            let name = f.get("name").and_then(|x| x.as_str()).unwrap_or("");
                            let new = f.get("new").and_then(|x| x.as_f64());
                            let old = f.get("old").and_then(|x| x.as_f64());
                            if let (Some(nv), Some(ov)) = (new, old) {
                                let delta = nv - ov;
                                if delta.abs() >= 1.0 {
                                    lines.push(format!("{device} {name} {delta:+.0}"));
                                }
                            }
                        }
                    }
                }
                lines.truncate(6);
                lines
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(WeeklyReport {
        ts: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        drives,
        total_bytes: total,
        temp_max_c,
        hw_snapshots,
        hw_delta,
    })
}

/// 归档周报：写入 weekly-report/*.json，裁剪旧份（最多 MAX_REPORTS）。
pub(crate) fn archive_weekly_report(app: &AppHandle, report: &WeeklyReport) -> Result<PathBuf, String> {
    const MAX_REPORTS: usize = 12;
    let dir = reports_dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    let p = next_report_path(app);
    std::fs::write(&p, text).map_err(|e| e.to_string())?;
    // 裁剪旧份：按文件名排序保留最近 MAX_REPORTS。
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort();
    while files.len() > MAX_REPORTS {
        if let Some(old) = files.first() {
            let _ = std::fs::remove_file(old);
            files.remove(0);
        }
    }
    Ok(p)
}

/// 生成 + 归档 + emit（前端手动按钮 / 后台线程共用）。
pub(crate) fn run_weekly_report(app: &AppHandle) -> Result<WeeklyReport, String> {
    let report = generate_weekly_report(app)?;
    let _ = archive_weekly_report(app, &report);
    let _ = app.emit("health-weekly", &report);
    Ok(report)
}

/// 手动生成一次周报（只读 L0）。
#[tauri::command]
pub(crate) fn generate_weekly_report_cmd(app: AppHandle) -> Result<WeeklyReport, String> {
    run_weekly_report(&app)
}

/// 读最近 N 份周报（只读 L0，历史页回显）。
#[tauri::command]
pub(crate) fn get_weekly_reports(app: AppHandle, limit: Option<usize>) -> Vec<WeeklyReport> {
    let limit = limit.unwrap_or(5).min(30);
    let mut files: Vec<PathBuf> = std::fs::read_dir(reports_dir(&app))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort();
    files.reverse();
    files
        .into_iter()
        .take(limit)
        .filter_map(|p| {
            std::fs::read_to_string(&p)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_known_dates() {
        // 天数 = 距 1970-01-01 的天数：2026-01-01 = 20454（56 年 20440 + 14 闰日）；
        // 2026-10-01 = 20454 + 273（前九月 31+28+31+30+31+30+31+31+30）= 20727。
        assert_eq!(civil_from_days(20454), (2026, 1, 1));
        assert_eq!(civil_from_days(20727), (2026, 10, 1));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn weekly_report_serialize_roundtrip() {
        let r = WeeklyReport {
            ts: 123,
            drives: vec![DriveCleanupBrief { path: "C:\\".into(), bytes: 100 }],
            total_bytes: 100,
            temp_max_c: Some(72.5),
            hw_snapshots: 2,
            hw_delta: vec!["C temp +3".into()],
        };
        let text = serde_json::to_string_pretty(&r).unwrap();
        let back: WeeklyReport = serde_json::from_str(&text).unwrap();
        assert_eq!(back.ts, 123);
        assert_eq!(back.total_bytes, 100);
        assert_eq!(back.temp_max_c, Some(72.5));
        assert_eq!(back.hw_delta, vec!["C temp +3"]);
    }
}