//! glob 锚点推导 + 回收命令（从 executor.rs 拆分，2026-10-12）。
//! expand_first_alternation / scope_glob_roots / recycle_paths。

use std::path::PathBuf;

use diskpilot_executor::{execute, Plan, UndoEntry};
use tauri::State;

use super::confirm_gate;
use crate::AppState;
use diskpilot_scaffold::expand_env;

/// 展开 glob 里第一个 `{A,B,...}` 替代组为多个候选分支；无替代组时原样。
/// 只展开第一个（现有 scaffold 的 glob 至多一个替代组）。
fn expand_first_alternation(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(close_rel) = pattern[open..].find('}') else {
        return vec![pattern.to_string()];
    };
    let close = open + close_rel;
    let inner = &pattern[open + 1..close];
    inner
        .split(',')
        .map(|br| format!("{}{}{}", &pattern[..open], br, &pattern[close + 1..]))
        .collect()
}

/// 从 scope glob 推导「遍历起点」集合。盘根被 protected_path 拦下时调用：
/// 目标是找一个（或几个）真实存在的目录作 walk 起点，glob 仍绝对路径锚定，
/// 起点只影响性能不影响匹配范围（executor 最终按 matched 精确删）。
/// brace 替代（`{A,B}`）展开为多个分支，**每个分支的锚点都收集**——两个
/// 替代分支常是并行的独立目录（`AppData/Local/{npm-cache,pnpm-cache}`），
/// 只取一个会漏掉另一个（npm-cache 空、pnpm-cache 占 706MB 时只回退到
/// npm-cache → 0 匹配，「建议可清、执行 0b」脱节）。分支是同一父目录下的
/// 兄弟时取公共祖先，一个起点即可覆盖全部分支（见 glob_anchor 注释）。
/// 收集不到任何真实目录时回退 scaffold.detect 里第一个存在的目录。
pub(crate) fn scope_glob_roots(pattern: &str, detect: &[String]) -> Vec<PathBuf> {
    let branches = expand_first_alternation(pattern);
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for branch in &branches {
        let mut anchor: Option<PathBuf> = None;
        // 1. 静态前缀：`%USERPROFILE%/.cargo/registry/**` → pop 到存在目录
        let first_wild = branch
            .char_indices()
            .find(|(_, c)| matches!(c, '*' | '?' | '[' | '{'))
            .map(|(i, _)| i)
            .unwrap_or(branch.len());
        let prefix = branch[..first_wild].trim_end_matches('/');
        if !prefix.is_empty() {
            let mut cur = PathBuf::from(prefix);
            while !cur.as_os_str().is_empty() {
                if cur.is_dir() && diskpilot_executor::protected_path(&cur).is_none() {
                    anchor = Some(cur);
                    break;
                }
                if !cur.pop() {
                    break;
                }
            }
        }
        // 2. 裸 `**` 前缀：取尾部非通配锚段在 detect 里找以它结尾的存在目录
        if anchor.is_none() {
            let tail = branch
                .trim_start_matches("**/")
                .trim_start_matches("*/")
                .split('/')
                .filter(|s| !s.is_empty() && !s.contains(['*', '?', '[', '{', '}']))
                .collect::<Vec<_>>();
            if !tail.is_empty() {
                let anchor_s = tail.join("/");
                anchor = detect
                    .iter()
                    .map(|d| PathBuf::from(expand_env(d)))
                    .filter(|d| {
                        let s = d.to_string_lossy().replace('\\', "/");
                        s.ends_with(&anchor_s) && d.is_dir()
                    })
                    .max_by_key(|d| d.as_os_str().len());
            }
        }
        if let Some(a) = anchor {
            if seen.insert(a.clone()) {
                roots.push(a);
            }
        }
    }
    if roots.is_empty() {
        // 3. 旧行为：detect 第一条存在的目录
        if let Some(d) = detect
            .iter()
            .map(|d| PathBuf::from(expand_env(d)))
            .find(|d| d.is_dir())
        {
            roots.push(d);
        }
    }
    roots
}

/// 文件树右键 / 聊天建议卡「回收此项」的执行入口：把用户点选的显式路径
/// 移入回收站。动作在这里**写死 Recycle**——前端不再能提交 action=delete 的
/// 任意计划（旧 `execute_plan` 收一个完整的 Plan，等于把「永久删除」这个
/// 选项开放给任何能 invoke 命令的一侧）。
///
/// 与 `execute_scope` 同属人驱动流程，只要求 `confirmed = true`（前端两步确认
/// 弹窗通过后才带）；`execute()` 内部仍逐个校验 `protected_path`。
#[tauri::command]
pub(crate) async fn recycle_paths(
    state: State<'_, AppState>,
    paths: Vec<String>,
    reason: String,
    confirmed: Option<bool>,
) -> Result<Vec<UndoEntry>, String> {
    // 这个命令没有预览分支，永远真实落盘 → dry_run 传 false。
    confirm_gate(false, confirmed).map_err(|e| format!("recycle:confirm: {e}"))?;
    // R505：真实回收同样要求后端权限 cleanup.execute（与 execute_scope 同口径）。
    if !state
        .perm_grants
        .lock()
        .unwrap()
        .contains("cleanup.execute")
    {
        return Err(
            "recycle:denied: 未开启「执行清理」权限（cleanup.execute）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    if paths.is_empty() {
        return Err("recycle: 路径清单为空，拒绝执行。".into());
    }
    let plan = Plan {
        action: diskpilot_executor::Action::Recycle,
        paths: paths.into_iter().map(PathBuf::from).collect(),
        reason,
    };
    let invalidate_roots: Vec<PathBuf> = plan.paths.clone();
    let undo_log = state.undo_log.clone();
    let quarantine_root = state.quarantine_root.clone();
    let out = tokio::task::spawn_blocking(move || {
        execute(&plan, false, &undo_log, &quarantine_root).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    crate::cleanup_logic::remove_cache_entries(&state, &invalidate_roots);
    Ok(out)
}

/// 打开系统回收站：给台账里的「回收」条目一个明确的还原出口。
///
/// 为什么要单独一条命令，而不是复用 `reveal_in_explorer`：回收站是 shell 特殊
/// 文件夹（`shell:RecycleBinFolder`），不是文件系统路径，`reveal_in_explorer`
/// 第一句 `path.exists()` 就把它拒了。
///
/// 只开窗口，**不代用户还原任何文件**——还原由用户在系统回收站里自己点，
/// 应用不猜「哪一条对应回收站里的哪个条目」（回收站里的名字与原始路径的映射
/// 只有 shell 知道）。
#[tauri::command]
pub(crate) fn open_recycle_bin() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("shell:RecycleBinFolder")
            .spawn()
            .map_err(|e| format!("open_recycle_bin: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let trash = std::env::var("HOME")
            .map(|h| PathBuf::from(h).join(".Trash"))
            .map_err(|_| "open_recycle_bin: 找不到 HOME".to_string())?;
        std::process::Command::new("open")
            .arg(&trash)
            .spawn()
            .map_err(|e| format!("open_recycle_bin: {e}"))?;
        Ok(())
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let trash = std::env::var("HOME")
            .map(|h| PathBuf::from(h).join(".local/share/Trash"))
            .map_err(|_| "open_recycle_bin: 找不到 HOME".to_string())?;
        if !trash.exists() {
            return Err(format!(
                "open_recycle_bin: 回收站目录不存在（{}）——该桌面的回收站还没被用过。",
                trash.display()
            ));
        }
        std::process::Command::new("xdg-open")
            .arg(&trash)
            .spawn()
            .map_err(|e| format!("open_recycle_bin: {e}"))?;
        Ok(())
    }
}
