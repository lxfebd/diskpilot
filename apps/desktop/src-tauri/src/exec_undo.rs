//! 撤销列表 + 清理预检命令（从 executor.rs 拆分，2026-10-12）。
//! list_undo / PreflightReport / cleanup_preflight。

use diskpilot_executor::{list_undo as list_undo_log, UndoEntry};
use tauri::State;

use super::exec_preflight::{preflight_recommended, running_process_names, running_required};
use crate::AppState;

/// Lists every recorded cleanup action, newest first. Frontend shows these
/// as a "最近清理" panel; quarantine entries carry a restore action.
#[tauri::command]
pub(crate) fn list_undo(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<UndoEntry>, String> {
    let entries = list_undo_log(&state.undo_log).map_err(|e| e.to_string())?;
    Ok(entries.into_iter().take(limit.unwrap_or(100)).collect())
}

/// P2b 清理预检（只读）：清理页进入 / 执行前调用。返回
/// ① `running_processes` —— TOML `required_stopped_processes` 里当前还在跑的
///    进程名（提示用户先退出；这里绝不杀进程，杀进程是独立写路径）；
/// ② `recommended_scope_ids` —— `recommended_selected = true` 的 scope，供前端
///    做预选建议（用户仍可逐项取消，不改变「先出清单 → 确认」铁律）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PreflightReport {
    pub scaffold_id: String,
    pub running_processes: Vec<String>,
    pub recommended_scope_ids: Vec<String>,
}

#[tauri::command]
pub(crate) fn cleanup_preflight(
    state: State<'_, AppState>,
    scaffold_id: String,
) -> Result<PreflightReport, String> {
    let scaffolds = state.scaffolds.lock().unwrap();
    let scaffold = scaffolds
        .iter()
        .find(|s| s.id == scaffold_id)
        .ok_or_else(|| format!("unknown scaffold: {scaffold_id}"))?;
    let wanted: Vec<String> = scaffold
        .scopes
        .iter()
        .filter_map(|sc| sc.required_stopped_processes.as_ref())
        .flatten()
        .cloned()
        .collect();
    let snapshot = running_process_names();
    Ok(PreflightReport {
        running_processes: running_required(&wanted, &snapshot),
        recommended_scope_ids: preflight_recommended(scaffold),
        scaffold_id,
    })
}
