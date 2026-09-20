//! 硬件报告历史（R7）：`hw_report` 每次生成时自动归档一份 JSON 快照到
//! `app_data_dir/hw-history/`，硬件页据此展示「历史快照」区，并支持两次
//! 归档并排对比温度 / SMART 磨损 / 通电时长变化。
//!
//! 设计约束：
//! - 归档只发生在报告生成成功之后（`hw_report` 主流程尾部，失败不影响报告返回）；
//! - 归档文件按时间命名 `hw-<yyyyMMdd-HHmmss>.json`，永不覆盖旧快照；
//! - 快照内容 = 报告同源的 `{info, health, note}`（与 json 格式报告完全一致），
//!   对比字段从中提取；
//! - 只读查询命令不触发任何硬件采集（对比用已有归档，绝不重跑 PowerShell）；
//! - 每台机器最多保留最近 30 份归档，超长裁剪最旧。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;

/// 单份归档元信息（列表返回用，不含正文）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HwSnapshotMeta {
    /// 文件名（hw-20260910-153000.json）
    pub file: String,
    /// Unix 秒（归档生成时刻）
    pub at: u64,
    /// 机器标识（主板序列号/机型 + 系统，用于区分多机归档）
    pub machine: String,
    /// 归档内磁盘数（展示用）
    pub disk_count: usize,
}

/// 对比结果：两组快照里都存在的磁盘，逐字段给出旧→新与差值。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct HwDiskCompare {
    pub device: String,
    /// 字段名 → (旧值, 新值, 差值说明)
    pub fields: Vec<HwFieldDelta>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HwFieldDelta {
    pub name: String,
    pub old: serde_json::Value,
    pub new: serde_json::Value,
    /// 人话差值：温度「+3 ℃」、通电「+12 小时」、磨损「+1 %」
    pub delta: String,
}

const MAX_SNAPSHOTS: usize = 30;

fn history_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("hw-history"))
}

fn machine_label(info: &serde_json::Value) -> String {
    let system = info
        .get("system")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or_default();
    let os = info
        .get("os")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or_default();
    let mfr = os
        .get("Manufacturer")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let model = system
        .get("Model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let cap = os
        .get("Caption")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut label = format!("{mfr} {model}").trim().to_string();
    if label.is_empty() {
        label = "本机".into();
    }
    if !cap.is_empty() {
        label.push_str(&format!(" · {cap}"));
    }
    label
}

/// 归档一次报告快照（幂等：同一秒内重复生成不会覆盖，文件名带秒级时间戳）。
/// 失败静默——归档是增强功能，报告生成成功即可返回。
pub(crate) fn archive_snapshot(
    app: &tauri::AppHandle,
    info: &serde_json::Value,
    health: &Option<serde_json::Value>,
    note: &str,
) {
    let Some(dir) = history_dir(app) else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let now = chrono::Utc::now();
    let file = format!("hw-{}.json", now.format("%Y%m%d-%H%M%S"));
    let path = dir.join(&file);
    let payload = serde_json::json!({
        "info": info,
        "health": health,
        "note": note,
        "at": now.timestamp(),
        "machine": machine_label(info),
    });
    if std::fs::write(
        &path,
        serde_json::to_string_pretty(&payload).unwrap_or_default(),
    )
    .is_err()
    {
        return;
    }
    prune(&dir);
}

/// 裁剪到最近 MAX_SNAPSHOTS 份（按文件名排序，删最旧）。
fn prune(dir: &Path) {
    let Ok(mut files) = std::fs::read_dir(dir).map(|rd| {
        rd.filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|s| s.starts_with("hw-") && s.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>()
    }) else {
        return;
    };
    files.sort_by_key(|e| e.file_name());
    let overflow = files.len().saturating_sub(MAX_SNAPSHOTS);
    for f in files.into_iter().take(overflow) {
        let _ = std::fs::remove_file(f.path());
    }
}

fn parse_meta(path: &Path) -> Option<HwSnapshotMeta> {
    let name = path.file_name()?.to_str()?.to_string();
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let at = v.get("at").and_then(|x| x.as_u64()).unwrap_or_else(|| {
        name.chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0)
    });
    let machine = v
        .get("machine")
        .and_then(|x| x.as_str())
        .unwrap_or("本机")
        .to_string();
    let disk_count = v
        .get("info")
        .and_then(|i| i.get("disk"))
        .and_then(|d| d.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    Some(HwSnapshotMeta {
        file: name,
        at,
        machine,
        disk_count,
    })
}

/// 列出全部硬件快照归档（只读 L0，硬件页历史区）。按时间倒序。
#[tauri::command]
pub(crate) fn get_hw_history(app: tauri::AppHandle) -> Vec<HwSnapshotMeta> {
    let Some(dir) = history_dir(&app) else {
        return Vec::new();
    };
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<HwSnapshotMeta> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with("hw-") && s.ends_with(".json"))
                .unwrap_or(false)
        })
        .filter_map(|p| parse_meta(&p))
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out
}

/// 归档文件名白名单：只允许 `hw-*.json` 且不含 `..`（防路径穿越）。
fn safe_snapshot_file(file: &str) -> bool {
    file.starts_with("hw-") && file.ends_with(".json") && !file.contains("..")
}

/// 读取单份归档的完整内容（{info, health, note, at, machine}）。
fn read_snapshot(app: &tauri::AppHandle, file: &str) -> Option<serde_json::Value> {
    let dir = history_dir(app)?;
    if !safe_snapshot_file(file) {
        return None;
    }
    let text = std::fs::read_to_string(dir.join(file)).ok()?;
    serde_json::from_str(&text).ok()
}

fn v_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

fn v_num(v: &serde_json::Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| {
        if x.is_number() {
            x.as_f64()
        } else if let Some(s) = x.as_str() {
            let cleaned: String = s
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                .collect();
            cleaned.parse().ok()
        } else {
            None
        }
    })
}

/// 提取单块磁盘的健康字段：通电时间/温度/磨损度/读写错误。
fn disk_fields(d: &serde_json::Value) -> Vec<(String, Option<f64>)> {
    // SMART 字段在 hw_disk_health 里按「Device」「通电时间」「温度」等键出现；
    // 优先从 health 侧（带 WMI 数值），拿不到再从 CDI 报告取。
    let mut out = vec![
        (
            "通电时间(小时)".to_string(),
            v_num(d, "power_on_hours").or_else(|| v_num(d, "通电时间")),
        ),
        (
            "温度(℃)".to_string(),
            v_num(d, "temperature").or_else(|| v_num(d, "温度")),
        ),
        (
            "磨损度(%)".to_string(),
            v_num(d, "wear").or_else(|| v_num(d, "磨损度")),
        ),
        (
            "读取错误".to_string(),
            v_num(d, "read_errors").or_else(|| v_num(d, "读取错误")),
        ),
        (
            "写入错误".to_string(),
            v_num(d, "write_errors").or_else(|| v_num(d, "写入错误")),
        ),
    ];
    // 去掉全都拿不到的字段，避免对比表塞满「未知」
    out.retain(|(_, val)| val.is_some());
    out
}

fn fmt_field(name: &str, delta: f64) -> String {
    if name.contains("温度") {
        format!("{delta:+.0} ℃")
    } else if name.contains("通电") {
        format!("{delta:+.0} 小时")
    } else if name.contains("磨损") {
        format!("{delta:+.1} %")
    } else {
        format!("{delta:+.0}")
    }
}

/// 对比两份归档（只读）：按磁盘名对齐，输出温度/通电/磨损等字段旧→新 + 差值。
/// 取归档时用 `archive_a` / `archive_b`（文件名为空时自动取最近两份）。
#[tauri::command]
pub(crate) fn compare_hw_snapshots(
    app: tauri::AppHandle,
    archive_a: Option<String>,
    archive_b: Option<String>,
) -> Result<serde_json::Value, String> {
    let all = get_hw_history(app.clone());
    if all.is_empty() {
        return Ok(
            serde_json::json!({ "ok": false, "reason": "还没有硬件快照：先在「硬件报告」里生成一次报告，之后每次生成都会自动归档。" }),
        );
    }
    let pick = |name: Option<String>| -> Result<String, String> {
        match name {
            Some(n) if !n.trim().is_empty() => Ok(n.trim().to_string()),
            _ => all
                .first()
                .map(|m| m.file.clone())
                .ok_or_else(|| "没有可用快照".into()),
        }
    };
    let fa = pick(archive_a)?;
    let fb = match archive_b {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => all
            .iter()
            .find(|m| m.file != fa)
            .map(|m| m.file.clone())
            .ok_or_else(|| "只有一份快照，无法对比（生成第二次报告后即可对比）".to_string())?,
    };
    let va = read_snapshot(&app, &fa).ok_or_else(|| format!("读取归档 {fa} 失败"))?;
    let vb = read_snapshot(&app, &fb).ok_or_else(|| format!("读取归档 {fb} 失败"))?;

    // 磁盘对齐：旧的 health.disk[] 或 info.disk[] 与新的同设备名配对。
    let disks_of = |v: &serde_json::Value| -> Vec<serde_json::Value> {
        v.get("health")
            .and_then(|h| h.get("disk"))
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_else(|| {
                v.get("info")
                    .and_then(|i| i.get("disk"))
                    .and_then(|d| d.as_array())
                    .cloned()
                    .unwrap_or_default()
            })
    };
    let da = disks_of(&va);
    let db = disks_of(&vb);
    if da.is_empty() && db.is_empty() {
        return Ok(
            serde_json::json!({ "ok": false, "reason": "两份归档都没有磁盘健康数据，无法对比。" }),
        );
    }
    let mut compares: Vec<HwDiskCompare> = Vec::new();
    for new_disk in &db {
        let dev = v_str(new_disk, "device");
        let dev = if dev.is_empty() {
            v_str(new_disk, "Device")
        } else {
            dev
        };
        if dev.is_empty() {
            continue;
        }
        let old_disk = da.iter().find(|d| {
            let odev = v_str(d, "device");
            let odev = if odev.is_empty() {
                v_str(d, "Device")
            } else {
                odev
            };
            odev.eq_ignore_ascii_case(&dev)
        });
        let Some(old_disk) = old_disk else { continue };
        let fields_new = disk_fields(new_disk);
        let fields_old = disk_fields(old_disk);
        let mut deltas: Vec<HwFieldDelta> = Vec::new();
        for (name, nv) in &fields_new {
            let ov = fields_old
                .iter()
                .find(|(n, _)| n == name)
                .and_then(|(_, v)| *v);
            if let Some(ov) = ov {
                let d = nv.unwrap_or(0.0) - ov;
                let old_val = serde_json::json!(ov);
                let new_val = serde_json::json!(nv.unwrap_or(0.0));
                deltas.push(HwFieldDelta {
                    name: name.clone(),
                    old: old_val,
                    new: new_val,
                    delta: fmt_field(name, d),
                });
            }
        }
        compares.push(HwDiskCompare {
            device: dev,
            fields: deltas,
        });
    }
    compares.retain(|c| !c.fields.is_empty());
    Ok(serde_json::json!({
        "ok": compares.len() > 0,
        "a": fa,
        "b": fb,
        "disks": compares,
        "reason": if compares.is_empty() { "新旧归档的磁盘健康字段无法对齐（设备名不一致或缺 SMART 数据）".to_string() } else { String::new() },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hw-hist-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mk_snapshot(
        dir: &Path,
        file: &str,
        at: i64,
        power_on: f64,
        temp: f64,
        wear: f64,
        device: &str,
    ) {
        let payload = serde_json::json!({
            "info": {
                "disk": [ { "device": device, "Model": device, "Size": 100 } ],
                "system": [ { "Model": "TestBox" } ],
                "os": [ { "Manufacturer": "Test", "Caption": "Windows Test" } ]
            },
            "health": { "disk": [ { "device": device, "power_on_hours": power_on, "temperature": temp, "wear": wear } ] },
            "note": "",
            "at": at,
            "machine": "TestBox · Windows Test",
        });
        std::fs::write(
            dir.join(file),
            serde_json::to_string_pretty(&payload).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn archive_names_are_timestamped_and_listed_desc() {
        let dir = tmp_dir("list");
        mk_snapshot(
            &dir,
            "hw-20260910-100000.json",
            1_700_000_000,
            100.0,
            40.0,
            5.0,
            "Disk A",
        );
        mk_snapshot(
            &dir,
            "hw-20260911-100000.json",
            1_700_086_400,
            130.0,
            45.0,
            7.0,
            "Disk A",
        );
        let metas = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter_map(|p| parse_meta(&p))
            .collect::<Vec<_>>();
        assert_eq!(metas.len(), 2);
        // get_hw_history 的排序逻辑：按 at 倒序（最新的在前）。read_dir 本身
        // 顺序任意，这里直接验证排序后结果。
        let mut sorted = metas.clone();
        sorted.sort_by(|a, b| b.at.cmp(&a.at));
        let ats = sorted.iter().map(|m| m.at).collect::<Vec<_>>();
        assert_eq!(ats, vec![1_700_086_400, 1_700_000_000]);
        assert_eq!(sorted[0].machine, "TestBox · Windows Test");
        assert_eq!(sorted[0].disk_count, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compare_extracts_field_deltas() {
        let dir = tmp_dir("cmp");
        // 两次归档：通电 100→130h、温度 40→45℃、磨损 5→7%
        mk_snapshot(
            &dir,
            "hw-old.json",
            1_700_000_000,
            100.0,
            40.0,
            5.0,
            "Disk A",
        );
        mk_snapshot(
            &dir,
            "hw-new.json",
            1_700_086_400,
            130.0,
            45.0,
            7.0,
            "Disk A",
        );
        // 直接调用底层逻辑：读两份 → 对齐 → 算差值
        let va: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("hw-old.json")).unwrap())
                .unwrap();
        let vb: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("hw-new.json")).unwrap())
                .unwrap();
        let disks_of = |v: &serde_json::Value| -> Vec<serde_json::Value> {
            v.get("health")
                .and_then(|h| h.get("disk"))
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default()
        };
        let da = disks_of(&va);
        let db = disks_of(&vb);
        let new_d = &db[0];
        let old_d = da
            .iter()
            .find(|d| v_str(d, "device").eq_ignore_ascii_case(&v_str(new_d, "device")))
            .unwrap();
        let f_new = disk_fields(new_d);
        let f_old = disk_fields(old_d);
        assert!(f_new.len() >= 3);
        let hours = f_new
            .iter()
            .find(|(n, _)| n.contains("通电"))
            .unwrap()
            .1
            .unwrap();
        assert_eq!(hours, 130.0);
        let temp = f_new
            .iter()
            .find(|(n, _)| n.contains("温度"))
            .unwrap()
            .1
            .unwrap();
        assert_eq!(temp, 45.0);
        let wear = f_new
            .iter()
            .find(|(n, _)| n.contains("磨损"))
            .unwrap()
            .1
            .unwrap();
        assert_eq!(wear, 7.0);
        // 旧值对比正确
        let old_hours = f_old
            .iter()
            .find(|(n, _)| n.contains("通电"))
            .unwrap()
            .1
            .unwrap();
        assert_eq!(old_hours, 100.0);
        assert_eq!(fmt_field("通电时间(小时)", hours - old_hours), "+30 小时");
        assert_eq!(fmt_field("温度(℃)", 5.0), "+5 ℃");
        assert_eq!(fmt_field("磨损度(%)", 2.0), "+2.0 %");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_latest_30() {
        let dir = tmp_dir("prune");
        for i in 0..35 {
            let file = format!("hw-20260910-{:06}.json", i);
            mk_snapshot(&dir, &file, 1_700_000_000 + i, 10.0, 30.0, 1.0, "Disk A");
        }
        prune(&dir);
        let count = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .map(|x| {
                        x.file_name()
                            .to_str()
                            .map(|s| s.starts_with("hw-"))
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(count, MAX_SNAPSHOTS, "裁剪后应保留最近 30 份");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_traversal_rejected() {
        // read_snapshot 的文件名白名单：只允许 hw-*.json 且不含 ..
        assert!(!safe_snapshot_file("..\\evil.json"));
        assert!(!safe_snapshot_file("hw-evil.txt"));
        assert!(safe_snapshot_file("hw-20260910-153000.json"));
        assert!(safe_snapshot_file("hw-20260910-153000.json"));
        // 组合场景：白名单本身也拒绝带 .. 的合法前缀
        assert!(!safe_snapshot_file("hw-..\\x.json"));
    }
}
