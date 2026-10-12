//! USN Journal 增量扫描域（从 scan_core.rs 拆分，2026-10-12）。
//! `scan_with_usn` 回放变更日志合入缓存树。盘符辅助（drive_letter_of /
//! is_drive_root）留在父模块 scan_core.rs（MFT 快速路径共用）。

use anyhow::Context;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{Node, ScanProgress};

/// USN Journal 增量扫描入口（Windows / NTFS）。
///
/// 相比全量 walkdir/MFT 扫描，增量路径只回放 Change Journal 里的变更记录
/// （create/delete/rename/data change），再只对变更文件做一次 stat 补大小。
/// 对基本空闲的卷，重新扫描从分钟级降到亚秒级。
///
/// - `cache`：上一次全量扫描得到的树（调用方持久化）。
/// - `cursor`：上一次增量后的游标（journal_id + next_usn）。
///
/// 返回 `(新树, 新游标)`。下列情况调用方应回退到全量扫描：
///   1. 卷不是 NTFS（`query_journal` 失败 / journal 未启用）。
///   2. journal 被重置或已回绕（`cursor_still_valid()` 为 false）。
#[cfg(windows)]
pub fn scan_with_usn(
    root: &Path,
    mut cache: Node,
    cursor: &crate::usn::UsnCursor,
    cancel: Option<&AtomicBool>,
    on_progress: &(dyn Fn(&ScanProgress) + Sync),
) -> anyhow::Result<(Node, crate::usn::UsnCursor)> {
    let letter = super::drive_letter_of(root)
        .ok_or_else(|| anyhow::anyhow!("scan:usn: root 不在盘根上 (root={})", root.display()))?;
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(anyhow::anyhow!("scan:cancelled: before usn"));
    }

    // 1. 卷上 journal 的当前状态；非 NTFS / journal 未启用 → 错误（调用方回退全量）。
    let state = crate::usn::win::query_journal(letter)
        .with_context(|| format!("USN journal unavailable on {letter}:"))?;
    if !state.cursor_still_valid(cursor) {
        return Err(anyhow::anyhow!(
            "scan:usn: journal reset or rolled over (id {} vs {}, next_usn {} < lowest {}); full re-scan required",
            cursor.journal_id, state.journal_id, cursor.next_usn, state.lowest_valid_usn
        ));
    }

    // 2. 全卷 FRN→(parent,name) 表（MFT 速度枚举，用于把变更解析成绝对路径）。
    let mut frn_table = crate::usn::win::build_frn_table(letter, cancel, |seen, _| {
        on_progress(&ScanProgress {
            files_seen: seen,
            bytes_seen: 0,
            current_path: "USN 枚举 FRN 表…".into(),
        });
    })?;

    // 3. 回放 journal 自 cursor 起的变更。
    let (changes, next_usn) = crate::usn::win::read_changes(letter, cursor)
        .context("scan:usn: reading change journal")?;
    let new_cursor = crate::usn::UsnCursor {
        journal_id: state.journal_id,
        next_usn,
    };

    // 4. 解析成绝对路径（只保留在 root 子树内的）。
    let resolved = crate::usn::resolve_paths(&changes, &frn_table, root);
    let _ = &mut frn_table;

    // 5. 变更合入缓存树（delta 传播聚合，只 stat 变更文件）。
    let applied = crate::usn::merge_changes(
        &mut cache,
        &resolved,
        root,
        crate::ScanOptions::default().keep_files_per_dir,
    );

    on_progress(&ScanProgress {
        files_seen: applied as u64,
        bytes_seen: 0,
        current_path: "USN 增量合并完成".into(),
    });

    tracing::info!(
        "scan:usn: applied={applied} changes={} root={}",
        changes.len(),
        root.display()
    );
    Ok((cache, new_cursor))
}
