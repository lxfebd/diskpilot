//! USN 增量游标持久化（R481）：把每次扫描后的基线游标（journal_id + next_usn）
//! 落到 `app_data_dir/usn-cursors.json`，让进程重启后增量扫描仍有起点——此前游标只
//! 活在 AppState 内存里（lib.rs），退出即丢，重启后第一次扫描必然全量。
//!
//! 诚实性（R88 口径）：启动加载时对每条游标查卷的 live journal，`journal_id` 不一致
//! 或 `next_usn` 已滑出保留窗（journal 重置/回绕）就丢弃该游标——丢弃意味着下次对该盘
//! 诚实地全量扫描，绝不拿失效游标硬做增量再漏文件。查不了 journal（非 NTFS 等）同样丢弃。
//!
//! 族规：单文档 state 文件（游标是可变状态而非历史，不适合追加式 jsonl），tmp+rename
//! 原子写；记录带 `"v": 1`；最多 MAX_CURSORS 条，超出淘汰落盘最旧的；写路径静默失败，
//! 不影响扫描主流程。整个模块只在 Windows 编译（lib.rs 用 `#[cfg(windows)] mod` 挂载）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use diskpilot_scanner::usn::{JournalState, UsnCursor};

/// state 文件里一条游标：比 UsnCursor 多一个落盘时刻 `at`（供淘汰与审计）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredCursor {
    at: u64,
    journal_id: u64,
    next_usn: i64,
}

/// usn-cursors.json 的文档结构。key = 归一化扫描根（与 AppState.usn_cursors 同口径，
/// 形如 `c:/`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CursorStore {
    v: u32,
    cursors: HashMap<String, StoredCursor>,
}

/// 最多保留的游标条数（每盘一条，32 覆盖所有盘符还有余量）。
const MAX_CURSORS: usize = 32;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn store_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("usn-cursors.json"))
}

/// 归一化 key（如 `c:/`）里取盘符；取不到返回 None（调用方当作查不了 journal 处理）。
fn drive_letter_of_key(key: &str) -> Option<char> {
    let c = key.chars().next()?;
    c.is_ascii_alphabetic().then_some(c.to_ascii_uppercase())
}

/// 纯函数：live 为 None（查不了 journal）或游标已不在 journal 保留窗内 → None（淘汰，
/// 下次诚实全量扫描）；否则 Some(游标)。边界：next_usn == lowest_valid_usn / == next_usn
/// 都算有效（与 JournalState::cursor_still_valid 一致）。
fn keep_if_valid(live: Option<&JournalState>, stored: &StoredCursor) -> Option<UsnCursor> {
    let st = live?;
    let c = UsnCursor {
        journal_id: stored.journal_id,
        next_usn: stored.next_usn,
    };
    st.cursor_still_valid(&c).then_some(c)
}

/// 超出 MAX_CURSORS 时淘汰 at 最旧的条目。
fn evict(store: &mut CursorStore) {
    while store.cursors.len() > MAX_CURSORS {
        let Some(oldest) = store
            .cursors
            .iter()
            .min_by_key(|(_, c)| c.at)
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        store.cursors.remove(&oldest);
    }
}

fn insert_stored(store: &mut CursorStore, root: &str, stored: StoredCursor) {
    store.v = 1;
    store.cursors.insert(root.to_string(), stored);
    evict(store);
}

/// 写路径：读旧 state（缺失/损坏都从空开始，损坏打 warn 不整体失败）、插入新游标、
/// 淘汰、原子整写。静默失败返回 false，不影响扫描主流程。
fn record_cursor_at(p: &Path, root: &str, c: &UsnCursor) -> bool {
    let mut store = match crate::obs::read_state::<CursorStore>(p) {
        Ok(v) => v.unwrap_or_default(),
        Err(e) => {
            tracing::warn!("usn-cursors.json 读取失败，从空状态重建: {e}");
            CursorStore::default()
        }
    };
    insert_stored(
        &mut store,
        root,
        StoredCursor {
            at: now_secs(),
            journal_id: c.journal_id,
            next_usn: c.next_usn,
        },
    );
    crate::obs::write_state(p, &store)
}

/// commit_scan 落点：把新游标写进 state 文件。静默失败。
pub(crate) fn record_cursor(app: &AppHandle, root: &str, c: &UsnCursor) -> bool {
    match store_path(app) {
        Some(p) => record_cursor_at(&p, root, c),
        None => false,
    }
}

/// 启动加载：读 state，逐条用 live journal 校验，只保留仍有效的游标。
/// `query` 是每盘符查 journal 的函数（生产实现 = usn::win::query_journal，测试可注入假实现）。
/// 缺文件 = 真空态空 map；文件损坏 = 打 warn 按空 map 处理（全部诚实全量）。
fn load_validated_at(
    p: &Path,
    query: impl Fn(char) -> Option<JournalState>,
) -> HashMap<String, UsnCursor> {
    let mut out = HashMap::new();
    let store = match crate::obs::read_state::<CursorStore>(p) {
        Ok(Some(s)) => s,
        Ok(None) => return out,
        Err(e) => {
            tracing::warn!("usn-cursors.json 损坏，全部游标丢弃（全量扫描）: {e}");
            return out;
        }
    };
    for (root, stored) in &store.cursors {
        let live = drive_letter_of_key(root).and_then(&query);
        match keep_if_valid(live.as_ref(), stored) {
            Some(c) => {
                out.insert(root.clone(), c);
            }
            None => {
                tracing::info!(
                    "usn cursor for {root} dropped (journal unavailable/reset/rolled over); full scan will run"
                );
            }
        }
    }
    out
}

/// 生产入口：启动时把仍有效的游标装进 AppState.usn_cursors。
pub(crate) fn load_validated_cursors(app: &AppHandle) -> HashMap<String, UsnCursor> {
    let Some(p) = store_path(app) else {
        return HashMap::new();
    };
    load_validated_at(&p, |letter| {
        diskpilot_scanner::usn::win::query_journal(letter).ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(journal_id: u64, next_usn: i64, lowest_valid_usn: i64) -> JournalState {
        JournalState {
            journal_id,
            first_usn: 0,
            next_usn,
            lowest_valid_usn,
        }
    }

    #[test]
    fn keep_if_valid_window() {
        let st = live(1, 1000, 100);
        let s = |usn: i64| StoredCursor {
            at: 0,
            journal_id: 1,
            next_usn: usn,
        };
        // 边界：== lowest_valid_usn 与 == next_usn 都算有效。
        assert!(keep_if_valid(Some(&st), &s(100)).is_some());
        assert!(keep_if_valid(Some(&st), &s(1000)).is_some());
        // 已滑出保留窗 / 超过当前 next_usn / journal_id 重置 / 查不了 journal → 淘汰。
        assert!(keep_if_valid(Some(&st), &s(99)).is_none());
        assert!(keep_if_valid(Some(&st), &s(1001)).is_none());
        assert!(keep_if_valid(
            Some(&st),
            &StoredCursor {
                at: 0,
                journal_id: 2,
                next_usn: 500
            }
        )
        .is_none());
        assert!(keep_if_valid(None, &s(500)).is_none());
    }

    #[test]
    fn evict_drops_oldest_beyond_cap() {
        let mut store = CursorStore::default();
        for i in 0..(MAX_CURSORS as u64 + 5) {
            insert_stored(
                &mut store,
                &format!("r{i:02}/"),
                StoredCursor {
                    at: i,
                    journal_id: 1,
                    next_usn: i as i64,
                },
            );
        }
        assert_eq!(store.cursors.len(), MAX_CURSORS);
        assert_eq!(store.v, 1);
        // 保留的是 at 最新的 32 条（5..=36），最旧的 5 条被淘汰。
        let min_at = store.cursors.values().map(|c| c.at).min().unwrap();
        assert_eq!(min_at, 5);
    }

    #[test]
    fn record_then_load_validates_against_journal() {
        let tdir = std::env::temp_dir().join(format!("dp-usncur-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tdir);
        std::fs::create_dir_all(&tdir).unwrap();
        let p = tdir.join("usn-cursors.json");

        assert!(record_cursor_at(
            &p,
            "c:/",
            &UsnCursor {
                journal_id: 7,
                next_usn: 100
            }
        ));
        assert!(record_cursor_at(
            &p,
            "d:/",
            &UsnCursor {
                journal_id: 8,
                next_usn: 200
            }
        ));

        // live journal：c: 还是 id 7 且保留窗 [50,500] → 保留；d: journal id 变成 9（重置）→ 淘汰。
        let query = |letter: char| match letter {
            'C' => Some(live(7, 500, 50)),
            'D' => Some(live(9, 999, 0)),
            _ => None,
        };
        let got = load_validated_at(&p, query);
        assert_eq!(got.len(), 1);
        assert_eq!(got.get("c:/").unwrap().next_usn, 100);
        assert_eq!(got.get("c:/").unwrap().journal_id, 7);

        // 查不了 journal → 全部淘汰（诚实全量扫描）。
        assert!(load_validated_at(&p, |_| None).is_empty());

        // 缺文件 = 真空态。
        assert!(load_validated_at(&tdir.join("nope.json"), |_| None).is_empty());

        // state 文件损坏 → 打 warn 按空处理，不炸。
        std::fs::write(&p, "not json{{{{").unwrap();
        assert!(load_validated_at(&p, query).is_empty());

        let _ = std::fs::remove_dir_all(&tdir);
    }
}
