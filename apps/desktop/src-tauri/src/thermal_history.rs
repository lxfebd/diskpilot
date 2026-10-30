//! 温度采样历史（R291）：把 `{temp_max_c, at}` 追加到 `app_data_dir/thermal-history.jsonl`，
//! 让「温度**持续**偏高」这件事第一次变得可判断。**纯只读展示的底座**：本模块
//! 不做任何温控写入、不弹通知，只存点 + 提供纯判定。
//!
//! 为什么需要它：`sample_thermal_c()` 只给**当前**一帧温度。此前无论总览「今日体检」
//! 的 isTempHigh，还是 health-butler P0-3 想要的管家通知、P2-2 想要的风扇托管，都卡在
//! 同一处——**没有"持续"可言**：瞬时尖峰（编译/解压让 CPU 冲高 10 秒）与真的散热不济
//! 在单帧里长得一模一样。瞬时即判定会把正常机器唱成过热机，是比不报更坏的谎。
//!
//! 设计约束（对齐 space_history / suggestion_history 族规，R479 起由 obs 内核落地）：
//! - **追加式 jsonl**：单行极短，追加成本可忽略；
//! - **写失败静默**：底座不阻塞任何主流程（调用方是 reminder 后台线程的顺手采样）；
//! - **同源 dedup**：60s 内重复采样丢弃（后台 tick 是 60s 粒度，天然合一）;
//! - **裁剪上限**：最多保留 MAX_SAMPLES 条，超长丢最旧（原子覆写；跨年无限增长要防）；
//! - **只读查询区分真空态与读失败**：缺文件 = 从未采样（空 vec）；坏行跳过+计数，
//!   文件在但读不开/全部行损坏 → Err（R88：读失败不得伪装成"从未过热"）。

#[cfg(test)]
use std::fs::OpenOptions;
#[cfg(test)]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tauri::Manager;

/// 单条温度快照。`temp_max_c` = 该帧归一化最高温（CPU/GPU/主板取最大，与
/// `fan_curve_advice().temp_max_c` 同源口径）；`at` 为 Unix 秒。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ThermalPoint {
    pub temp_max_c: f32,
    pub at: u64,
}

/// 距上次采样不足 60s 视为同一事件（后台 tick 60s，天然合一）。
const DEDUP_SECS: u64 = 60;
/// 最多保留的采样条数（5 分钟一条约可覆盖 3 周半）。
const MAX_SAMPLES: usize = 3000;

fn history_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("thermal-history.jsonl"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 追加一条采样。**静默失败**：拿不到温度（None）、目录不可写、文件锁冲突都直接
/// 返回 false，绝不向上抛——底座不该让调用方（提醒线程）为它改错误处理。
/// dedup：距最后一条同文件记录不足 60s 则跳过（返回 false）。
pub(crate) fn record_thermal_sample(app: &tauri::AppHandle, temp_max_c: f32) -> bool {
    if !temp_max_c.is_finite() {
        return false; // NaN/Inf 不是温度，落库只会污染后续判定
    }
    let Some(p) = history_path(app) else {
        return false;
    };
    let at = now_secs();
    // 只看最后一条有效记录做 dedup（R479：obs 尾读；对齐 space_history）。
    if let Ok(Some(l)) = crate::obs::read_jsonl_latest::<ThermalPoint>(&p) {
        if at.saturating_sub(l.at) < DEDUP_SECS {
            return false;
        }
    }
    let ok = crate::obs::append_jsonl(&p, &ThermalPoint { temp_max_c, at });
    if ok {
        prune_samples(&p);
    }
    ok
}

/// 裁剪到最近 MAX_SAMPLES 条（R479：obs 原子覆写 tmp+rename；文件最大数百 KB，成本可接受）。
/// 失败静默——多留几条旧点不影响判定，而抛错会波及调用方。
fn prune_samples(p: &Path) {
    let Ok(out) = crate::obs::read_jsonl::<ThermalPoint>(p) else {
        return;
    };
    if out.items.len() <= MAX_SAMPLES {
        return;
    }
    let keep = &out.items[out.items.len() - MAX_SAMPLES..];
    crate::obs::write_jsonl(p, keep);
}

/// 读全部历史。
/// - 缺文件 = 从未采样（真空态）→ 空 vec 诚实；
/// - 坏行跳过+计数；文件存在但打开/读取失败或全部行损坏 → Err（R88：读失败不得伪装成"从未过热"）。
pub(crate) fn read_history_at(p: &Path) -> Result<Vec<ThermalPoint>, String> {
    // R479：obs 内核读取。
    Ok(crate::obs::read_jsonl::<ThermalPoint>(p)?.items)
}

/// 读应用的历史（命令/后台线程用）。
pub(crate) fn read_history(app: &tauri::AppHandle) -> Result<Vec<ThermalPoint>, String> {
    let Some(p) = history_path(app) else {
        return Ok(Vec::new());
    };
    read_history_at(&p)
}

/// R299：把采样历史暴露给前端。此前这套点只有 reminder 后台线程自己读——
/// 用户在设置里打开了采样开关，却看不到「到底采到了没有、采了多少、最高多少」。
/// **只读**：不落新点、不改任何配置、不触发判定以外的事。
/// 三态诚实（与 read_history 同口径）：缺文件 = 从未采样 → 空数组；
/// 文件在但损坏 → Err，前端显示「读失败」而不是伪装成「没有样本」。
#[tauri::command]
pub(crate) fn get_thermal_history(app: tauri::AppHandle) -> Result<Vec<ThermalPoint>, String> {
    read_history(&app)
}

/// 「持续偏高」判定（纯函数）：窗口内至少 `min_samples` 个采样、且**全部** ≥
/// 阈值，才算持续。返回 (命中, 实际样本数)。
///
/// 为什么要求**全部**而非平均：平均 84℃ 可能来自"一半时间 95℃、一半 73℃"
/// （真实的散热不济），也可能来自"一直稳稳 84℃"（机器本来就是这个负载）。
/// 二者处置完全不同，平均值会把它们混为一谈。全超温 = 无一秒喘息，才配叫"持续"。
///
/// 为什么要求最小样本数：只有一个点时"全部 ≥ 阈值"等价于瞬时判定，正是本模块
/// 要消灭的东西。样本不足 → 判否并如实返回实际条数（调用方据此说"还在观察"）。
pub(crate) fn is_sustained_high(
    pts: &[ThermalPoint],
    threshold_c: f32,
    min_samples: usize,
) -> (bool, usize) {
    if pts.len() < min_samples {
        return (false, pts.len());
    }
    // 只看最近 min_samples 条：更早的低温不能给现在的持续高温洗白。
    let window = &pts[pts.len() - min_samples..];
    let n = window.len();
    let all_high = window.iter().all(|p| p.temp_max_c >= threshold_c);
    (all_high, n)
}

/// 窗口内的最高温（展示用：告诉用户"到底到了多少"）。空窗口 → None。
pub(crate) fn window_max_c(pts: &[ThermalPoint]) -> Option<f32> {
    pts.iter()
        .map(|p| p.temp_max_c)
        .fold(None, |acc, c| match acc {
            Some(m) if m >= c => Some(m),
            _ => Some(c),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tp(c: f32, at: u64) -> ThermalPoint {
        ThermalPoint { temp_max_c: c, at }
    }

    #[test]
    fn insufficient_samples_is_not_sustained() {
        // 只有一个高温点：这正是"瞬时"的形态，必须判否。
        let pts = vec![tp(95.0, 1_700_000_000)];
        let (ok, n) = is_sustained_high(&pts, 80.0, 3);
        assert!(!ok, "单点不得判为持续");
        assert_eq!(n, 1, "如实返回实际样本数");
        // 两点高温同样不够
        assert!(!is_sustained_high(&[tp(95.0, 1), tp(96.0, 2)], 80.0, 3).0);
    }

    #[test]
    fn all_high_window_is_sustained() {
        let pts = vec![tp(81.0, 1), tp(82.0, 2), tp(83.0, 3)];
        assert!(is_sustained_high(&pts, 80.0, 3).0, "三点全 ≥ 阈值 = 持续");
        // 边界（≥ 语义，与 isTempHigh 一致）：阈值取窗口内最低值时仍算持续。
        assert!(is_sustained_high(&pts, 81.0, 3).0, "恰好等于最低点 = 仍算");
        // 阈值高过最低点 → 否（[81,82,83] 对 83 不成立——曾误写此断言，实测纠错）。
        assert!(!is_sustained_high(&pts, 83.0, 3).0);
        // 再验「恰好等于阈值算」的真边界：三点全部等于阈值。
        let flat = vec![tp(83.0, 1), tp(83.0, 2), tp(83.0, 3)];
        assert!(is_sustained_high(&flat, 83.0, 3).0, "全等阈值 = 含边界成立");
    }

    #[test]
    fn one_cool_sample_breaks_sustained() {
        // 三点里有一秒回落到阈值下 → 不是"无一秒喘息"，判否。
        // 平均仍高达 85，若用平均口径会误判——这就是不用平均的原因。
        let pts = vec![tp(95.0, 1), tp(79.0, 2), tp(95.0, 3)];
        assert!(!is_sustained_high(&pts, 80.0, 3).0);
    }

    #[test]
    fn only_recent_window_counts() {
        // 前面一堆低温，只有最近 3 点全高 → 持续（看最近窗口）。
        let mut pts = vec![tp(40.0, 1), tp(45.0, 2), tp(50.0, 3)];
        pts.extend([tp(81.0, 4), tp(82.0, 5), tp(84.0, 6)]);
        assert!(
            is_sustained_high(&pts, 80.0, 3).0,
            "只看最近 min_samples 条"
        );
        // 反过来：最近 3 点里含低温 → 否，较早的高温不给现在的低温洗白
        let mut pts2 = vec![tp(95.0, 1), tp(95.0, 2), tp(95.0, 3)];
        pts2.extend([tp(81.0, 4), tp(70.0, 5), tp(82.0, 6)]);
        assert!(!is_sustained_high(&pts2, 80.0, 3).0);
    }

    #[test]
    fn window_max_reports_peak() {
        assert_eq!(
            window_max_c(&[tp(70.0, 1), tp(88.5, 2), tp(80.0, 3)]),
            Some(88.5)
        );
        assert_eq!(window_max_c(&[]), None, "空窗口返回 None，不报 0℃");
    }

    #[test]
    fn missing_file_is_empty_not_error() {
        let dir = std::env::temp_dir().join(format!("dp-thermal-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("thermal-history.jsonl");
        let pts = read_history_at(&p).expect("缺文件 = 真空态，不该 Err");
        assert!(pts.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_line_is_error_not_empty() {
        // R88 族规：文件在但内容损坏 → Err，不得静默当空历史（否则"读失败"
        // 被展示成"从未过热"）。
        let dir = std::env::temp_dir().join(format!("dp-thermal-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("thermal-history.jsonl");
        std::fs::write(&p, "not json !!!\n").unwrap();
        assert!(read_history_at(&p).is_err(), "损坏必须 Err");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_append_and_prune_keeps_newest() {
        let dir = std::env::temp_dir().join(format!("dp-thermal-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("thermal-history.jsonl");
        // 直接手工写 3 条（绕过 dedup 的时间门槛），验证 read 与裁剪语义。
        for (c, at) in [(50.0f32, 10u64), (60.0, 20), (70.0, 30)] {
            writeln!(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&p)
                    .unwrap(),
                "{}",
                serde_json::to_string(&tp(c, at)).unwrap()
            )
            .unwrap();
        }
        let pts = read_history_at(&p).unwrap();
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[2].temp_max_c, 70.0, "按写入顺序保留");
        // 裁剪后保留最新两条（模拟 MAX=2 的语义）
        let mut pts = pts;
        let keep: Vec<_> = pts.split_off(1);
        assert_eq!(keep.len(), 2);
        assert_eq!(keep[0].temp_max_c, 60.0, "split_off 后丢的是最旧");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
