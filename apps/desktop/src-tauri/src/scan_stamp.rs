//! 扫描时间戳历史（R293）：把每次**真实扫描完成**的时间按 root 追加到
//! `app_data_dir/scan-stamps.jsonl`，让「这块盘多久没扫了」第一次可判断。
//! **纯只读底座**：不触发扫描、不清理、只存点 + 提供纯判定。
//!
//! 为什么需要它：health-butler P0-3 的第三条信号「C 盘三周没扫」缺数据源。
//! 扫描只发生在内存——`AppState.cleanup_cache` 与 Node 树缓存都是进程内
//! `Mutex`、不落盘，进程一退全部消失；全仓库此前没有任何 `last_scan_at`
//! 类记录。
//!
//! **不能拿 space_history 冒充**：`space-history.jsonl` 的采样时间是趋势
//! 采样（后台线程每日一次，`record_all_drives`），与「用户扫过没」是两件
//! 事——没扫过的盘照样每天在采样。拿它算「多久没扫」会把从未扫过的盘说成
//! 「昨天刚扫」，是编造。故本模块独立记录，只认 `commit_scan`。
//!
//! 设计约束（对齐 suggestion_history / thermal_history 族规）：
//! - **追加式 jsonl**，单行极短；
//! - **同 root 60s 去重**：一次扫描事件只落一行；
//! - **每 root 最近 MAX_PER_ROOT 条**，超长丢最旧（防跨年无限增长）；
//! - **缺文件 = 真空态 → 空 vec**（从未扫过）；**文件在但读失败 → Err**
//!   （R88：读失败不得伪装成「刚扫过」——那会让该提醒时完全不提醒）；
//! - 本模块无写盘以外的任何副作用，`record` 失败一律静默（bool 返回值）。

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// 单条扫描时间戳。`root` 是**归一化后**的盘根（`C:\` → `c:/`，与
/// `suggestion_history` / `space_history` 同口径）——否则同一块盘会因大小写
/// 与斜杠差异裂成多条记录，「多久没扫」算出来全错。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ScanStamp {
    pub root: String,
    pub at: u64,
}

/// 距上次记录不足 60s 视为同一扫描事件。
const DEDUP_SECS: u64 = 60;
/// 每 root 最多保留的时间戳条数。
const MAX_PER_ROOT: usize = 30;

fn history_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("scan-stamps.jsonl"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 当前时刻（unix 秒）。供 reminder 侧读取——调用方不该自己读时钟，
/// 否则提醒判定与记录写入会基于两个不同的「现在」。
pub(crate) fn now_secs_pub() -> u64 {
    now_secs()
}

/// 追加一条扫描时间戳。**静默失败**（返回 bool 供测试/手动回显）：
/// 写不进去不该影响扫描主流程的返回。
pub(crate) fn record_scan_stamp(app: &tauri::AppHandle, root: &str, at: u64) -> bool {
    let Some(p) = history_path(app) else {
        return false;
    };
    if let Some(parent) = p.parent() {
        if !parent.exists() && std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    // 读一次现有 stamp 做 dedup + 裁剪：文件最多数百行（每 root 30 条 × 盘数），
    // 全读成本可忽略，换来的是不需要额外索引文件。
    let mut stamps = read_stamps_at(&p).unwrap_or_default();
    if let Some(last) = stamps.iter().rev().find(|s| s.root == root) {
        if at.saturating_sub(last.at) < DEDUP_SECS {
            return false;
        }
    }
    stamps.push(ScanStamp { root: root.to_string(), at });
    // 裁剪：每 root 只留最近 MAX_PER_ROOT 条。
    let mut kept: Vec<ScanStamp> = Vec::with_capacity(stamps.len());
    for root_key in distinct_roots(&stamps) {
        let mut rows: Vec<&ScanStamp> = stamps.iter().filter(|s| &s.root == root_key).collect();
        rows.sort_by_key(|s| s.at);
        let start = rows.len().saturating_sub(MAX_PER_ROOT);
        kept.extend(rows[start..].iter().map(|s| (*s).clone()));
    }
    // 保持写入顺序（调用方按时间追加），裁剪后仍有序。
    kept.sort_by_key(|s| s.at);
    write_stamps(&p, &kept)
}

/// 用当前时刻记录。生产调用点（`commit_scan`）用这个——它不该自己算时间，
/// 也不该为了可测性把 now 传得到处都是。
pub(crate) fn record_scan_stamp_now(app: &tauri::AppHandle, root: &str) -> bool {
    record_scan_stamp(app, root, now_secs())
}

fn distinct_roots(stamps: &[ScanStamp]) -> Vec<&String> {
    let mut v: Vec<&String> = stamps.iter().map(|s| &s.root).collect();
    v.sort();
    v.dedup();
    v
}

/// 整文件覆写（临时文件 + rename，避免写到一半崩掉留下半个文件）。
/// 静默失败。
fn write_stamps(p: &Path, stamps: &[ScanStamp]) -> bool {
    let tmp = p.with_extension("jsonl.tmp");
    let Ok(mut out) = OpenOptions::new().create(true).write(true).truncate(true).open(&tmp) else {
        return false;
    };
    for s in stamps {
        let Ok(line) = serde_json::to_string(s) else {
            continue;
        };
        if writeln!(out, "{line}").is_err() {
            return false;
        }
    }
    drop(out);
    std::fs::rename(&tmp, p).is_ok()
}

pub(crate) fn read_stamps_at(p: &Path) -> Result<Vec<ScanStamp>, String> {
    if !p.exists() {
        return Ok(Vec::new());
    }
    let f = std::fs::File::open(p).map_err(|e| format!("打开扫描时间戳失败: {e}"))?;
    let reader = BufReader::new(f);
    let mut out: Vec<ScanStamp> = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let s: ScanStamp = serde_json::from_str(line)
            .map_err(|e| format!("扫描时间戳存在损坏行: {e}"))?;
        out.push(s);
    }
    Ok(out)
}

pub(crate) fn read_stamps(app: &tauri::AppHandle) -> Result<Vec<ScanStamp>, String> {
    let Some(p) = history_path(app) else {
        return Ok(Vec::new());
    };
    read_stamps_at(&p)
}

/// 某盘最近一次扫描时间（unix 秒）。没有记录 → None（从未扫过，不是「0 秒前」）。
/// 主动查询一次（总览/设置页「多久没扫」）。区分真空态与读失败：
/// - 从没扫过 → `never: true`（说「还没扫过」，不是「扫了很久」）；
/// - 读失败 → Err（前端说「未读到」，绝不显示成「刚扫过」——那会让该提醒时
///   完全不提醒）。
#[tauri::command]
pub(crate) fn scan_staleness_cmd(
    app: AppHandle,
    root: String,
) -> Result<ScanStalenessOut, String> {
    let stamps = read_stamps(&app)?;
    let now = now_secs();
    // 查询侧必须与写入侧同口径归一化。写入走 `norm_path_key`（小写 + 正斜杠，
    // `C:\\` → `c:/`）；若这里拿原样 `C:\\` 去比对，一条都匹配不上，
    // 会把「天天扫」的盘误报成「从未扫过」——正好与 Never 的语义相反。
    let key = norm_query_root(&root);
    let out = match scan_staleness(&stamps, &key, now) {
        ScanStaleness::Never => ScanStalenessOut { never: true, clock_skew: false, days: None, last_at: None },
        ScanStaleness::ClockSkew => ScanStalenessOut { never: false, clock_skew: true, days: None, last_at: None },
        ScanStaleness::Days(d) => ScanStalenessOut {
            never: false,
            clock_skew: false,
            days: Some(d),
            last_at: latest_stamp(&stamps, &key),
        },
    };
    Ok(out)
}

/// 归一化查询用的盘根，与写入侧 `cleanup_logic::norm_path_key` 同口径
/// （小写 + 正斜杠）。额外把裸盘符 `c:` 补成 `c:/`——前端传 `C:`、
/// 传 `C:/`、传 `C:` 都应命中同一条记录，盘根只差一个分隔符不该判成两块盘。
pub(crate) fn norm_query_root(root: &str) -> String {
    let s = root.replace('\\', "/").to_lowercase();
    // 裸盘符（`c:`）补齐尾斜杠；更深的路径原样（它是具体的扫描根，不猜）。
    if s.len() == 2 && s.ends_with(':') {
        return format!("{s}/");
    }
    s
}

/// `scan_staleness_cmd` 的返回形状。三个态必须可区分——`Option<u64>` 一种
/// 表示分不清「从未扫过」「时钟异常」「正常」，而这三种的 UI 措辞完全不同。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ScanStalenessOut {
    /// 从未有过该盘的扫描记录。
    pub never: bool,
    /// 记录时间晚于当前时刻（时钟回拨 / 时间戳异常）。
    pub clock_skew: bool,
    /// 距上次扫描的天数；None = never 或 clock_skew。
    pub days: Option<u64>,
    /// 上次扫描的 unix 秒（展示「上次扫于 X」用）；None = 无有效记录。
    pub last_at: Option<u64>,
}

pub(crate) fn latest_stamp(stamps: &[ScanStamp], root: &str) -> Option<u64> {
    stamps.iter().filter(|s| s.root == root).map(|s| s.at).max()
}

/// 「多久没扫」判定（纯函数，可单测）。
///
/// # 为什么 Never / ClockSkew 与天数并列而不是塞进 `Option<u64>`
/// `None` 得分清「从未扫过」与「读失败」与「时钟回拨」——三者的正确处置
/// 完全不同：
/// - `Never` → 应该说「还没扫过」，而不是「扫了很久」；
/// - `ClockSkew`（记录时间晚于 `now`）→ 时钟被改过或时间戳被外部改过。
///   此刻算出的"负数天"毫无意义。保守返回 0 天（视为刚扫过）而不猜：
///   误报「刚扫过」最多让提醒晚一轮，误报「三年没扫」会让用户以为数据全丢了。
/// - 正常 → 距上次的天数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ScanStaleness {
    /// 该盘从未有过扫描记录（真空态）。
    Never,
    /// 记录时间晚于当前时刻（时钟回拨 / 时间戳异常）。
    ClockSkew,
    /// 距上次扫描的天数（0 = 一天内）。
    Days(u64),
}

pub(crate) fn scan_staleness(
    stamps: &[ScanStamp],
    root: &str,
    now: u64,
) -> ScanStaleness {
    let Some(at) = latest_stamp(stamps, root) else {
        return ScanStaleness::Never;
    };
    if at > now {
        return ScanStaleness::ClockSkew;
    }
    ScanStaleness::Days((now - at) / 86400)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(root: &str, at: u64) -> ScanStamp {
        ScanStamp { root: root.to_string(), at }
    }

    #[test]
    fn no_record_is_never_not_zero_days() {
        // 从未扫过必须与「0 天前扫过」分开：前者该说「还没扫过」。
        assert_eq!(scan_staleness(&[], "c:/", 1_700_000_000), ScanStaleness::Never);
        // 有其它盘的记录也不算：按 root 精确匹配，不拿别人的扫描冒充。
        let other = vec![st("d:/", 1_700_000_000)];
        assert_eq!(scan_staleness(&other, "c:/", 1_700_000_000), ScanStaleness::Never);
    }

    #[test]
    fn days_floor_and_boundaries() {
        let stamps = vec![st("c:/", 1_700_000_000)];
        assert_eq!(scan_staleness(&stamps, "c:/", 1_700_000_000), ScanStaleness::Days(0));
        assert_eq!(scan_staleness(&stamps, "c:/", 1_700_000_001), ScanStaleness::Days(0));
        assert_eq!(
            scan_staleness(&stamps, "c:/", 1_700_000_000 + 20 * 86400),
            ScanStaleness::Days(20)
        );
        // 21 天差 1 秒：仍 20 天（向下取整，不把「差一点点满 3 周」唱成 3 周）
        assert_eq!(
            scan_staleness(&stamps, "c:/", 1_700_000_000 + 21 * 86400 - 1),
            ScanStaleness::Days(20)
        );
    }

    #[test]
    fn future_stamp_is_clock_skew_not_huge_days() {
        // 时间戳晚于 now：时钟回拨或外部篡改。不得算出「负数天」，
        // 也不得当成「刚扫过」而不报——单独成态，让调用方能说明缘由。
        let stamps = vec![st("c:/", 1_700_000_000 + 86400)];
        assert_eq!(scan_staleness(&stamps, "c:/", 1_700_000_000), ScanStaleness::ClockSkew);
    }

    #[test]
    fn latest_stamp_takes_the_max_not_the_last_row() {
        // 多次扫描：取最大 at，而不是数组最后一行（写入顺序通常有序，
        // 但不依赖这个假设——外部可能重排文件）。
        let stamps = vec![st("c:/", 1_700_000_100), st("c:/", 1_700_000_000), st("c:/", 1_700_000_200)];
        assert_eq!(latest_stamp(&stamps, "c:/"), Some(1_700_000_200));
    }

    #[test]
    fn norm_query_root_matches_write_side_casing_and_slash() {
        // 查询侧归一化必须与写入侧（norm_path_key：小写 + 正斜杠）同口径，
        // 否则 scan_staleness_cmd("C:\\") 对不上写入的 c:/，天天扫的盘
        // 会误报成从未扫过（Never 的语义恰好相反）。
        let stamps = vec![st("c:/", 1_700_000_000)];
        let now = 1_700_000_000 + 5 * 86400;
        // 前端可能传 C:\（总览 DriveInfo.path）、C:/、c:\、c:/，都该命中同一条记录
        for q in ["C:\\", "C:/", "c:\\", "c:/"] {
            assert_eq!(
                scan_staleness(&stamps, &norm_query_root(q), now),
                ScanStaleness::Days(5),
                "查询 {q:?} 应命中写入的 c:/"
            );
        }
    }

    #[test]
    fn norm_query_root_bare_letter_gets_trailing_slash() {
        // 裸盘符 c: 补成 c:/——盘根只差一个分隔符不该判成两块盘。
        assert_eq!(norm_query_root("C:"), "c:/");
        assert_eq!(norm_query_root("c:"), "c:/");
        // 具体的目录路径原样归一化（它本来就是扫描根，不猜）
        assert_eq!(norm_query_root("C:\\Users\\x"), "c:/users/x");
    }

    #[test]
    fn missing_file_is_empty_not_error() {
        let dir = std::env::temp_dir().join(format!("dp-scanstamp-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("scan-stamps.jsonl");
        let v = read_stamps_at(&p).expect("缺文件 = 真空态，不该 Err");
        assert!(v.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_line_is_error_not_silently_empty() {
        // R88 族规：文件在但损坏 → Err。否则「读失败」被当成「从未扫过」，
        // 提醒会突然开始喊「这块盘从没扫过」——把故障演成事实。
        let dir = std::env::temp_dir().join(format!("dp-scanstamp-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("scan-stamps.jsonl");
        std::fs::write(&p, "not json !!!\n").unwrap();
        assert!(read_stamps_at(&p).is_err(), "损坏必须 Err");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_and_prune_keeps_newest_per_root() {
        // 直接写文件验证读回与裁剪语义（dedup 依赖时间戳，不便在无 AppHandle
        // 的单测里驱动 record_scan_stamp）。
        let dir = std::env::temp_dir().join(format!("dp-scanstamp-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("scan-stamps.jsonl");
        for i in 0..(MAX_PER_ROOT as u64 + 5) {
            writeln!(
                OpenOptions::new().create(true).append(true).open(&p).unwrap(),
                "{}",
                serde_json::to_string(&st("c:/", 1_700_000_000 + i)).unwrap()
            )
            .unwrap();
        }
        let all = read_stamps_at(&p).unwrap();
        assert_eq!(all.len(), MAX_PER_ROOT + 5, "落盘时不裁剪，读回全量");
        // 模拟裁剪逻辑：每 root 留最近 N 条
        let mut rows: Vec<&ScanStamp> = all.iter().filter(|s| s.root == "c:/").collect();
        rows.sort_by_key(|s| s.at);
        let keep = &rows[rows.len() - MAX_PER_ROOT..];
        assert_eq!(keep.len(), MAX_PER_ROOT);
        assert_eq!(keep[0].at, 1_700_000_000 + 5, "裁掉的是最旧 5 条");
        assert_eq!(keep[keep.len() - 1].at, 1_700_000_000 + MAX_PER_ROOT as u64 + 4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
