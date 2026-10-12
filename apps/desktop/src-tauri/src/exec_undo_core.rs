//! 撤销核心（从 executor.rs 拆分，2026-10-12）。
//! undo 命令 + undo_core 纯逻辑（防 index 漂移 + Quarantine 恢复）。

use diskpilot_executor::{
    list_undo as list_undo_log, remove_restored, restore_quarantined, UndoEntry,
};
use tauri::State;

use crate::AppState;

/// Restores a quarantined item back to its original path. Only `Quarantine`
/// records are restorable; recycled/delete entries must be restored from the
/// OS recycle bin or are unrecoverable.
///
/// `index` 是展示列表里「最新在前」的位置；`source` 是用户当时点的那一行的
/// 原始路径。两者都校验：列表可能因其他操作在两次刷新之间变化导致 index
/// 漂移（恢复错条目），必须确认 `entries[index].source == source` 才动手。
#[tauri::command]
pub(crate) fn undo(
    state: State<'_, AppState>,
    index: usize,
    source: String,
) -> Result<Option<UndoEntry>, String> {
    undo_core(&state.undo_log, &state.quarantine_root, index, source)
}

/// `undo` 的纯逻辑：可单测（不依赖 Tauri State）。
pub(crate) fn undo_core(
    undo_log: &std::path::Path,
    quarantine_root: &std::path::Path,
    index: usize,
    source: String,
) -> Result<Option<UndoEntry>, String> {
    let entries = list_undo_log(undo_log).map_err(|e| e.to_string())?;
    let entry = entries
        .get(index)
        .cloned()
        .ok_or_else(|| format!("undo index out of range: {index}"))?;
    // 防 index 漂移：条目已不是用户点的那一行则拒绝（重新刷新后再试）。
    if entry.source.to_string_lossy() != source {
        return Err("撤销目标已变化（列表可能被其他清理操作刷新），请刷新后再试。".into());
    }
    if !matches!(entry.action, diskpilot_executor::Action::Quarantine) {
        return Ok(None);
    }
    let restored_source =
        restore_quarantined(&entry, quarantine_root).map_err(|e| format!("undo failed: {e}"))?;
    remove_restored(undo_log, std::slice::from_ref(&restored_source)).map_err(|e| e.to_string())?;
    Ok(Some(entry))
}
