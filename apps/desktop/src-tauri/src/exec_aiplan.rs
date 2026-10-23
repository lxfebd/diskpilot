//! AI 清理执行（从 executor.rs 拆分，2026-10-12）。
//! execute_ai_plan：确认窗后带 user_confirmed 调用，强制回收站 + cleanup.execute 权限。

use std::path::{Path, PathBuf};

use diskpilot_executor::{execute, Plan, UndoEntry};
use tauri::State;

use crate::AppState;

/// AI 清理执行入口。两个用途，安全边界不同：
///
/// 1. **预演（dry_run=true，无需确认）**——AI 自己发起：逐路径实测存在性与
///    字节，让它在建议前就知道"这条路径真的命中了吗、能清多少"，而不是把
///    编造/不存在的路径当成事实交给用户。预演是**纯只读**：`execute()` 的
///    dry-run 分支只实测字节、不回收、**不写 undo.jsonl**（否则"最近清理"台账
///    会塞满无法撤销的幽灵条目），所以这里对 `dry_run` 放行、不要求确认。
/// 2. **真执行（dry_run=false）**——**AI 自己调不到**：只有前端确认窗在用户
///    逐项勾选并点「确认执行」后才会带 `user_confirmed = true` 调用它。没有
///    确认标记一律拒绝，防止绕过确认窗。
///
/// 两种路径都沿用 `execute()` 里的 `protected_path()` 危险路径黑名单；真执行
/// 一律强制回收站（AI 永不永久删除）。
#[tauri::command]
pub(crate) async fn execute_ai_plan(
    state: State<'_, AppState>,
    paths: Vec<String>,
    user_confirmed: bool,
    dry_run: bool,
) -> Result<Vec<UndoEntry>, String> {
    // 放行条件按 dry_run 分两种口径：
    // - 预演（dry_run）：纯只读实测，AI 自己发起，不要求确认（下方 execute 的
    //   dry-run 分支只测字节、不回收、不写 undo 台账）；
    // - 真执行：唯一放行条件是用户已在确认窗显式确认。没有确认标记绝不动手。
    if !dry_run && !user_confirmed {
        return Err(
            "拒绝执行：缺少用户确认标记（user_confirmed）。清理必须经确认窗逐项确认。".into(),
        );
    }
    // 纵深防御：后端也校验 cleanup.execute 权限快照（前端 sync_perms 同步）。
    // 即使绕过前端直接 invoke 本命令，未在权限中心开启「执行清理计划」也会被拒。
    // **预演免此门**：dry-run 是纯只读实测（不回收、不写台账），与"执行清理"
    // 不同级；若预演也要开 L2 权限，AI 连"核实路径是否命中"都做不到，反而
    // 会更依赖猜测——那是比放行只读更大的风险。
    if !dry_run
        && !state
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
    // 存在性校验：模型编造或已不存在的路径（如 C:\Users\Public\…）不该被
    // 当成「成功回收」写进 undo —— 真执行时不存在就整体拒绝，一条都别静默放过。
    // 预演反过来：**正是要靠它发现"这条不存在"**，所以逐条如实报 0 字节，
    // 让 AI 自己把不存在的项从清单里划掉，而不是整份预演失败。
    if !dry_run {
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
    }
    let safe_plan = Plan {
        action: diskpilot_executor::Action::Recycle,
        paths: paths.into_iter().map(PathBuf::from).collect(),
        // 预演与真执行在台账 reason 上可区分（dry-run 分支本就不落盘，这里
        // 只是让日志字段自描述）。
        reason: if dry_run { "AI 清理预演（只读）".into() } else { "AI 清理（用户已确认）".into() },
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
