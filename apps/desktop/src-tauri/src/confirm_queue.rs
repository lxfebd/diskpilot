//! R530 待人工确认清单（confirm-queue.jsonl）：诊断插件（如 reg-hunter --json）
//! 发现的失效残留先**入队待人工确认**，绝不自动删除——删除仍只走清理确认窗
//! （user_confirmed 硬校验）独占。入队/出队是 L1 配置类写（用户自己的数据目录），
//! 不进 nonce 闸；真正删键/删文件永远不从这里走。
//!
//! - 文件：app_data_dir()/confirm-queue.jsonl，每行一条 {id, at, source, kind, key, name, detail}。
//! - 上限 500 条（超出删最旧），与 undo 台账同款裁剪思路。
//! - 读失败报错（诚实族规），仅「文件不存在」= 空清单；其余 IO 错误必须上报，
//!   绝不能当空列表重写（R549 审查 H1：否则一次瞬时读错误会清空整个队列）。
//! - 写侧走 tmp 文件 + rename 原子替换（R549），全程持模块级互斥锁防并发命令丢条目。
//! - 字段上限各 64KB（R549 审查 L2），id 追加随机后缀防同秒等长 key 碰撞（M2）。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

const MAX_QUEUE: usize = 500;
/// R549：单字段上限，防止 L1 免 nonce 命令塞超大字段撑爆文件
const MAX_FIELD: usize = 64 * 1024;
/// R549：队列读改写互斥锁（Tauri 命令在 async runtime 并发执行）
static QUEUE_LOCK: Mutex<()> = Mutex::new(());

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
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        // R549：仅 NotFound 视为空清单；权限/IO/非法 UTF-8 一律报错，绝不当空重写
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("读取确认队列失败: {e}")),
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

/// R549：字段截断到 MAX_FIELD，防止超大字段撑爆 jsonl。
fn clip(s: &str) -> String {
    if s.len() > MAX_FIELD {
        s.chars().take(MAX_FIELD).collect()
    } else {
        s.to_string()
    }
}

/// R549：tmp + rename 原子写，崩溃 mid-write 不会留下半截文件。
fn write_jsonl_atomic(p: &std::path::Path, list: &[QueueItem]) -> Result<(), String> {
    let mut text = String::new();
    for it in list {
        text.push_str(&serde_json::to_string(it).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    let tmp = p.with_extension("jsonl.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())?;
    Ok(())
}

/// 追加一条。返回新条目 id。
pub(crate) fn queue_push(app: &AppHandle, mut item: QueueItem) -> Result<String, String> {
    // R549：互斥锁 + 字段截断；id 追加随机后缀防同秒碰撞
    let _guard = QUEUE_LOCK.lock().map_err(|_| "队列锁中毒".to_string())?;
    let Some(p) = queue_path(app) else {
        return Err("无法定位数据目录".into());
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    item.source = clip(&item.source);
    item.kind = clip(&item.kind);
    item.key = clip(&item.key);
    item.name = clip(&item.name);
    item.detail = clip(&item.detail);
    item.id = format!("{}-{}", item.id, rand_suffix());
    let mut list = queue_list(app)?;
    // 同源同 key 去重：重复入队只刷新时间与 detail
    list.retain(|x| !(x.source == item.source && x.key == item.key));
    let id = item.id.clone();
    list.push(item);
    while list.len() > MAX_QUEUE {
        list.remove(0);
    }
    write_jsonl_atomic(&p, &list)?;
    Ok(id)
}

pub(crate) fn queue_remove(app: &AppHandle, id: &str) -> Result<usize, String> {
    let _guard = QUEUE_LOCK.lock().map_err(|_| "队列锁中毒".to_string())?;
    let Some(p) = queue_path(app) else {
        return Err("无法定位数据目录".into());
    };
    let list = queue_list(app)?;
    let before = list.len();
    let kept: Vec<_> = list.into_iter().filter(|x| x.id != id).collect();
    let removed = before - kept.len();
    write_jsonl_atomic(&p, &kept)?;
    Ok(removed)
}

/// R549：随机后缀（零依赖，xorshift 种子取系统时间 + 进程 id）。
fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = t ^ (std::process::id() as u64).wrapping_mul(0x9E3779B97F4A7C15);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51AFD7ED558CCD);
    x ^= x >> 33;
    format!("{:08x}", x)
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

    #[test]
    fn r549_atomic_write_roundtrip_and_clip() {
        let dir = std::env::temp_dir().join(format!("cq-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("confirm-queue.jsonl");
        let items = vec![QueueItem {
            id: "q1-x".into(),
            at: 1,
            source: "reg-hunter".into(),
            kind: "run".into(),
            key: "K".into(),
            name: "N".into(),
            detail: "{}".into(),
        }];
        write_jsonl_atomic(&p, &items).unwrap();
        // 原子写后不存在 tmp 残留
        assert!(!p.with_extension("jsonl.tmp").exists());
        let text = std::fs::read_to_string(&p).unwrap();
        let back: QueueItem = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(back.id, "q1-x");
        // clip：超长字段截断到 MAX_FIELD 字符
        let big = "x".repeat(MAX_FIELD * 2);
        assert_eq!(clip(&big).chars().count(), MAX_FIELD);
        // rand_suffix 为 8~16 位十六进制（u64 最小宽度 8）
        let s = rand_suffix();
        assert!(!s.is_empty() && s.len() <= 16 && s.chars().all(|c| c.is_ascii_hexdigit()));
        std::fs::remove_dir_all(&dir).ok();
    }
}
