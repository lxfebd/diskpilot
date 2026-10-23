//! 启动项快照历史（R292）：把启动项集合的**身份键**快照追加到
//! `app_data_dir/startup-history.jsonl`，让「启动项悄悄加了两个」第一次可判断。
//! **纯只读底座**：不写启动项、不删除、不弹通知，只存点 + 提供纯 diff。
//!
//! 为什么需要它：`list_startup_items()` 只给**当前**一份清单。没有上一份，
//! "多了两个"就无从谈起——总览今日体检里的 `startup` 数字只是计数，
//! 无法回答"和上次比变了没"。
//!
//! 关键设计决定——**不用 StartupItem.id 做 diff**：
//! `startup.rs` 的 `id` 是 `format!("{loc}-{seq}")`，`seq` 是**单次枚举的自增
//! 计数器**（注册表 HKCU/HKLM 两个键 + 启动文件夹依序累加）。这意味着删掉
//! 任意一项会让其后所有项的 id 整体错位——拿 id 做集合 diff 会产出大量
//! 虚假的「新增 / 消失」。故本模块用**内容键** `location + '\u{1f}' + name`
//! 做身份：同一位置、同一名字就是同一项，与枚举顺序无关。
//!
//! 诚实性约束（必须写进产品文案）：diff 只能说**「自上次快照后新出现」**，
//! 不能说是**谁**加的、也不能断言它有害。软件自己装的启动项与恶意软件装的
//! 在数据上长得一模一样，本模块没有能力区分，也不假装能。
//!
//! 设计约束（对齐 space_history / suggestion_history / thermal_history 族规）：
//! - **追加式 jsonl**：单行极短（只存身份键集合），追加成本可忽略；
//! - **写失败静默**：底座不阻塞任何主流程；
//! - **同源 dedup**：60s 内重复采样丢弃（调用方 tick 是 60s 粒度）；
//! - **裁剪上限**：最多保留 MAX_SNAPSHOTS 份，超长丢最旧；
//! - **只读查询区分真空态与读失败**：缺文件 = 从未快照（空 vec）；文件在但
//!   读失败 → Err（R88：读失败不得伪装成「从无变化」）。

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tauri::Manager;

/// 一份启动项快照。只存身份键集合——不存命令行（含用户数据且会随软件升级
/// 自然变化，存下来只会制造噪音），不存 enabled（启停状态变化属于另一类
/// 事件，本模块只回答「多了/少了哪些项」）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StartupSnapshot {
    /// 身份键集合，形如 `location\u{1f}name`。用 BTreeSet 保证序列化稳定
    /// （同输入同字节，测试可复现）。
    pub keys: std::collections::BTreeSet<String>,
    /// Unix 秒。
    pub at: u64,
}

/// 身份键分隔符：单元分隔符（几乎不可能出现在注册表值名/文件名里）。
const SEP: char = '\u{1f}';

/// 距上次快照不足 60s 视为同一事件。
const DEDUP_SECS: u64 = 60;
/// 最多保留的快照份数（60s 一份约可覆盖 20 天）。
const MAX_SNAPSHOTS: usize = 600;

fn history_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("startup-history.jsonl"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 由 (location, name) 生成身份键。**与枚举顺序无关**——这是不用
/// `StartupItem.id` 的全部原因（见模块注释）。
pub(crate) fn startup_key(location: &str, name: &str) -> String {
    format!("{location}{SEP}{name}")
}

/// 追加一份快照。**静默失败**（返回 bool 供测试/手动回显）。
/// 集合与上一份完全相同时跳过（返回 false）：没有变化就不落新点，
/// 否则一天能写出几百份一模一样的行。
pub(crate) fn record_startup_snapshot(
    app: &tauri::AppHandle,
    keys: &std::collections::BTreeSet<String>,
) -> bool {
    if keys.is_empty() {
        // 空集合不落：读失败/枚举失败会掏出空清单，把它当"上次是空的"会让
        // 下一次把现有启动项全报成新增——那是把故障放大成谎报。
        return false;
    }
    let Some(p) = history_path(app) else {
        return false;
    };
    if let Some(parent) = p.parent() {
        if !parent.exists() && std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    let at = now_secs();
    // 倒查最后一行：dedup（时间）+ 去重（内容相同）。
    if let Ok(f) = std::fs::File::open(&p) {
        let mut last: Option<StartupSnapshot> = None;
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            if let Ok(sn) = serde_json::from_str::<StartupSnapshot>(&line) {
                last = Some(sn);
            }
        }
        if let Some(l) = last {
            if at.saturating_sub(l.at) < DEDUP_SECS {
                return false;
            }
            if &l.keys == keys {
                return false;
            }
        }
    }
    let line = match serde_json::to_string(&StartupSnapshot { keys: keys.clone(), at }) {
        Ok(l) => l,
        Err(_) => return false,
    };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&p) {
        if writeln!(f, "{line}").is_err() {
            return false;
        }
    } else {
        return false;
    }
    prune_snapshots(&p);
    true
}

/// 裁剪到最近 MAX_SNAPSHOTS 份。失败静默（多留几份旧的只影响 diff 的历史深度）。
fn prune_snapshots(p: &Path) {
    let Ok(f) = std::fs::File::open(p) else {
        return;
    };
    let reader = BufReader::new(f);
    let mut valid: Vec<StartupSnapshot> = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        if let Ok(sn) = serde_json::from_str::<StartupSnapshot>(&line) {
            valid.push(sn);
        }
    }
    if valid.len() <= MAX_SNAPSHOTS {
        return;
    }
    let keep: Vec<StartupSnapshot> = valid.split_off(valid.len() - MAX_SNAPSHOTS);
    let tmp = p.with_extension("jsonl.tmp");
    let Ok(mut out) = OpenOptions::new().create(true).write(true).truncate(true).open(&tmp) else {
        return;
    };
    for sn in &keep {
        let Ok(l) = serde_json::to_string(sn) else {
            continue;
        };
        if writeln!(out, "{l}").is_err() {
            return;
        }
    }
    drop(out);
    let _ = std::fs::rename(&tmp, p);
}

/// 读全部快照（按写入顺序）。
/// 缺文件 = 从未快照（真空态）→ 空 vec；文件在但读失败 → Err（R88）。
pub(crate) fn read_history_at(p: &Path) -> Result<Vec<StartupSnapshot>, String> {
    if !p.exists() {
        return Ok(Vec::new());
    }
    let f = std::fs::File::open(p).map_err(|e| format!("打开启动项历史失败: {e}"))?;
    let reader = BufReader::new(f);
    let mut out: Vec<StartupSnapshot> = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let sn: StartupSnapshot =
            serde_json::from_str(line).map_err(|e| format!("启动项历史存在损坏行: {e}"))?;
        out.push(sn);
    }
    Ok(out)
}

/// 读应用的历史。
pub(crate) fn read_history(app: &tauri::AppHandle) -> Result<Vec<StartupSnapshot>, String> {
    let Some(p) = history_path(app) else {
        return Ok(Vec::new());
    };
    read_history_at(&p)
}

/// 与上一份快照的差异。返回 None 的情形必须是**可解释的**，不许伪装成"没变化"：
/// - 没有任何历史 → None（首次，无从比起）；`reason = "no_history"`
/// - 只有一份 → None（同上，至少两份才谈得上 diff）；`reason = "no_history"`
/// - 读失败 → 由调用方处理（本函数只拿已读到的 vec）
///
/// 返回的 added/gone 是身份键集合。**只能说「自上次快照后新出现/消失」，
/// 不能说是谁做的**（见模块注释的诚实性约束）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StartupDelta {
    pub added: Vec<String>,
    pub gone: Vec<String>,
    /// 距上一份快照的天数（向上取整；0 = 同一天内）。
    pub gap_days: u64,
    /// 上一份快照的时间（unix 秒），供 UI 说"相对于 X 时候的清单"。
    pub prev_at: u64,
}

pub(crate) fn diff_snapshots(
    snaps: &[StartupSnapshot],
) -> Result<StartupDelta, String> {
    match snaps.len() {
        0 | 1 => Err("no_history".into()),
        n => {
            let prev = &snaps[n - 2];
            let cur = &snaps[n - 1];
            let added: Vec<String> =
                cur.keys.difference(&prev.keys).cloned().collect();
            let gone: Vec<String> = prev.keys.difference(&cur.keys).cloned().collect();
            let span = cur.at.saturating_sub(prev.at);
            // added/gone 均为空 = 没变化。这不是错误，调用方据此说"无变化"。
            // gap_days 用**向下**取整：跨度 1 秒也该是 0 天（「同一天内」），
            // div_ceil 会把任何非零跨度抬成 1 天，于是「上一次快照就在一分钟前」
            // 会被说成「距上次 1 天」——把间隔夸大一倍以上的误导。
            Ok(StartupDelta {
                added,
                gone,
                gap_days: span / 86400,
                prev_at: prev.at,
            })
        }
    }
}

/// 便捷入口：读 + diff。返回 Err 的两种情形对调用方语义不同：
/// - `"no_history"` → 真空态，UI 说"首次快照，暂无对比"；
/// - 其他字符串 → 真实读失败，UI 说"未读到"（不得显示成"无变化"）。
pub(crate) fn diff_with_history(app: &tauri::AppHandle) -> Result<StartupDelta, String> {
    let snaps = read_history(app)?;
    diff_snapshots(&snaps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sn(keys: &[impl AsRef<str>], at: u64) -> StartupSnapshot {
        StartupSnapshot {
            keys: keys.iter().map(|s| s.as_ref().to_string()).collect(),
            at,
        }
    }

    #[test]
    fn key_is_content_based_not_sequence_based() {
        // 本模块存在的理由：同一 (location, name) 永远同一键，与枚举顺序无关。
        assert_eq!(startup_key("registry_hkcu", "OneDrive"), "registry_hkcu\u{1f}OneDrive");
        assert_eq!(
            startup_key("registry_hkcu", "OneDrive"),
            startup_key("registry_hkcu", "OneDrive")
        );
        // 不同位置同名 → 不同键（HKCU 与 HKLM 各装一份是两项）
        assert_ne!(
            startup_key("registry_hkcu", "Foo"),
            startup_key("registry_hklm", "Foo")
        );
        // 同位置不同名 → 不同键
        assert_ne!(
            startup_key("registry_hkcu", "Foo"),
            startup_key("registry_hkcu", "Bar")
        );
    }

    #[test]
    fn sequence_shift_must_not_produce_false_added_or_gone() {
        // 模拟「删掉中间一项后 seq 整体错位」：若用 startup.rs 的 id 做 diff，
        // 会看到 2 项消失 + 2 项新增。内容键则如实只报 1 项消失。
        let before = [
            startup_key("registry_hkcu", "A"),
            startup_key("registry_hkcu", "B"),
            startup_key("registry_hkcu", "C"),
        ];
        let after = [startup_key("registry_hkcu", "A"), startup_key("registry_hkcu", "C")];
        let snaps = vec![sn(&before, 1_700_000_000), sn(&after, 1_700_086_400)];
        let d = diff_snapshots(&snaps).expect("两份应能 diff");
        assert!(d.added.is_empty(), "删一项不得报出任何新增");
        assert_eq!(d.gone.len(), 1, "只该报消失的那一项");
        assert_eq!(d.gone[0], startup_key("registry_hkcu", "B"));
    }

    #[test]
    fn no_history_is_named_error_not_empty_delta() {
        // 空/单份都必须是可命名的错误，而不是"added/gone 全空"——后者会被
        // UI 说成"无变化"，把"没数据"演成"没变化"是谎。
        assert_eq!(diff_snapshots(&[]).unwrap_err(), "no_history");
        let one = vec![sn(&["registry_hkcu\u{1f}A"], 1_700_000_000)];
        assert_eq!(diff_snapshots(&one).unwrap_err(), "no_history");
    }

    #[test]
    fn added_and_gone_are_both_reported() {
        let prev = [startup_key("registry_hkcu", "Old"), startup_key("registry_hkcu", "Keep")];
        let cur = [startup_key("registry_hkcu", "Keep"), startup_key("registry_hkcu", "New")];
        let snaps = vec![sn(&prev, 1_700_000_000), sn(&cur, 1_700_086_400)];
        let d = diff_snapshots(&snaps).unwrap();
        assert_eq!(d.added, vec![startup_key("registry_hkcu", "New")]);
        assert_eq!(d.gone, vec![startup_key("registry_hkcu", "Old")]);
        assert_eq!(d.gap_days, 1);
    }

    #[test]
    fn identical_sets_give_empty_delta_not_error() {
        let k = [startup_key("registry_hkcu", "A"), startup_key("registry_hkcu", "B")];
        let snaps = vec![sn(&k, 1_700_000_000), sn(&k, 1_700_086_400)];
        let d = diff_snapshots(&snaps).expect("两份相同集合仍应给出 diff");
        assert!(d.added.is_empty() && d.gone.is_empty(), "没变化 = 两侧皆空");
    }

    #[test]
    fn gap_days_rounds_up_and_zero_for_same_day() {
        let k = [startup_key("registry_hkcu", "A")];
        // 同一秒内两份 → 0 天
        let same = vec![sn(&k, 1_700_000_000), sn(&k, 1_700_000_000)];
        assert_eq!(diff_snapshots(&same).unwrap().gap_days, 0);
        // 1 秒后 → 仍 0 天（ ceil ）
        let soon = vec![sn(&k, 1_700_000_000), sn(&k, 1_700_000_001)];
        assert_eq!(diff_snapshots(&soon).unwrap().gap_days, 0);
        // 3 天后
        let later = vec![sn(&k, 1_700_000_000), sn(&k, 1_700_000_000 + 3 * 86400)];
        assert_eq!(diff_snapshots(&later).unwrap().gap_days, 3);
    }

    #[test]
    fn missing_file_is_empty_not_error() {
        let dir = std::env::temp_dir().join(format!("dp-startup-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("startup-history.jsonl");
        let v = read_history_at(&p).expect("缺文件 = 真空态，不该 Err");
        assert!(v.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_line_is_error_not_empty() {
        // R88 族规：文件在但损坏 → Err（否则 UI 把读失败显示成"无变化"）。
        let dir = std::env::temp_dir().join(format!("dp-startup-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("startup-history.jsonl");
        std::fs::write(&p, "not json !!!\n").unwrap();
        assert!(read_history_at(&p).is_err(), "损坏必须 Err");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
