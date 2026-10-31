//! 观测总览（R485）：MCP 侧复刻桌面端 observations_overview
//! （apps/desktop/src-tauri/src/observations.rs）的索引逻辑——只读统一数据根下
//! 各观测域，汇成 { v, data_root, domains }。纯只读：不采集、不写任何文件。
//!
//! 族规与桌面端一致（R88/R479）：缺文件 = 真空态（entries=0），读取失败 = error，
//! 部分行损坏 = 跳过并计 corrupt_lines，全部行损坏 = error（诚实性）。

use std::path::Path;

use serde_json::{json, Map, Value};

use super::rep_storage::data_dir;

/// jsonl 域：域名 → 数据根下的文件名（与桌面端 observations.rs 的表一致）。
const JSONL_DOMAINS: &[(&str, &str)] = &[
    ("scan_stamps", "scan-stamps.jsonl"),
    ("scan_summary", "scan-summary.jsonl"),
    ("space_history", "space-history.jsonl"),
    ("startup_history", "startup-history.jsonl"),
    ("suggestion_history", "suggestion-history.jsonl"),
    ("thermal_history", "thermal-history.jsonl"),
    ("undo", "undo.jsonl"),
    // agent-server 单根统一（R482）后也写进同一数据根：
    ("sensor_history", "sensor-history.jsonl"),
    ("sensor_alerts", "sensor-alerts.jsonl"),
];

/// state 单文档域：域名 → 数据根下的文件名。
const STATE_DOMAINS: &[(&str, &str)] = &[
    ("usn_cursors", "usn-cursors.json"),
    ("general", "general.json"),
    ("reminder", "reminder.json"),
    ("auto_list", "auto-list.json"),
    ("mcp_servers", "mcp-servers.json"),
    ("scaffolds_registry", "scaffolds-registry.json"),
    ("tool_plugin", "tool.plugin.json"),
];

/// hw 快照归档目录名（数据根下）。
const HW_HISTORY_DIR: &str = "hw-history";

/// 读一个 jsonl 域：返回 (有效行数, 损坏行数, 最后一条)。
/// 缺文件 = 真空态 Ok((0,0,None))；读取失败/全部行损坏 = Err。
fn read_jsonl_indexed(root: &Path, file: &str) -> Result<(usize, usize, Option<Value>), String> {
    let p = root.join(file);
    if !p.exists() {
        return Ok((0, 0, None));
    }
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{file} 读取失败: {e}"))?;
    let mut entries = 0usize;
    let mut corrupt = 0usize;
    let mut latest = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(v) => {
                entries += 1;
                latest = Some(v);
            }
            Err(_) => corrupt += 1,
        }
    }
    if entries == 0 && corrupt > 0 {
        return Err(format!("{file} 全部行损坏（{corrupt} 条），疑似写入中断"));
    }
    Ok((entries, corrupt, latest))
}

/// 汇总一个数据根下的全部观测域。纯函数，便于测试。
fn build_overview(root: &Path) -> Result<Value, String> {
    let mut domains = Map::new();

    for &(name, file) in JSONL_DOMAINS {
        let entry = match read_jsonl_indexed(root, file) {
            Ok((entries, corrupt, latest)) => {
                json!({"kind":"jsonl","file":file,"entries":entries,"corrupt_lines":corrupt,"latest":latest})
            }
            Err(e) => json!({"kind":"jsonl","file":file,"error":e}),
        };
        domains.insert(name.to_string(), entry);
    }

    for &(name, file) in STATE_DOMAINS {
        let p = root.join(file);
        let entry = if !p.exists() {
            json!({"kind":"state","file":file,"present":false})
        } else {
            match std::fs::read_to_string(&p) {
                Ok(t) => match serde_json::from_str::<Value>(&t) {
                    Ok(_) => json!({"kind":"state","file":file,"present":true}),
                    Err(e) => json!({"kind":"state","file":file,"error":format!("解析失败: {e}")}),
                },
                Err(e) => json!({"kind":"state","file":file,"error":format!("读取失败: {e}")}),
            }
        };
        domains.insert(name.to_string(), entry);
    }

    // hw-history/ 目录域：只计 .json，latest 取文件名字典序最大（文件名带时间戳 = 最新）
    let hw_dir = root.join(HW_HISTORY_DIR);
    let mut hw_count = 0usize;
    let mut hw_latest: Option<String> = None;
    if hw_dir.is_dir() {
        let mut names: Vec<String> = std::fs::read_dir(&hw_dir)
            .map_err(|e| format!("{HW_HISTORY_DIR}/ 读取失败: {e}"))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .collect();
        names.sort();
        hw_count = names.len();
        hw_latest = names.last().cloned();
    }
    domains.insert(
        "hw_history".to_string(),
        json!({"kind":"dir","dir":HW_HISTORY_DIR,"entries":hw_count,"latest":hw_latest}),
    );

    Ok(json!({
        "v": 1,
        "data_root": root.to_string_lossy().as_ref(),
        "domains": Value::Object(domains),
    }))
}

/// MCP 工具入口：解析统一数据根后汇总。数据根未解析 = 诚实报错，不猜路径。
pub fn observations_overview() -> Result<String, String> {
    let root = data_dir().ok_or_else(|| {
        "数据根未解析（DISKPILOT_DATA_DIR 未设置且无 LOCALAPPDATA/TEMP）".to_string()
    })?;
    let v = build_overview(&root)?;
    serde_json::to_string_pretty(&v).map_err(|e| format!("序列化失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("diskpilot-rep-obs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn overview_indexes_all_domain_kinds() {
        let d = tmp_dir("mixed");
        // jsonl 域：2 行有效 + 1 行损坏
        std::fs::write(
            d.join("scan-stamps.jsonl"),
            "{\"at\":1}\n{\"at\":2}\nnot json\n",
        )
        .unwrap();
        // state 域
        std::fs::write(d.join("general.json"), "{\"v\":1}").unwrap();
        // hw-history 目录
        let hw = d.join(HW_HISTORY_DIR);
        std::fs::create_dir_all(&hw).unwrap();
        std::fs::write(hw.join("hw-20260101-000000.json"), "{}").unwrap();
        std::fs::write(hw.join("hw-20260201-000000.json"), "{}").unwrap();
        std::fs::write(hw.join("not-json.txt"), "x").unwrap();

        let v = build_overview(&d).unwrap();
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
        // dir 域：只计 .json，latest 取字典序最大
        assert_eq!(dom["hw_history"]["kind"], "dir");
        assert_eq!(dom["hw_history"]["entries"], 2);
        assert_eq!(dom["hw_history"]["latest"], "hw-20260201-000000.json");
        // 全部域都进表
        assert_eq!(
            dom.as_object().unwrap().len(),
            JSONL_DOMAINS.len() + STATE_DOMAINS.len() + 1
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn all_corrupt_jsonl_is_error_not_vacuum() {
        let d = tmp_dir("corrupt");
        std::fs::write(d.join("undo.jsonl"), "not json\nnot json either\n").unwrap();
        let v = build_overview(&d).unwrap();
        assert!(v["domains"]["undo"]["error"]
            .as_str()
            .unwrap()
            .contains("全部行损坏"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
