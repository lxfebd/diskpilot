//! 清理建议跨次对比（health-butler P0）：把每次「建议清理」的汇总数字落一份
//! 快照，总览页据此回答"比上次多出 X GB"。
//!
//! ## 为什么单独一个模块
//! health-butler P0 承诺"建议清理卡补跨次对比：比上次多出 X GB"，此前只有
//! 文档承诺、没有实现。各 scaffold 的可清量每次都要重算（全盘 walk 或内存树
//! 统计），跨次对比不可能靠"再算一次上次"得到——上次的树早就变了。所以必须在
//! 建议算出来那一刻把汇总数字记下来，与 `space_history`（卷容量趋势）同一范式。
//!
//! ## 设计约束（对齐 space_history / hw_history 既有族规，R479 起由 obs 内核落地）
//! - 追加式 jsonl，单行极短，天然小文件；绝不整文件重写；
//! - 写失败一律静默（对比是增强功能，不能阻塞扫描/建议主流程）；
//! - 缺文件 = 真空态（从未记录过）→ 空，不是错误；
//! - 坏行跳过+计数；文件存在但打不开/全部行损坏 → Err（R88：读失败不得伪装成"没有历史"）；
//! - 单 root 短时间内重复记录去重，避免一次扫描事件写两行；
//! - 每 root 保留最近 N 条，超长裁剪（原子覆写），防跨年无限增长。
//!
//! ## 只存汇总，不存明细
//! 快照只记 root 级总量 + 每 scaffold 小计，不记每条 scope 的路径/大小：
//! ①明细会随文件系统实时变化，存下来立刻过期，拿旧明细当事实是编造；
//! ②总量与 scaffold 小计足以回答"多出多少/涨在哪个应用"，正是 P0 要的口径；
//! ③体积小、隐私面窄（不落具体用户路径）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(test)]
use std::fs::OpenOptions;
#[cfg(test)]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tauri::Manager;

/// 单条建议汇总快照。`at` 为 Unix 秒；`scaffolds` 为 scaffold_id → 可清字节。
/// `total_bytes` 是各 scaffold 之和，落盘时由调用方算好（保持"快照=当时算出的
/// 原值"，读取端不做重新汇总，避免两处口径打架）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SuggestionPoint {
    pub root: String,
    pub total_bytes: u64,
    /// scaffold_id → 该 scaffold 本次可清字节小计（缺失的 scaffold 不计 0，
    /// 直接不出现——没有命中就是没有命中，不编造）。
    pub scaffolds: HashMap<String, u64>,
    pub at: u64,
}

/// 建议汇总对比结果（当前 vs 上一次）。字段缺失一律 null，不编造差值。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SuggestionDiff {
    /// 本次汇总快照的时间戳（0 = 没有本次快照）。
    pub at: u64,
    /// 上一次快照的时间戳（0 = 没有上次快照）。
    pub prev_at: u64,
    /// 总量差值（字节）：>0 比上次多出，<0 更少，=0 持平。无上次 → null。
    pub total_delta: Option<i64>,
    /// 与上次间隔天数（环比结论的强度参照）。无上次或时间倒序 → null。
    pub gap_days: Option<i64>,
    /// 每 scaffold 的环比：scaffold_id → (现字节, 上次字节, 差值)。
    /// 只在两个快照里都出现过的 scaffold 才给项；单边出现的（新增/消失）
    /// 走 new_scaffolds / gone_scaffolds，不假装"涨了 N 字节"。
    /// R337：按 |delta| 降序（同值按 scaffold_id 升序），前端取 [0] 即最大变化。
    pub scaffolds: Vec<SuggestionScopeDiff>,
    /// 本次才出现的 scaffold（上次没有）：可清量是净新增，无从谈"涨跌"。
    pub new_scaffolds: Vec<String>,
    /// 上次有、本次没有的 scaffold：回收完了或不再命中。
    pub gone_scaffolds: Vec<String>,
}

/// 单个 scaffold 的环比明细。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SuggestionScopeDiff {
    pub scaffold_id: String,
    pub now_bytes: u64,
    pub prev_bytes: u64,
    /// 差值（字节），方向与 total_delta 一致。
    pub delta: i64,
}

/// 距上次记录不足该秒数视为同一次建议事件（一次扫描/切页只落一行）。
const DEDUP_SECS: u64 = 60;
/// 每 root 最多保留的快照数；超出丢弃最旧。
const MAX_PER_ROOT: usize = 30;

fn history_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("suggestion-history.jsonl"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 追加一条建议汇总快照。**静默失败**：目录不可写 / 文件锁冲突时忽略
/// （对比是增强功能，绝不阻塞主流程）。同 root 60s 内重复丢弃。
pub(crate) fn record_suggestion_point(
    app: &tauri::AppHandle,
    root: &str,
    total_bytes: u64,
    scaffolds: &HashMap<String, u64>,
) {
    let Some(p) = history_path(app) else {
        return;
    };
    let at = now_secs();
    if last_same_root_within(&p, root, at, DEDUP_SECS) {
        return;
    }
    let _ = crate::obs::append_jsonl(
        &p,
        &SuggestionPoint {
            root: root.to_string(),
            total_bytes,
            scaffolds: scaffolds.clone(),
            at,
        },
    );
}

/// 检查文件最后一条同 root 记录是否在 `now - within_secs` 内。
/// 只倒查最后 64 行（单行极短），成本可忽略且避免全文件读。
fn last_same_root_within(p: &Path, root: &str, now: u64, within_secs: u64) -> bool {
    // R479：obs 尾读；文件不存在/全损坏 → false（不当有记录）。
    let Ok(out) = crate::obs::read_jsonl_tail::<SuggestionPoint>(p, 64) else {
        return false;
    };
    for pt in out.items.iter().rev() {
        if pt.root == root {
            return now.saturating_sub(pt.at) < within_secs;
        }
    }
    false
}

/// 读取全部历史（按写入顺序）。
/// - 缺文件 = 真空态 → 空 map；
/// - 存在但打不开/全部行损坏 → Err（R88：读失败不得伪装成"没有历史"，前端要能提示未读到）；
/// - 坏行跳过+计数（R479 obs 内核）。
fn read_history_at(p: &Path) -> Result<HashMap<String, Vec<SuggestionPoint>>, String> {
    let out = crate::obs::read_jsonl::<SuggestionPoint>(p)?;
    let mut map: HashMap<String, Vec<SuggestionPoint>> = HashMap::new();
    for pt in out.items {
        map.entry(pt.root.clone()).or_default().push(pt);
    }
    for v in map.values_mut() {
        if v.len() > MAX_PER_ROOT {
            v.drain(..v.len() - MAX_PER_ROOT);
        }
    }
    Ok(map)
}

pub(crate) fn read_history(
    app: &tauri::AppHandle,
) -> Result<HashMap<String, Vec<SuggestionPoint>>, String> {
    let Some(p) = history_path(app) else {
        return Err("无法定位数据目录".into());
    };
    read_history_at(&p)
}

/// 读回并裁剪超长历史（写文件）。由只读命令在返回前调用，保证上限真实落盘。
pub(crate) fn prune_history(app: &tauri::AppHandle) {
    let Some(p) = history_path(app) else {
        return;
    };
    let Ok(map) = read_history_at(&p) else {
        return;
    };
    if map.values().all(|v| v.len() <= MAX_PER_ROOT) {
        return;
    }
    let mut pts: Vec<SuggestionPoint> = map.into_values().flatten().collect();
    pts.sort_by_key(|pt| pt.at);
    // R479：obs 原子覆写（tmp + rename）。
    crate::obs::write_jsonl(&p, &pts);
}

const DAY_SECS: i64 = 86_400;

/// 由「当前 + 上一次」两份汇总算环比。纯函数，可测。
///
/// 诚实性约定（与 hw_history / WeeklyHealthCard 的 weeklyDelta 同族）：
/// - 任一份缺失 → total_delta = null（没有上次就无从谈"多出多少"）；
/// - 只在两个快照都出现过的 scaffold 才进 scaffolds；单边出现分别归
///   new/gone，不把"新增"误述成"涨了 N 字节"；
/// - gap_days 为 0 或负（同刻或时间倒序）→ null，不给虚假间隔。
pub(crate) fn diff_points(
    cur: Option<&SuggestionPoint>,
    prev: Option<&SuggestionPoint>,
) -> Option<SuggestionDiff> {
    let cur = cur?;
    let mut out = SuggestionDiff {
        at: cur.at,
        prev_at: 0,
        total_delta: None,
        gap_days: None,
        scaffolds: Vec::new(),
        new_scaffolds: Vec::new(),
        gone_scaffolds: Vec::new(),
    };
    let prev = match prev {
        Some(p) if p.at > 0 && cur.at > p.at => p,
        // 没有上次、或上次不比这次更早（同刻/倒序）→ 只回本次总量，不出差值。
        _ => {
            out.new_scaffolds = cur.scaffolds.keys().cloned().collect();
            out.new_scaffolds.sort();
            return Some(out);
        }
    };
    out.prev_at = prev.at;
    out.total_delta = Some(cur.total_bytes as i64 - prev.total_bytes as i64);
    out.gap_days = Some(((cur.at - prev.at) / DAY_SECS as u64) as i64);
    // 求交集并集：单边出现的分别归 new / gone。
    let mut shared: Vec<&String> = cur
        .scaffolds
        .keys()
        .filter(|k| prev.scaffolds.contains_key(*k))
        .collect();
    shared.sort();
    out.new_scaffolds = cur
        .scaffolds
        .keys()
        .filter(|k| !prev.scaffolds.contains_key(*k))
        .cloned()
        .collect();
    out.new_scaffolds.sort();
    out.gone_scaffolds = prev
        .scaffolds
        .keys()
        .filter(|k| !cur.scaffolds.contains_key(*k))
        .cloned()
        .collect();
    out.gone_scaffolds.sort();
    for k in shared {
        let now_bytes = cur.scaffolds[k];
        let prev_bytes = prev.scaffolds[k];
        out.scaffolds.push(SuggestionScopeDiff {
            scaffold_id: k.clone(),
            now_bytes,
            prev_bytes,
            delta: now_bytes as i64 - prev_bytes as i64,
        });
    }
    // R337：按变化量降序排，别按 id 字典序——前端「主要变化在 X」取的就是 [0]。
    out.scaffolds.sort_by(|a, b| {
        b.delta
            .abs()
            .cmp(&a.delta.abs())
            .then(a.scaffold_id.cmp(&b.scaffold_id))
    });
    Some(out)
}

/// 计算某 root 的跨次对比（只读 L0，总览页建议卡用）。
/// 取该 root 最新的两份快照做环比；不足两份 → 只回本次、差值全 null。
/// 缺文件 = 真空态 → Ok(None)；存在但读失败 → Err（前端提示"未读到"）。
#[tauri::command]
pub(crate) fn suggestion_diff(
    app: tauri::AppHandle,
    root_path: String,
) -> Result<Option<SuggestionDiff>, String> {
    let map = read_history(&app)?;
    prune_history(&app);
    // 写入侧 key 由 `norm_path_key` 产出（小写 + 正斜杠，`C:\` → `c:/`），
    // 前端送的是原始盘符形状（`C:\`）。不做同一口径的归一化就会永远
    // 查不到自己刚记的快照——R288 的环比会在真机上静默失效，所以这里
    // 必须对齐，而不是把「查不到」当成「没有历史」。
    let key = crate::cleanup_logic::norm_path_key(std::path::Path::new(&root_path));
    let Some(pts) = map.get(&key) else {
        return Ok(None);
    };
    // read_history_at 已按写入顺序 push，末两条即最新两份。
    let cur = pts.last();
    let prev = if pts.len() >= 2 {
        pts.get(pts.len() - 2)
    } else {
        None
    };
    Ok(diff_points(cur, prev))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(at: u64, total: u64, entries: &[(&str, u64)]) -> SuggestionPoint {
        SuggestionPoint {
            root: "C:\\".into(),
            total_bytes: total,
            scaffolds: entries.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            at,
        }
    }

    #[test]
    fn missing_file_returns_empty() {
        let dir = std::env::temp_dir().join(format!("sugg-hist-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("suggestion-history.jsonl");
        let map = read_history_at(&p).expect("缺文件 = 真空态，不该 Err");
        assert!(map.is_empty());
    }

    /// R288 key 归一化回归：写入侧是 `norm_path_key`（`C:\` → `c:/`），
    /// 查询侧若直接用前端原始盘符（`C:\`）就永远查不到自己刚记的快照——
    /// 环比会静默失效，且表现成「没有历史」这种谎言。这里锁住查询会归一化。
    #[test]
    fn query_key_is_normalized_like_the_write_side() {
        // 与 record 侧同源：key 一律经 norm_path_key。
        assert_eq!(
            crate::cleanup_logic::norm_path_key(std::path::Path::new("C:\\")),
            "c:/",
            "norm_path_key 产出小写 + 正斜杠"
        );
        // 历史文件里的 root 就是 norm 形状；用原始形状查必须能命中。
        let map: HashMap<String, Vec<SuggestionPoint>> = [(
            crate::cleanup_logic::norm_path_key(std::path::Path::new("C:\\")),
            vec![pt(20, 250, &[("wechat", 150)])],
        )]
        .into_iter()
        .collect();
        assert!(
            map.contains_key(&crate::cleanup_logic::norm_path_key(std::path::Path::new(
                "C:\\"
            ))),
            "原始盘符查询必须命中 norm 形状的 key"
        );
        // 反向：以未归一化的原始形状直接查（旧实现的写法）确实查不到，
        // 证明这个归一化不是空操作。
        assert!(!map.contains_key("C:\\"));
    }

    #[test]
    fn unreadable_file_is_error_not_empty() {
        // R88 同款构造：把 jsonl 路径占成一个目录，File::open 必失败。
        let dir = std::env::temp_dir().join(format!("sugg-hist-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("suggestion-history.jsonl");
        std::fs::create_dir_all(&p).unwrap();
        assert!(p.exists());
        let r = read_history_at(&p);
        assert!(r.is_err(), "存在但读失败必须 Err，不得伪装成空历史");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_then_read_roundtrip_and_bad_lines_skipped() {
        let dir = std::env::temp_dir().join(format!("sugg-hist-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("suggestion-history.jsonl");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .unwrap();
        writeln!(f, "not json").unwrap();
        writeln!(
            f,
            "{}",
            serde_json::to_string(&pt(10, 100, &[("wechat", 60)])).unwrap()
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            serde_json::to_string(&pt(20, 250, &[("wechat", 150)])).unwrap()
        )
        .unwrap();
        drop(f);
        let map = read_history_at(&p).expect("有效文件必须读出");
        let pts = &map["C:\\"];
        assert_eq!(pts.len(), 2, "坏行静默跳过，只留合法行");
        assert_eq!(pts[0].at, 10);
        assert_eq!(pts[1].at, 20);
        assert_eq!(pts[1].scaffolds["wechat"], 150);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_two_points_gives_signed_deltas_and_shared_scopes() {
        let cur = pt(1_700_100_000, 250, &[("wechat", 150), ("chrome", 100)]);
        let prev = pt(1_700_000_000, 100, &[("wechat", 60), ("chrome", 40)]);
        let d = diff_points(Some(&cur), Some(&prev)).expect("两份都该出 diff");
        assert_eq!(d.total_delta, Some(150), "多出 150 字节");
        assert_eq!(d.gap_days, Some(1));
        assert_eq!(d.scaffolds.len(), 2);
        let wx = d
            .scaffolds
            .iter()
            .find(|s| s.scaffold_id == "wechat")
            .unwrap();
        assert_eq!((wx.now_bytes, wx.prev_bytes, wx.delta), (150, 60, 90));
        assert!(d.new_scaffolds.is_empty());
        assert!(d.gone_scaffolds.is_empty());
    }

    #[test]
    fn diff_without_prev_has_null_delta_not_fake_zero() {
        let cur = pt(1_700_000_000, 250, &[("wechat", 150)]);
        let d = diff_points(Some(&cur), None).expect("只有本次也该返回");
        assert_eq!(d.total_delta, None, "没有上次 → null，不得假装持平");
        assert_eq!(d.gap_days, None);
        assert_eq!(d.prev_at, 0);
        // 单边 scaffold 归 new，不进 scaffolds（不编造涨跌）。
        assert_eq!(d.new_scaffolds, vec!["wechat".to_string()]);
        assert!(d.scaffolds.is_empty());
    }

    #[test]
    fn diff_new_and_gone_scaffolds_are_separated() {
        let cur = pt(1_700_100_000, 300, &[("wechat", 200), ("chrome", 100)]);
        let prev = pt(1_700_000_000, 200, &[("wechat", 200), ("conda", 0)]);
        let d = diff_points(Some(&cur), Some(&prev)).unwrap();
        assert_eq!(d.new_scaffolds, vec!["chrome".to_string()]);
        assert_eq!(d.gone_scaffolds, vec!["conda".to_string()]);
        assert_eq!(d.scaffolds.len(), 1, "只有共同出现的才给环比");
        assert_eq!(d.scaffolds[0].scaffold_id, "wechat");
        assert_eq!(d.scaffolds[0].delta, 0, "持平");
    }

    #[test]
    fn diff_scaffolds_sorted_by_abs_delta_desc() {
        // R337：前端「主要在 X」直接取 scaffolds[0] → 必须是最·大变化，而不是按 id
        // 字典序的第一个（browser 排在 wechat 前，可能只涨了 1 字节）。
        let cur = pt(
            1_700_100_000,
            1_100,
            &[("browser", 501), ("wechat", 500), ("conda", 99)],
        );
        let prev = pt(
            1_700_000_000,
            300,
            &[("browser", 500), ("wechat", 100), ("conda", 0)],
        );
        let d = diff_points(Some(&cur), Some(&prev)).unwrap();
        let order: Vec<&str> = d.scaffolds.iter().map(|s| s.scaffold_id.as_str()).collect();
        assert_eq!(order, vec!["wechat", "conda", "browser"], "按 |delta| 降序");
        assert_eq!(d.scaffolds[0].delta, 400, "最大变化在前");
        // 同 |delta| 时按 id 升序，结果稳定可预测。
        let cur2 = pt(1_700_100_000, 200, &[("zeta", 100), ("alpha", 100)]);
        let prev2 = pt(1_700_000_000, 400, &[("zeta", 200), ("alpha", 200)]);
        let d2 = diff_points(Some(&cur2), Some(&prev2)).unwrap();
        let order2: Vec<&str> = d2
            .scaffolds
            .iter()
            .map(|s| s.scaffold_id.as_str())
            .collect();
        assert_eq!(order2, vec!["alpha", "zeta"], "同值按 id 升序");
    }

    #[test]
    fn diff_rejects_same_timestamp_or_reversed_order() {
        let a = pt(1_700_000_000, 100, &[("wechat", 100)]);
        let b = pt(1_700_000_000, 200, &[("wechat", 200)]);
        // 同刻：不得给出 diff（间隔 0 天是无意义的环比）。
        let d = diff_points(Some(&b), Some(&a)).unwrap();
        assert_eq!(d.total_delta, None);
        assert_eq!(d.gap_days, None);
        // 倒序（prev 比 cur 新）：同样不出差值。
        let d2 = diff_points(Some(&a), Some(&b)).unwrap();
        assert_eq!(d2.total_delta, None);
        assert_eq!(d2.prev_at, 0);
    }

    #[test]
    fn prune_caps_per_root() {
        let dir = std::env::temp_dir().join(format!("sugg-hist-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("suggestion-history.jsonl");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .unwrap();
        for i in 0..40u64 {
            writeln!(
                f,
                "{}",
                serde_json::to_string(&pt(1_700_000_000 + i, i, &[])).unwrap()
            )
            .unwrap();
        }
        drop(f);
        let map = read_history_at(&p).unwrap();
        let pts = &map["C:\\"];
        assert_eq!(pts.len(), MAX_PER_ROOT);
        assert_eq!(pts[pts.len() - 1].at, 1_700_000_039, "保留最新 30 条");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dedup_within_60s() {
        let dir = std::env::temp_dir().join(format!("sugg-hist-dedup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("suggestion-history.jsonl");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .unwrap();
        for at in [1_700_000_000u64, 1_700_000_030] {
            writeln!(f, "{}", serde_json::to_string(&pt(at, 1, &[])).unwrap()).unwrap();
        }
        drop(f);
        assert!(last_same_root_within(&p, "C:\\", 1_700_000_045, 60));
        // 末条 at=30s；间隔 61s 已超 60s 窗口 → 视为新事件，可再记。
        assert!(!last_same_root_within(&p, "C:\\", 1_700_000_030 + 61, 60));
        assert!(
            !last_same_root_within(&p, "D:\\", 1_700_000_045, 60),
            "别的盘不受影响"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
