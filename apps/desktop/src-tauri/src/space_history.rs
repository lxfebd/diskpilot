//! 磁盘空间历史（R6）：每次扫描把 `{root, total_bytes, used_bytes, at}` 追加到
//! `app_data_dir/space-history.jsonl`（增量小文件），总览页据此画多日曲线 +
//! 「本周新增 N GB 于 X」定位。**纯只读展示**：不删文件、不清理，数据只增不改。
//!
//! 设计约束：
//! - 追加式 jsonl，避免整文件重写（扫描次数有限，文件天然小）；
//! - 缺文件 / 解析坏行一律静默跳过（历史记录是锦上添花，不能因它失败）；
//! - 同 root 短时间（<60s）重复记录去重，避免一次扫描事件写两行；
//! - 每 root 最多保留最近 180 条，超长裁剪，防止跨年无限增长。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tauri::Manager;

/// 单条空间快照。`at` 为 Unix 秒；`total_bytes` / `used_bytes` 为扫描时
/// `GetDiskFreeSpaceExW` 读到的卷容量（used = total - free）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SpacePoint {
    pub root: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub at: u64,
}

/// 距上次记录不足 60 秒视为同一扫描事件（一次扫描只落一行）。
const DEDUP_SECS: u64 = 60;
/// 每 root 最多保留的记录数；超出丢弃最旧。
const MAX_PER_ROOT: usize = 180;

fn history_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("space-history.jsonl"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 追加一条空间快照。**静默失败**：目录不可写 / 文件锁冲突时忽略
/// （趋势是增强功能，绝不能阻塞扫描主流程）。同 root 60s 内重复丢弃。
pub(crate) fn record_space_point(
    app: &tauri::AppHandle,
    root: &str,
    total_bytes: u64,
    used_bytes: u64,
) {
    let Some(p) = history_path(app) else {
        return;
    };
    if let Some(parent) = p.parent() {
        if !parent.exists() && std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let at = now_secs();
    // 追加前看一眼最后一行同 root 的时间戳，去重（读 1 行成本可忽略）。
    if last_same_root_within(&p, root, at, DEDUP_SECS) {
        return;
    }
    let line = match serde_json::to_string(&SpacePoint {
        root: root.to_string(),
        total_bytes,
        used_bytes,
        at,
    }) {
        Ok(l) => l,
        Err(_) => return,
    };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{line}");
    }
}

/// 检查文件最后一条同 root 记录是否在 `now - within_secs` 内。
fn last_same_root_within(p: &Path, root: &str, now: u64, within_secs: u64) -> bool {
    let Ok(f) = std::fs::File::open(p) else {
        return false;
    };
    let reader = BufReader::new(f);
    // 只倒查最后 ~64 行：jsonl 单行极短，倒查成本可忽略且避免全文件读。
    let mut lines: Vec<String> = Vec::new();
    for line in reader.lines().flatten() {
        lines.push(line);
        if lines.len() > 64 {
            lines.remove(0);
        }
    }
    for line in lines.into_iter().rev() {
        let Ok(pt) = serde_json::from_str::<SpacePoint>(&line) else {
            continue;
        };
        if pt.root == root {
            return now.saturating_sub(pt.at) < within_secs;
        }
    }
    false
}

/// 读取全部历史（按写入顺序）。缺文件返回空 vec；坏行静默跳过。
/// 跨 root 的记录交错保存，按 root 聚合成 map 后返回。
pub(crate) fn read_history(app: &tauri::AppHandle) -> HashMap<String, Vec<SpacePoint>> {
    let Some(p) = history_path(app) else {
        return HashMap::new();
    };
    read_history_at(&p)
}

/// 供单测使用的纯函数入口（不依赖 AppHandle）。
fn read_history_at(p: &Path) -> HashMap<String, Vec<SpacePoint>> {
    let Ok(f) = std::fs::File::open(p) else {
        return HashMap::new();
    };
    let mut out: HashMap<String, Vec<SpacePoint>> = HashMap::new();
    for line in BufReader::new(f).lines().flatten() {
        if let Ok(pt) = serde_json::from_str::<SpacePoint>(&line) {
            out.entry(pt.root.clone()).or_default().push(pt);
        }
    }
    for v in out.values_mut() {
        if v.len() > MAX_PER_ROOT {
            v.drain(..v.len() - MAX_PER_ROOT);
        }
    }
    out
}

/// 读回并裁剪超长历史（写文件）。由 `get_space_history` 在返回前调用，
/// 保证 180 条上限真实落盘而不是只在内存裁剪。
pub(crate) fn prune_history(app: &tauri::AppHandle) {
    let Some(p) = history_path(app) else {
        return;
    };
    let map = read_history_at(&p);
    if map.values().all(|v| v.len() <= MAX_PER_ROOT) {
        return;
    }
    let mut lines: Vec<(u64, String)> = Vec::new();
    for v in map.values() {
        for pt in v {
            if let Ok(l) = serde_json::to_string(pt) {
                lines.push((pt.at, l));
            }
        }
    }
    lines.sort_by_key(|(at, _)| *at);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&p)
    {
        for (_, l) in lines {
            let _ = writeln!(f, "{l}");
        }
    }
}

/// 扫描完成后记录卷容量快照。`key` 是规范化扫描根（如 `C:\` / `D:\dir`）。
/// 通过 `crate::volume_info` 读卷容量（复用 list_drives 同款 Win32 查询）；
/// 读取失败静默（历史是增强功能，不能因它失败）。
pub(crate) fn record_after_scan(app: &tauri::AppHandle, key: &str) {
    let Ok(info) = crate::volume_info(key.to_string()) else {
        return;
    };
    record_space_point(app, key, info.total_bytes, info.used_bytes);
}

/// 读取全部磁盘空间历史（只读 L0，总览页画趋势曲线）。
/// 返回 `{root -> [SpacePoint]}`，按写入顺序排列；缺文件返回空对象。
#[tauri::command]
pub(crate) fn get_space_history(app: tauri::AppHandle) -> HashMap<String, Vec<SpacePoint>> {
    let map = read_history(&app);
    prune_history(&app);
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_returns_empty() {
        let dir = std::env::temp_dir().join(format!("space-hist-none-{}", std::process::id()));
        let p = dir.join("space-history.jsonl");
        let map = read_history_at(&p);
        assert!(map.is_empty(), "缺文件必须静默返回空 map");
    }

    #[test]
    fn append_then_read_roundtrip() {
        let dir = std::env::temp_dir().join(format!("space-hist-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("space-history.jsonl");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .unwrap();
        for (i, (total, used)) in [(100, 40), (100, 55), (100, 70)].iter().enumerate() {
            let pt = SpacePoint {
                root: "C:\\".into(),
                total_bytes: *total,
                used_bytes: *used,
                at: 1_700_000_000 + i as u64 * 3600,
            };
            writeln!(f, "{}", serde_json::to_string(&pt).unwrap()).unwrap();
        }
        drop(f);
        let map = read_history_at(&p);
        let pts = map.get("C:\\").expect("应有 C:\\ 历史");
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0].used_bytes, 40);
        assert_eq!(pts[2].used_bytes, 70);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_lines_are_skipped_silently() {
        let dir = std::env::temp_dir().join(format!("space-hist-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("space-history.jsonl");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .unwrap();
        writeln!(f, "not json at all").unwrap();
        let pt = SpacePoint {
            root: "D:\\".into(),
            total_bytes: 200,
            used_bytes: 90,
            at: 1_700_000_000,
        };
        writeln!(f, "{}", serde_json::to_string(&pt).unwrap()).unwrap();
        drop(f);
        let map = read_history_at(&p);
        assert_eq!(map.len(), 1, "坏行静默跳过，只留合法行");
        assert_eq!(map.get("D:\\").unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dedup_within_60s() {
        let dir = std::env::temp_dir().join(format!("space-hist-dedup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("space-history.jsonl");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .unwrap();
        for at in [1_700_000_000u64, 1_700_000_030, 1_700_000_500] {
            let pt = SpacePoint {
                root: "C:\\".into(),
                total_bytes: 100,
                used_bytes: 50,
                at,
            };
            writeln!(f, "{}", serde_json::to_string(&pt).unwrap()).unwrap();
        }
        drop(f);
        // 60s 内重复（+30s）→ true；超过 60s（+500s）→ false
        assert!(last_same_root_within(&p, "C:\\", 1_700_000_045, 60));
        assert!(!last_same_root_within(&p, "C:\\", 1_700_000_500 + 61, 60));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_caps_per_root() {
        let dir = std::env::temp_dir().join(format!("space-hist-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("space-history.jsonl");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .unwrap();
        for i in 0..200 {
            let pt = SpacePoint {
                root: "C:\\".into(),
                total_bytes: 100,
                used_bytes: i,
                at: 1_700_000_000 + i,
            };
            writeln!(f, "{}", serde_json::to_string(&pt).unwrap()).unwrap();
        }
        drop(f);
        let map = read_history_at(&p);
        let pts = map.get("C:\\").unwrap();
        assert_eq!(pts.len(), MAX_PER_ROOT, "超长必须裁剪到 180");
        assert_eq!(pts[0].used_bytes, 20, "裁剪后应保留最新 180 条");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn get_space_history_missing_file_is_empty_map() {
        // 缺文件时命令级入口必须返回空 map（不 panic、不 Err）——前端首次打开
        // 总览页（从未扫描过）直接命中此路径。
        let dir = std::env::temp_dir().join(format!("space-hist-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let map = read_history_at(&dir.join("space-history.jsonl"));
        assert!(map.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
