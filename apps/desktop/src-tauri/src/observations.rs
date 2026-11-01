//! observations —— 观测总览门面（R482）。
//!
//! P0-3 的最后一块：给"这台机器被观测成什么样"一个单一入口。
//! 不重新实现任何存储逻辑，只是用 obs 内核（R479）把 app_data_dir 下
//! 各域文件读一遍，汇成索引：{ v, data_root, domains: { 域名: 概要 } }。
//!
//! - jsonl 域：entries（有效行数）/ corrupt_lines（损坏行数）/ latest（最后一条）
//! - state 域：present（文件是否存在且可解析）；损坏 = error 字段（R88 诚实性）
//! - hw-history/ 目录域：entries（快照数）/ latest（最新文件名，文件名带时间戳）
//!
//! 域名 → 文件名的映射集中在下面两个表，新增域只改表不改逻辑。
//! agent-server 的观测文件在 R482 单根统一后也落在同一 data_root，
//! 这里一并纳入（文件不存在时 entries=0，属正常真空态）。

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};

use crate::obs;

/// jsonl 域：域名 → app_data_dir 下的文件名。
const JSONL_DOMAINS: &[(&str, &str)] = &[
    ("scan_stamps", "scan-stamps.jsonl"),
    ("scan_summary", "scan-summary.jsonl"),
    ("space_history", "space-history.jsonl"),
    ("startup_history", "startup-history.jsonl"),
    ("suggestion_history", "suggestion-history.jsonl"),
    ("thermal_history", "thermal-history.jsonl"),
    ("undo", "undo.jsonl"),
    // agent-server 单根统一（R482）后也写进同一 data_root：
    ("sensor_history", "sensor-history.jsonl"),
    ("sensor_alerts", "sensor-alerts.jsonl"),
];

/// state 单文档域：域名 → app_data_dir 下的文件名。
const STATE_DOMAINS: &[(&str, &str)] = &[
    ("usn_cursors", "usn-cursors.json"),
    ("general", "general.json"),
    ("reminder", "reminder.json"),
    ("auto_list", "auto-list.json"),
    ("mcp_servers", "mcp-servers.json"),
    ("scaffolds_registry", "scaffolds-registry.json"),
    ("tool_plugin", "tool.plugin.json"),
];

/// hw 快照归档目录名（app_data_dir 下）。
const HW_HISTORY_DIR: &str = "hw-history";

/// 汇总一个数据根下的全部观测域。纯函数，便于测试。
/// R486 起 pub(crate)：周报（weekly_report.rs）聚合观测异常时复用。
pub(crate) fn build_overview(root: &Path) -> Value {
    let mut domains = Map::new();

    for &(name, file) in JSONL_DOMAINS {
        let p = root.join(file);
        let entry = match obs::read_jsonl::<Value>(&p) {
            Ok(out) => json!({
                "kind": "jsonl",
                "file": file,
                "entries": out.items.len(),
                "corrupt_lines": out.corrupt_lines,
                "latest": out.items.into_iter().last(),
            }),
            Err(e) => json!({ "kind": "jsonl", "file": file, "error": e }),
        };
        domains.insert(name.to_string(), entry);
    }

    for &(name, file) in STATE_DOMAINS {
        let p = root.join(file);
        let entry = match obs::read_state::<Value>(&p) {
            Ok(Some(_)) => json!({ "kind": "state", "file": file, "present": true }),
            Ok(None) => json!({ "kind": "state", "file": file, "present": false }),
            Err(e) => json!({ "kind": "state", "file": file, "error": e }),
        };
        domains.insert(name.to_string(), entry);
    }

    let hw_dir = root.join(HW_HISTORY_DIR);
    let hw_entry = match std::fs::read_dir(&hw_dir) {
        Ok(rd) => {
            let mut names: Vec<String> = rd
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x == "json")
                        .unwrap_or(false)
                })
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            json!({
                "kind": "dir",
                "file": format!("{HW_HISTORY_DIR}/"),
                "entries": names.len(),
                "latest": names.last().cloned(),
            })
        }
        Err(_) => json!({
            "kind": "dir",
            "file": format!("{HW_HISTORY_DIR}/"),
            "entries": 0,
            "latest": Value::Null,
        }),
    };
    domains.insert("hw_history".to_string(), hw_entry);

    json!({
        "v": 1,
        "data_root": root.to_string_lossy(),
        "domains": Value::Object(domains),
    })
}

/// 观测总览命令：返回 app_data_dir 下各观测域的索引（只读，不写任何文件）。
#[tauri::command]
pub(crate) fn observations_overview(app: AppHandle) -> Result<Value, String> {
    let root: PathBuf = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir 失败: {e}"))?;
    Ok(build_overview(&root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json as j;

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dp-observations-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn overview_indexes_all_domain_kinds() {
        let d = tmp_dir("full");
        // jsonl 域：2 条有效 + 1 条撕裂
        assert!(obs::write_jsonl(
            &d.join("scan-stamps.jsonl"),
            &[
                j!({"v":1,"root":"c:/","at":1}),
                j!({"v":1,"root":"c:/","at":2})
            ]
        ));
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(d.join("scan-stamps.jsonl"))
                .unwrap();
            let _ = writeln!(f, "{{\"torn");
        }
        // state 域
        assert!(obs::write_state(&d.join("general.json"), &j!({"v":1})));
        // hw-history 目录
        let hw = d.join(HW_HISTORY_DIR);
        std::fs::create_dir_all(&hw).unwrap();
        std::fs::write(hw.join("hw-20260101-000000.json"), "{}").unwrap();
        std::fs::write(hw.join("hw-20260201-000000.json"), "{}").unwrap();
        std::fs::write(hw.join("not-json.txt"), "x").unwrap();

        let v = build_overview(&d);
        assert_eq!(v["v"], 1);
        assert_eq!(
            v["data_root"].as_str().unwrap(),
            d.to_string_lossy().as_ref()
        );
        let dom = &v["domains"];
        assert_eq!(dom["scan_stamps"]["kind"], "jsonl");
        assert_eq!(dom["scan_stamps"]["entries"], 2);
        assert_eq!(dom["scan_stamps"]["corrupt_lines"], 1);
        assert_eq!(dom["scan_stamps"]["latest"]["at"], 2);
        // 缺文件 = 真空态而非错误
        assert_eq!(dom["undo"]["entries"], 0);
        assert_eq!(dom["undo"]["corrupt_lines"], 0);
        assert!(dom["undo"].get("error").is_none());
        // state 域
        assert_eq!(dom["general"]["present"], true);
        assert_eq!(dom["reminder"]["present"], false);
        // dir 域：只计 .json，latest 取字典序最大（即最新）
        assert_eq!(dom["hw_history"]["kind"], "dir");
        assert_eq!(dom["hw_history"]["entries"], 2);
        assert_eq!(dom["hw_history"]["latest"], "hw-20260201-000000.json");
        // 全部域都进表
        assert_eq!(
            dom.as_object().unwrap().len(),
            JSONL_DOMAINS.len() + STATE_DOMAINS.len() + 1
        );
    }

    #[test]
    fn all_corrupt_jsonl_is_error_not_vacuum() {
        let d = tmp_dir("corrupt");
        std::fs::write(d.join("undo.jsonl"), "not json\nnot json either\n").unwrap();
        let v = build_overview(&d);
        let undo = &v["domains"]["undo"];
        assert!(undo.get("error").is_some());
        assert!(undo.get("entries").is_none());
    }
}
