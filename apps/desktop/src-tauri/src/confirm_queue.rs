//! R530 待人工确认清单（confirm-queue.jsonl）：诊断插件（如 reg-hunter --json）
//! 发现的失效残留先**入队待人工确认**，绝不自动删除——删除仍只走清理确认窗
//! （user_confirmed 硬校验）独占。入队/出队是 L1 配置类写（用户自己的数据目录），
//! 不进 nonce 闸；真正删键/删文件永远不从这里走。
//!
//! - 文件：app_data_dir()/confirm-queue.jsonl，每行一条 {id, at, source, kind, key, name, detail}。
//! - 上限 500 条（超出删最旧），与 undo 台账同款裁剪思路。
//! - 读失败报错（诚实族规），目录不存在 = 空清单。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const MAX_QUEUE: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct QueueItem {
    pub id: String,
    /// 入队时间（unix 秒）
    pub at: u64,
    /// 来源插件 id（如 reg-hunter）
    pub source: String,
    /// uninstall | run | 其他
    pub kind: String,
    /// 注册表键 / 路径定位串
    pub key: String,
    /// 显示名
    pub name: String,
    /// 原始 detail（JSON 字符串，原样保留）
    pub detail: String,
}

fn queue_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("confirm-queue.jsonl"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn queue_list(app: &AppHandle) -> Result<Vec<QueueItem>, String> {
    let Some(p) = queue_path(app) else {
        return Err("无法定位数据目录".into());
    };
    let Ok(text) = std::fs::read_to_string(&p) else {
        // 文件不存在 = 空清单（诚实：从未入队过）
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        // 损坏行跳过但不整体失败（与 undo.jsonl 同款口径）
        if let Ok(it) = serde_json::from_str::<QueueItem>(l) {
            out.push(it);
        }
    }
    Ok(out)
}

/// 追加一条。返回新条目 id。
pub(crate) fn queue_push(app: &AppHandle, item: QueueItem) -> Result<String, String> {
    let Some(p) = queue_path(app) else {
        return Err("无法定位数据目录".into());
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut list = queue_list(app)?;
    // 同源同 key 去重：重复入队只刷新时间与 detail
    list.retain(|x| !(x.source == item.source && x.key == item.key));
    let id = item.id.clone();
    list.push(item);
    while list.len() > MAX_QUEUE {
        list.remove(0);
    }
    let mut text = String::new();
    for it in &list {
        text.push_str(&serde_json::to_string(it).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    std::fs::write(&p, text).map_err(|e| e.to_string())?;
    Ok(id)
}

pub(crate) fn queue_remove(app: &AppHandle, id: &str) -> Result<usize, String> {
    let Some(p) = queue_path(app) else {
        return Err("无法定位数据目录".into());
    };
    let mut list = queue_list(app)?;
    let before = list.len();
    list.retain(|x| x.id != id);
    let removed = before - list.len();
    let mut text = String::new();
    for it in &list {
        text.push_str(&serde_json::to_string(it).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    std::fs::write(&p, text).map_err(|e| e.to_string())?;
    Ok(removed)
}

/// R530 入队命令：前端把 reg-hunter --json 解析出的失效项逐条调这个（L1）。
#[tauri::command]
pub(crate) fn confirm_queue_add(
    app: AppHandle,
    source: String,
    kind: String,
    key: String,
    name: String,
    detail: String,
) -> Result<String, String> {
    let item = QueueItem {
        id: format!("q{}-{}", now_secs(), key.len()),
        at: now_secs(),
        source,
        kind,
        key,
        name,
        detail,
    };
    queue_push(&app, item)
}

#[tauri::command]
pub(crate) fn confirm_queue_list_cmd(app: AppHandle) -> Result<Vec<QueueItem>, String> {
    queue_list(&app)
}

#[tauri::command]
pub(crate) fn confirm_queue_remove_cmd(app: AppHandle, id: String) -> Result<usize, String> {
    queue_remove(&app, &id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_item_roundtrip() {
        let it = QueueItem {
            id: "q1".into(),
            at: 123,
            source: "reg-hunter".into(),
            kind: "uninstall".into(),
            key: "HKLM\\...\\Uninstall\\X".into(),
            name: "某软件".into(),
            detail: "{\"reasons\":[]}".into(),
        };
        let s = serde_json::to_string(&it).unwrap();
        let back: QueueItem = serde_json::from_str(&s).unwrap();
        assert_eq!(back.key, it.key);
        assert_eq!(back.name, "某软件");
    }
}
