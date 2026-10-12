//! AI 清理执行（从 executor.rs 拆分，2026-10-12）。
//! execute_ai_plan：确认窗后带 user_confirmed 调用，强制回收站 + cleanup.execute 权限。

use std::path::{Path, PathBuf};

use diskpilot_executor::{execute, Plan, UndoEntry};
use tauri::State;

use crate::AppState;

/// AI 清理执行入口——但 **AI 自己调不到**：只有前端确认窗在用户逐项勾选并
/// 点「确认执行」后才会带 `user_confirmed = true` 调用它。后端二次校验：
/// `user_confirmed` 不为 true、或 `paths` 为空，一律拒绝执行（防止绕过确认窗）。
/// 执行动作一律强制回收站（AI 永不永久删除），并沿用 `execute()` 里的
/// `protected_path()` 危险路径黑名单。`dry_run = true` 只返回将要处理的路径。
#[tauri::command]
pub(crate) async fn execute_ai_plan(
    state: State<'_, AppState>,
    paths: Vec<String>,
    user_confirmed: bool,
    dry_run: bool,
) -> Result<Vec<UndoEntry>, String> {
    // 唯一放行条件：用户已在确认窗显式确认。没有确认标记绝不动手。
    if !user_confirmed {
        return Err(
            "拒绝执行：缺少用户确认标记（user_confirmed）。清理必须经确认窗逐项确认。".into(),
        );
    }
    // 纵深防御：后端也校验 cleanup.execute 权限快照（前端 sync_perms 同步）。
    // 即使绕过前端直接 invoke 本命令，未在权限中心开启「执行清理计划」也会被拒。
    if !state
        .perm_grants
        .lock()
        .unwrap()
        .contains("cleanup.execute")
    {
        return Err(
            "拒绝执行：未开启「执行清理计划」权限（cleanup.execute）。请先在权限中心开启。".into(),
        );
    }
    if paths.is_empty() {
        return Err("拒绝执行：没有选中任何路径。".into());
    }
    // AI 清理一律走回收站（可找回）；execute() 内部还会逐个校验 protected_path。
    // 但先做存在性校验：模型编造或已不存在的路径（如 C:\Users\Public\…）不该
    // 被当成「成功回收」写进 undo —— 不存在就整体拒绝，一条都别静默放过。
    let missing: Vec<&str> = paths
        .iter()
        .filter(|p| !Path::new(p).exists())
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "拒绝执行：{} 个路径不存在或已无法访问：{}。请核对清单后重新生成，不要引用不存在的路径。",
            missing.len(),
            missing.join("；")
        ));
    }
    let safe_plan = Plan {
        action: diskpilot_executor::Action::Recycle,
        paths: paths.into_iter().map(PathBuf::from).collect(),
        reason: "AI 清理（用户已确认）".into(),
    };
    let invalidate_roots: Vec<PathBuf> = safe_plan.paths.clone();
    let undo_log = state.undo_log.clone();
    let quarantine_root = state.quarantine_root.clone();
    let out = tokio::task::spawn_blocking(move || {
        execute(&safe_plan, dry_run, &undo_log, &quarantine_root).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?;
    // 真实执行后按路径做 cleanup_cache key 级失效（同 execute_scope 末尾）。
    if !dry_run {
        crate::cleanup_logic::remove_cache_entries(&state, &invalidate_roots);
    }
    out
}
