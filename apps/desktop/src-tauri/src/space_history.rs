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
    for line in reader.lines().map_while(Result::ok) {
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

/// 读取全部历史（按写入顺序）。
/// - 缺文件 = 从未记录过（真空态）→ 空 map 诚实；
/// - 文件存在但打开/读取失败 → 报错（诚实性族规 R88）：否则曲线区把
///   「读失败」展示成「没有历史」（读失败伪装成空历史）；
/// - 跨 root 的记录交错保存，按 root 聚合成 map 后返回。
pub(crate) fn read_history(
    app: &tauri::AppHandle,
) -> Result<HashMap<String, Vec<SpacePoint>>, String> {
    let Some(p) = history_path(app) else {
        return Err("无法定位数据目录".into());
    };
    read_history_at(&p)
}

/// 供单测使用的纯函数入口（不依赖 AppHandle）。
fn read_history_at(p: &Path) -> Result<HashMap<String, Vec<SpacePoint>>, String> {
    if !p.exists() {
        // 从未记录过（真空态），不是读失败。
        return Ok(HashMap::new());
    }
    let f = std::fs::File::open(p).map_err(|e| format!("读取空间历史失败: {e}"))?;
    let mut out: HashMap<String, Vec<SpacePoint>> = HashMap::new();
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        if let Ok(pt) = serde_json::from_str::<SpacePoint>(&line) {
            out.entry(pt.root.clone()).or_default().push(pt);
        }
    }
    for v in out.values_mut() {
        if v.len() > MAX_PER_ROOT {
            v.drain(..v.len() - MAX_PER_ROOT);
        }
    }
    Ok(out)
}

/// 读回并裁剪超长历史（写文件）。由 `get_space_history` 在返回前调用，
/// 保证 180 条上限真实落盘而不是只在内存裁剪。
pub(crate) fn prune_history(app: &tauri::AppHandle) {
    let Some(p) = history_path(app) else {
        return;
    };
    // 写路径（非展示面）：读不开就没法裁剪，静默返回即可。
    let Ok(map) = read_history_at(&p) else {
        return;
    };
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

/// 映射盘（`subst D: J:\D`）的扫描根换成它真正的目标根。
///
/// 扫 `D:\` 和扫 `J:\D` 是同一块盘上的同一份空间：历史里各记一条，
/// 总览趋势就会出现两条重复曲线，免费空间也被数两遍。纯函数，可测。
fn canonical_scan_root(key: &str, substs: &[(char, String)]) -> String {
    let b = key.as_bytes();
    if b.len() < 2 || !b[0].is_ascii_alphabetic() || b[1] != b':' {
        return key.to_string();
    }
    let letter = b[0].to_ascii_uppercase() as char;
    let Some((_, target)) = substs.iter().find(|(l, _)| *l == letter) else {
        return key.to_string();
    };
    // 目标卷根要保留尾斜杠（`J:\`），子目录不保留（`J:\D`），与扫描根本身口径一致。
    let mut target = target.trim_end_matches('\\').to_string();
    if target.len() <= 2 {
        target.push('\\');
    }
    let rest = key[2..].trim_start_matches('\\');
    if rest.is_empty() {
        target
    } else if target.ends_with('\\') {
        format!("{target}{}", rest.trim_end_matches('\\'))
    } else {
        format!("{target}\\{}", rest.trim_end_matches('\\'))
    }
}

/// 扫描完成后记录卷容量快照。`key` 是规范化扫描根（如 `C:\` / `D:\dir`）。
/// 映射盘先换成真实目标根，避免同一块盘在历史里出现两条曲线。
/// 通过 `crate::volume_info` 读卷容量（复用 list_drives 同款 Win32 查询）；
/// 读取失败静默（历史是增强功能，不能因它失败）。
pub(crate) fn record_after_scan(app: &tauri::AppHandle, key: &str) {
    let key = canonical_scan_root(key, &crate::drives::collect_substs());
    let Ok(info) = crate::drives::volume_info(key.clone()) else {
        return;
    };
    record_space_point(app, &key, info.total_bytes, info.used_bytes);
}

/// 为**全部**本机磁盘记录一次容量快照（R89：后台每日自动采样）。
/// 趋势曲线不再只依赖用户手动扫描——应用挂着时按天积累，总览页开卷就有历史。
/// 复用 list_drives 同款 Win32 查询；静默失败（增强功能不阻塞主流程，
/// 且 record_space_point 自带 60s 去重，与应用启动时的扫描点天然合并）。
pub(crate) fn record_all_drives(app: &tauri::AppHandle) {
    for info in crate::drives::list_drives() {
        // 与 record_after_scan 同口径：映射盘记到真实卷根，避免两条曲线。
        let key = canonical_scan_root(&info.path, &crate::drives::collect_substs());
        record_space_point(app, &key, info.total_bytes, info.used_bytes);
    }
}

/// 读取全部磁盘空间历史（只读 L0，总览页画趋势曲线）。
/// 返回 `{root -> [SpacePoint]}`，按写入顺序排列；缺文件 = 真空态返回空对象；
/// 存在但读失败 → Err（R88）：前端 AssetOverview 曲线区据此显示「未读到」。
#[tauri::command]
pub(crate) fn get_space_history(
    app: tauri::AppHandle,
) -> Result<HashMap<String, Vec<SpacePoint>>, String> {
    let map = read_history(&app)?;
    prune_history(&app);
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapped_drive_scans_land_on_the_real_volume_root() {
        let substs = vec![('D', r"J:\D".to_string())];
        // 本机实测：subst D: J:\D。扫 D:\ 必须记成 J:\D，和直接扫 J:\D 同一列。
        assert_eq!(canonical_scan_root(r"D:\", &substs), r"J:\D");
        assert_eq!(canonical_scan_root(r"D:\dir", &substs), r"J:\D\dir");
        assert_eq!(canonical_scan_root(r"D:\dir\sub", &substs), r"J:\D\dir\sub");
        // 大小写不敏感（盘符可能小写传来）。
        assert_eq!(canonical_scan_root(r"d:\", &substs), r"J:\D");
        // 普通盘原样过去，不该被动。
        assert_eq!(canonical_scan_root(r"C:\", &substs), r"C:\");
        assert_eq!(canonical_scan_root(r"J:\D", &substs), r"J:\D");
        assert_eq!(canonical_scan_root(r"C:\Users\x", &substs), r"C:\Users\x");
        // 映射到卷根：目标要保住尾斜杠，别写成 `J:`。
        let to_root = vec![('E', r"J:\".to_string())];
        assert_eq!(canonical_scan_root(r"E:\", &to_root), r"J:\");
        assert_eq!(canonical_scan_root(r"E:\tmp", &to_root), r"J:\tmp");
        // 没有映射盘时一切原样。
        assert_eq!(canonical_scan_root(r"D:\", &[]), r"D:\");
    }

    #[test]
    fn missing_file_returns_empty() {
        let dir = std::env::temp_dir().join(format!("space-hist-none-{}", std::process::id()));
        let p = dir.join("space-history.jsonl");
        let map = read_history_at(&p).expect("缺文件 = 真空态，不该报错");
        assert!(map.is_empty(), "缺文件必须返回空 map");
    }

    #[test]
    fn unreadable_file_is_error_not_empty() {
        // 读失败伪装成「没有历史」是 R88 修的谎言：存在但打不开 → Err。
        // Windows 上 File::open 一个目录会失败——路径同名占位即可构造。
        let dir = std::env::temp_dir().join(format!("space-hist-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("space-history.jsonl");
        std::fs::create_dir_all(&p).unwrap();
        assert!(p.exists(), "前置：路径存在");
        let r = read_history_at(&p);
        assert!(r.is_err(), "存在但读取失败必须报错，不得回空 map");
        let _ = std::fs::remove_dir_all(&dir);
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
        let map = read_history_at(&p).expect("有效文件必须读出");
        let pts = map.get("C:\\").expect("应有 C:\\ 历史");
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0].used_bytes, 40);
        assert_eq!(pts[2].used_bytes, 70);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_lines_are_skipped_silently() {
        // 目录名独立（space-hist-badlines）：与 unreadable_file_is_error_not_empty
        // 的 space-hist-bad 区分，避免并发测试互相删目录。
        let dir = std::env::temp_dir().join(format!("space-hist-badlines-{}", std::process::id()));
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
        let map = read_history_at(&p).expect("有效文件必须读出");
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
        let map = read_history_at(&p).expect("有效文件必须读出");
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
        let map =
            read_history_at(&dir.join("space-history.jsonl")).expect("缺文件 = 真空态，不该 Err");
        assert!(map.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
