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

/// R529 超温提示（纯函数，可单测）：本周最高温超过持续偏高阈值 → 给提示；
/// 未开持续采样预警时，提示里建议去设置里打开（周报只读，不自行改配置）。
pub(crate) fn temp_alert_note(
    temp_max_c: Option<f32>,
    cfg: &crate::reminder::ReminderConfig,
) -> Option<String> {
    let t = temp_max_c?;
    if t < cfg.thermal_sustained_c {
        return None;
    }
    Some(if cfg.thermal_sample_enabled {
        format!(
            "本周最高温 {:.0}℃ 已达持续偏高阈值 {:.0}℃，持续采样预警已开启，留意通知。",
            t, cfg.thermal_sustained_c
        )
    } else {
        format!(
            "本周最高温 {:.0}℃ 超过 {:.0}℃ 阈值，但未开启持续采样预警——建议在设置里打开。",
            t, cfg.thermal_sustained_c
        )
    })
}

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
    /// 每块物理盘的健康摘要（带盘符）。旧归档没有这个字段 → 前端按空处理。
    #[serde(default)]
    pub disks: Vec<DiskHealthBrief>,
    /// 盘健康读取失败的原因（R86）：非空说明「读失败」而不是「没读到」。
    /// 旧归档没有此字段 → 前端按「旧格式无数据」处理（不显示失败提示）。
    #[serde(default)]
    pub disks_note: Option<String>,
    /// R529 超温提示：本周最高温超过提醒配置里的持续偏高阈值时给一句诚实提示
    /// （不编造预警、不自行开采样开关）。旧归档无此字段 → None。
    #[serde(default)]
    pub temp_alert: Option<String>,
    /// 各扫描根最近两次扫描的环比趋势行（R486，后端已格式化）。
    /// 旧归档没有此字段 → 前端整块不显示（不猜）。
    #[serde(default)]
    pub scan_trends: Vec<String>,
    /// 观测层异常告警行（损坏行 / 读取失败的域，R486）。无异常则空。
    /// 旧归档没有此字段 → 前端整块不显示。
    #[serde(default)]
    pub obs_alerts: Vec<String>,
    /// R541：异常盘清单行（grade 为 caution / bad 的盘）。全部健康则空——
    /// 空 = 真没异常，与 disks 为空（读失败）靠 disks_note 区分，不混淆。
    /// 旧归档没有此字段 → 前端整块不显示。
    #[serde(default)]
    pub disk_alerts: Vec<String>,
}

/// 周报里的单块盘健康摘要。`health` 是后端原值（Healthy / Warning / …），
/// `grade` 是统一分级（good / caution / bad / unknown），`letters` 可能为空
/// （映射不到盘符时不猜）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DiskHealthBrief {
    pub friendly: String,
    pub letters: String,
    pub health: String,
    pub grade: String,
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
        format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            y,
            m,
            d,
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        )
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
    // 1) 各盘建议可清合计（复用 cleanup_suggestions，cached_only=true：
    //    只读缓存/扫描树，未命中返回空，绝不全盘 walk）。
    let substs = crate::drives::collect_substs();
    let mut drives: Vec<DriveCleanupBrief> = Vec::new();
    let mut total: u64 = 0;
    for letter in b'C'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if crate::drives::volume_info(root.clone()).is_err() {
            continue;
        }
        // 映射盘（subst D: J:\D）不是独立物理盘：目标卷自己也在这一轮里被算过，
        // 再算一遍就把同一块盘的空间与可清量数两遍。
        if crate::drives::mapped_into_local_volume(letter as char, &substs) {
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
        .and_then(|v| {
            v.get("temp_max_c")
                .and_then(|x| x.as_f64())
                .map(|x| x as f32)
        });

    // 3) 本周硬件快照（hw_history 归档只读），首尾对比温度/通电。
    // 历史目录读失败（R87 起 get_hw_history 报 Err）降级为「无快照」：
    // 周报是聚合概览，不为单个数据源失败整体作废。
    let snaps = crate::hw_history::get_hw_history(app.clone()).unwrap_or_default();
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

    // 4) 每块物理盘的健康摘要（hw_disk_cached 带 5 分钟 TTL，周报是后台作业，跑一次
    //    PowerShell 可以接受）。quick=true：周报只要 WMI 的健康分级，不该在后台
    //    定时启动 CrystalDiskInfo。读失败要带原因（disks_note），不能让周报
    //    假装「盘都没有」——前端据此区分「读失败」与「旧格式无此数据」（R86）。
    let (disks, disks_note) = match crate::hw::hw_disk::hw_disk_cached(app, false, true) {
        Ok(v) => (
            v.get("disks")
                .and_then(|d| d.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|d| {
                            let health = d
                                .get("health")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string();
                            DiskHealthBrief {
                                friendly: d
                                    .get("friendly")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                letters: d
                                    .get("drive_letters")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                grade: crate::hw::hw_disk::grade_health(&health).to_string(),
                                health,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
            None,
        ),
        Err(e) => (Vec::new(), Some(format!("硬盘健康读取失败：{e}"))),
    };

    // 5) R486：扫描趋势环比（scan-summary.jsonl 各 root 最近两条）+ 观测层异常
    //    （observations_overview 的 domains 里 corrupt_lines>0 或 error 的域）。
    //    两个数据源失败都降级为空行——周报是聚合概览，不为单个数据源作废，
    //    但也不伪造「一切正常」（obs_alerts 空 = 真没异常，字段存在才代表读到过）。
    let scan_trends = crate::scan_summary::read_summaries(app)
        .map(|s| scan_trend_lines(&s))
        .unwrap_or_default();
    let obs_alerts = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| obs_alert_lines(&crate::observations::build_overview(&d)))
        .unwrap_or_default();

    // R529：超温提示（纯函数判定，阈值取 reminder 配置，未开持续采样时提示去打开）
    let rcfg = crate::reminder::config_at(app);
    let temp_alert = temp_alert_note(temp_max_c, &rcfg);

    // R541：异常盘清单（与 R540 硬件卡同一 grade 语义）
    let disk_alerts = disk_alert_lines(&disks);

    Ok(WeeklyReport {
        ts: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        drives,
        total_bytes: total,
        temp_max_c,
        temp_alert,
        hw_snapshots,
        hw_delta,
        disks,
        disks_note,
        scan_trends,
        obs_alerts,
        disk_alerts,
    })
}

/// R541：从盘健康摘要里挑出异常盘（caution / bad）格式化为周报行。
/// 纯函数：grade 为 good / unknown / 空都不算异常（unknown 是「没读到」，不报警）。
pub(crate) fn disk_alert_lines(disks: &[DiskHealthBrief]) -> Vec<String> {
    disks
        .iter()
        .filter(|d| d.grade == "caution" || d.grade == "bad")
        .map(|d| {
            let loc = if d.letters.trim().is_empty() {
                d.friendly.clone()
            } else {
                format!("{}: {}", d.letters, d.friendly)
            };
            format!("{loc} — {}（{}）", d.health, d.grade)
        })
        .collect()
}

/// 带符号的体量文案：+1.2 GB / -300.0 MB（R486 趋势行用）。
fn fmt_bytes_signed(b: i64) -> String {
    let a = (b as f64).abs();
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = a;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let sign = if b < 0 { "-" } else { "+" };
    format!("{sign}{v:.1} {}", units[u])
}

/// R486：按 root 分组，取文件序里最近两条做环比，产出一行中文趋势。
/// 纯函数：同一 root 不足两条、时间戳倒序都跳过（不编造对比）。
fn scan_trend_lines(summaries: &[crate::scan_summary::ScanSummary]) -> Vec<String> {
    use std::collections::HashMap;
    let mut order: Vec<&str> = Vec::new();
    let mut groups: HashMap<&str, Vec<&crate::scan_summary::ScanSummary>> = HashMap::new();
    for s in summaries {
        if !groups.contains_key(s.root.as_str()) {
            order.push(s.root.as_str());
        }
        groups.entry(s.root.as_str()).or_default().push(s);
    }
    let mut lines: Vec<String> = Vec::new();
    for root in order {
        let g = &groups[root];
        if g.len() < 2 {
            continue;
        }
        let prev = &g[g.len() - 2];
        let cur = &g[g.len() - 1];
        if cur.at <= prev.at {
            continue;
        }
        let days = (cur.at - prev.at) / 86400;
        let fdelta = cur.file_count as i64 - prev.file_count as i64;
        let bdelta = cur.total_bytes as i64 - prev.total_bytes as i64;
        lines.push(format!(
            "{} 文件 {fdelta:+}，体量 {}（距上次 {days} 天）",
            root,
            fmt_bytes_signed(bdelta)
        ));
        if lines.len() >= 8 {
            break;
        }
    }
    lines
}

/// R486：从 observations_overview 的 domains 里挑出异常域——读取失败（error）
/// 或损坏行（corrupt_lines > 0）。纯函数：结构不符返回空。
fn obs_alert_lines(overview: &serde_json::Value) -> Vec<String> {
    let Some(domains) = overview.get("domains").and_then(|d| d.as_object()) else {
        return Vec::new();
    };
    let mut lines: Vec<String> = Vec::new();
    for (name, e) in domains {
        if let Some(err) = e.get("error").and_then(|x| x.as_str()) {
            lines.push(format!("{name} 读取失败：{err}"));
        } else if let Some(c) = e.get("corrupt_lines").and_then(|x| x.as_u64()) {
            if c > 0 {
                lines.push(format!("{name} 有 {c} 行损坏（已跳过，文件未丢弃）"));
            }
        }
    }
    lines
}

/// 归档周报：写入 weekly-report/*.json，裁剪旧份（最多 MAX_REPORTS）。
pub(crate) fn archive_weekly_report(
    app: &AppHandle,
    report: &WeeklyReport,
) -> Result<PathBuf, String> {
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
            drives: vec![DriveCleanupBrief {
                path: "C:\\".into(),
                bytes: 100,
            }],
            total_bytes: 100,
            temp_max_c: Some(72.5),
            hw_snapshots: 2,
            hw_delta: vec!["C temp +3".into()],
            disks: vec![
                DiskHealthBrief {
                    friendly: "WD_BLACK SN770 1TB".into(),
                    letters: "C".into(),
                    health: "Healthy".into(),
                    grade: "good".into(),
                },
                DiskHealthBrief {
                    friendly: "Old HDD".into(),
                    letters: "F,G".into(),
                    health: "Warning".into(),
                    grade: "caution".into(),
                },
            ],
            disks_note: None,
            temp_alert: None,
            scan_trends: vec!["C:\\ 文件 +12，体量 +1.0 GB（距上次 7 天)".into()],
            obs_alerts: vec!["undo 有 2 行损坏（已跳过，文件未丢弃）".into()],
            disk_alerts: vec!["F,G: Old HDD — Warning（caution）".into()],
        };
        let text = serde_json::to_string_pretty(&r).unwrap();
        let back: WeeklyReport = serde_json::from_str(&text).unwrap();
        assert_eq!(back.ts, 123);
        assert_eq!(back.total_bytes, 100);
        assert_eq!(back.temp_max_c, Some(72.5));
        assert_eq!(back.hw_delta, vec!["C temp +3"]);
        assert_eq!(back.disks.len(), 2);
        assert_eq!(back.disks[1].letters, "F,G");
        assert_eq!(back.disks[1].grade, "caution");
        assert_eq!(back.scan_trends.len(), 1);
        assert_eq!(back.obs_alerts.len(), 1);
    }

    #[test]
    fn disk_alert_lines_r541() {
        let mk = |grade: &str, letters: &str| DiskHealthBrief {
            friendly: "Samsung 980".into(),
            letters: letters.into(),
            health: "Warning".into(),
            grade: grade.into(),
        };
        // good / unknown 不算异常（unknown 是没读到，不报警）
        assert!(disk_alert_lines(&[mk("good", "C"), mk("unknown", "D")]).is_empty());
        // caution / bad 入列，带盘符与原始 health 文案
        let alerts = disk_alert_lines(&[mk("bad", "C"), mk("caution", "")]);
        assert_eq!(alerts.len(), 2);
        assert!(
            alerts[0].contains("C:") && alerts[0].contains("bad") && alerts[0].contains("Warning")
        );
        // 无盘符时退化为型号名
        assert!(alerts[1].starts_with("Samsung 980 —"));
    }

    #[test]
    fn temp_alert_note_thresholds() {
        let mut cfg = crate::reminder::ReminderConfig::default(); // thermal_sustained_c = 85
                                                                  // 低于阈值 → 无提示
        assert!(temp_alert_note(Some(70.0), &cfg).is_none());
        assert!(temp_alert_note(None, &cfg).is_none());
        // 超阈值 + 未开采样 → 提示建议打开
        cfg.thermal_sample_enabled = false;
        let a = temp_alert_note(Some(90.0), &cfg).unwrap();
        assert!(a.contains("90") && a.contains("85") && a.contains("未开启"));
        // 超阈值 + 已开采样 → 提示留意通知
        cfg.thermal_sample_enabled = true;
        let b = temp_alert_note(Some(90.0), &cfg).unwrap();
        assert!(b.contains("已开启"));
        // 恰等于阈值也算达标（>= 触发）；差一点不达标
        assert!(temp_alert_note(Some(85.0), &cfg).is_some());
        assert!(temp_alert_note(Some(84.9), &cfg).is_none());
    }

    #[test]
    fn old_archive_without_disk_field_still_reads() {
        // 已归档的旧周报没有 disks：serde(default) 必须让它照样读得出来，
        // 否则历史页读旧档会整份失败。
        let json = r#"{"ts":1,"drives":[],"total_bytes":0,"temp_max_c":null,"hw_snapshots":0,"hw_delta":[]}"#;
        let back: WeeklyReport = serde_json::from_str(json).unwrap();
        assert!(back.disks.is_empty());
        // 旧格式也没有 disks_note（R86 新字段）→ 前端据此不显示失败提示。
        assert!(back.disks_note.is_none());
        // R486 新字段同样 serde(default)：旧归档读出为空
        assert!(back.scan_trends.is_empty());
        assert!(back.obs_alerts.is_empty());
    }

    #[test]
    fn disks_note_roundtrips_when_set() {
        // 生成时盘健康读失败 → disks_note 带原因存档；读回必须保留，
        // 前端据此显示「读失败」而不是「旧格式无数据」。
        let r = WeeklyReport {
            ts: 1,
            drives: Vec::new(),
            total_bytes: 0,
            temp_max_c: None,
            hw_snapshots: 0,
            hw_delta: Vec::new(),
            disks: Vec::new(),
            disks_note: Some("硬盘健康读取失败：timeout".into()),
            temp_alert: None,
            scan_trends: Vec::new(),
            obs_alerts: Vec::new(),
            disk_alerts: Vec::new(),
        };
        let text = serde_json::to_string(&r).unwrap();
        let back: WeeklyReport = serde_json::from_str(&text).unwrap();
        assert_eq!(
            back.disks_note.as_deref(),
            Some("硬盘健康读取失败：timeout")
        );
    }

    fn mk_summary(root: &str, at: u64, files: u64, bytes: u64) -> crate::scan_summary::ScanSummary {
        crate::scan_summary::ScanSummary {
            v: 1,
            root: root.to_string(),
            at,
            duration_ms: 1000,
            mode: "full".into(),
            file_count: files,
            dir_count: 10,
            total_bytes: bytes,
            top_dirs: Vec::new(),
            top_files: Vec::new(),
            buckets: Vec::new(),
        }
    }

    #[test]
    fn scan_trend_lines_compares_latest_two_per_root() {
        // C: 三条历史只对比最近两条；D: 只有一条 → 跳过（不编造对比）
        let s = vec![
            mk_summary("C:\\", 100, 1000, 1_000_000),
            mk_summary("D:\\", 100, 5, 50),
            mk_summary("C:\\", 100 + 7 * 86400, 1500, 3_221_225_472),
            mk_summary("C:\\", 100 + 14 * 86400, 1200, 1_073_741_824),
        ];
        let lines = scan_trend_lines(&s);
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert!(l.starts_with("C:\\"), "{l}");
        // 最近两条：at=14d vs 7d，文件 -300，体量 1GiB - 3GiB = -2.0 GB，间隔 7 天
        assert!(l.contains("-300"), "{l}");
        assert!(l.contains("-2.0 GB"), "{l}");
        assert!(l.contains("7 天"), "{l}");
    }

    #[test]
    fn obs_alert_lines_flags_corrupt_and_error_only() {
        let v = serde_json::json!({
            "v": 1,
            "domains": {
                "scan_summary": {"kind": "jsonl", "entries": 5, "corrupt_lines": 2},
                "undo": {"kind": "jsonl", "error": "全部行损坏"},
                "general": {"kind": "state", "present": true},
                "space_history": {"kind": "jsonl", "entries": 9, "corrupt_lines": 0}
            }
        });
        let lines = obs_alert_lines(&v);
        assert_eq!(lines.len(), 2);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("scan_summary") && l.contains('2')),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("undo") && l.contains("全部行损坏")),
            "{lines:?}"
        );
        // 结构不符（没有 domains）→ 空而不是 panic
        assert!(obs_alert_lines(&serde_json::json!({"v": 1})).is_empty());
    }
}
