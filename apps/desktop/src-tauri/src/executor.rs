//! 清理执行领域：execute_scope / execute_ai_plan / undo。
//! 从 lib.rs 拆分（拆分时逐函数核对，逻辑不变）。

use std::path::{Path, PathBuf};

use diskpilot_executor::{
    execute, list_undo as list_undo_log, remove_restored, restore_quarantined, Plan, UndoEntry,
};
use diskpilot_scaffold::{expand_env, RecycleGranularity};
use diskpilot_scanner::{
    diskpilot_walker, find_matching_dirs, find_matching_dirs_on_tree, find_matching_files_on_tree,
    locate_subtree, mtime_older_than, path_passes_env, path_passes_wxid, tree_files_adjudicable,
};
use tauri::State;

use crate::AppState;

/// 人驱动写操作的确认门：`dry_run` 恒放行（预览只读，可以随便调），一旦落到
/// 磁盘就必须 `confirmed == Some(true)`。抽成函数是为了让这条铁律有测试兜底
/// （`execute_scope` / `recycle_paths` 共用；`toolbelt_recycle` 没有预览分支，
/// 单独写了同样的判断）。
///
/// 注意这里**不**校验 `cleanup.execute` 权限：那条 L2 权限是「允许 AI 自己
/// 执行清理」的钥匙（见 `execute_ai_plan`），而这几条命令是用户在看清清单后
/// 主动点的按钮。把人驱动的流程绑到默认关闭的 AI 权限上，普通用户就清不了东西。
pub(crate) fn confirm_gate(dry_run: bool, confirmed: Option<bool>) -> Result<(), &'static str> {
    if !dry_run && confirmed != Some(true) {
        return Err("必须由用户明确确认（confirmed=true）后才能真实落盘。");
    }
    Ok(())
}

/// Resolve a scaffold scope's glob to actual file paths under `root_path` and
/// run the executor on just those files. Use this instead of feeding the
/// matched root directly into the executor — the latter would delete the
/// whole folder, ignoring the scope's glob. `dry_run = true` returns what
/// would be touched without performing the action.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // every arg is a distinct user-facing knob; bundling into a struct is just shuffling
pub(crate) async fn execute_scope(
    state: State<'_, AppState>,
    scaffold_id: String,
    scope_id: String,
    root_path: String,
    dry_run: bool,
    older_than_days: Option<u32>,
    wxid_filter: Option<Vec<String>>,
    env_filter: Option<Vec<String>>,
    confirmed: Option<bool>,
) -> Result<Vec<UndoEntry>, String> {
    // 绕过前端确认窗直接 invoke 也要被拦下（铁律：清理先出清单 + 用户确认）。
    confirm_gate(dry_run, confirmed).map_err(|e| format!("execute:confirm: {e}"))?;
    let (scaffold, scope) = {
        let scaffolds = state.scaffolds.lock().unwrap();
        let sc = scaffolds
            .iter()
            .find(|s| s.id == scaffold_id)
            .cloned()
            .ok_or_else(|| format!("scaffold not found: {scaffold_id}"))?;
        let scope = sc
            .scopes
            .iter()
            .find(|s| s.id == scope_id)
            .cloned()
            .ok_or_else(|| format!("scope not found: {scaffold_id}/{scope_id}"))?;
        (sc, scope)
    };

    let requested_root = PathBuf::from(&root_path);
    // 展开 %TEMP% 等环境变量后路径分隔符是 Windows 反斜杠，而 glob 匹配目标
    // （find_matching_dirs/find_matching_files_on_tree 内部统一 `replace('\\', "/")`）
    // 是正斜杠。必须同步规范化，否则真实 scaffold 的 `%TEMP%/**` 等文件粒度 glob
    // 在执行时永远 0 匹配——「建议显示可清、执行清理 0 个文件」的真根因。
    let pattern = expand_env(&scope.glob).replace('\\', "/");
    // 遍历起点：拒绝盘根 / 系统目录作为清理根（scope 的 glob 会递归匹配
    // 整棵子树，传 `C:\` 配合 File 粒度 + Delete 模式就能全盘删文件，必须
    // 在源头拦住）。但「总览页建议清理」习惯把整盘根（如 `C:\`）当
    // root_path 传进来——那是**遍历起点**而非清理目标：现有 scope 的 glob
    // 全部绝对路径锚定，起点只影响性能不影响匹配范围。因此被
    // protected_path 拦下时，从 **scope 自己的 glob** 推导锚定根，而不是
    // 随便拿 scaffold.detect 第一条（那条可能是另一个 scope 的锚，比如
    // dev-caches 第一条 detect 是 npm-cache，拿它当 cargo/pip 的起点会
    // 永远 0 匹配）。推导顺序：
    //   1. glob 静态前缀（第一个通配符之前）：`%USERPROFILE%/.cargo/registry/**`
    //      → `C:/Users/.../.cargo/registry`；`{A,B}/**/...` 取第一个替代 A。
    //   2. `**/AppData/Local/npm-cache/**` 这类裸 `**` 前缀：取 glob 尾部锚段
    //      （`AppData/Local/npm-cache`），在 detect 里找展开后以此结尾的存在目录。
    //   3. 都没有 → 旧行为：detect 第一条存在的目录。
    // 注意：这只是「遍历起点」，glob 仍然绝对路径锚定、匹配范围不变；
    // 起点选错只会漏匹配，绝不会扩大删除面（executor 最终按 matched 精确删）。
    //
    // brace 替代（`{npm-cache,pnpm-cache}`）的每个分支都可能是独立目录，
    // 只挑一个起点会漏掉其余分支（如 npm-cache 空、pnpm-cache 706MB 时
    // 回退到 npm-cache 就 0 匹配——「建议 955MB、清理 0b」脱节的真因）。
    // 所以这里收集**所有**分支的锚点作遍历起点，匹配阶段逐根 union。
    let mut exec_roots: Vec<PathBuf> = Vec::new();
    if diskpilot_executor::protected_path(&requested_root).is_some() {
        exec_roots = scope_glob_roots(&pattern, &scaffold.detect);
    }
    if exec_roots.is_empty() {
        exec_roots.push(requested_root.clone());
    }
    if let Some(what) = diskpilot_executor::protected_path(&requested_root) {
        tracing::info!(
            "execute_scope {}/{}: root {what} `{}` refused, using derived roots {:?}",
            scaffold.id,
            scope.id,
            requested_root.display(),
            exec_roots
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
        );
    }
    let root = exec_roots[0].clone();
    if !exec_roots.iter().all(|r| r.is_dir()) {
        return Err(format!(
            "scope root does not exist or is not a directory: {}",
            exec_roots
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let glob = globset::GlobBuilder::new(&pattern)
        .literal_separator(false)
        .case_insensitive(true)
        .build()
        .map_err(|e| format!("scope `{}` has invalid glob: {e}", scope.id))?;
    let mut b = globset::GlobSetBuilder::new();
    b.add(glob);
    let set = b.build().map_err(|e| e.to_string())?;

    let wxid_filter_owned = wxid_filter;
    let env_filter_owned = env_filter;
    let granularity = scope.recycle_granularity;
    // 实际执行后按 scope 根做 cleanup_cache key 级失效（不再全量 clear）。
    let invalidate_roots: Vec<PathBuf> = exec_roots.clone();

    // P2：dry_run 预览的快速路径——目录粒度 + 不带保留期过滤时，glob 匹配
    // 直接在最近一次扫描树（AppState.scan_tree）上做，秒回，免去一次可能
    // 上百秒的全盘 walk。树上没有 mtime、文件列表受 top-K 限制，所以只在
    // 目录粒度 + older_than_days=None 时启用；实际执行（dry_run=false）永远
    // 重新 walk 真实文件系统，删除决策不依赖可能过期的快照。树上匹配不到
    // （目标根不在最近扫描范围内）时回落真实 walk，行为与旧版一致。
    //
    // 目录粒度 scope 的 days 过滤在语义上不成立（目录 mtime = 最后写入
    // 时间，装包即刷新），已从 find_matching_dirs 去掉——所以 dry-run 的
    // "不带保留期过滤"前提在目录粒度下恒成立，这里不再要求 days=None。
    let mut tree_matched: Option<Vec<PathBuf>> = None;
    if dry_run && exec_roots.len() == 1 {
        if let Ok(trees) = state.scan_tree.lock() {
            // 多槽缓存按盘 key 精确命中；请求路径不在任何盘根时找覆盖它的最深根。
            let req_key = crate::cleanup::norm_path_key(&root);
            let node = trees.get(&req_key).or_else(|| {
                trees
                    .values()
                    .filter(|t| {
                        req_key.starts_with(&crate::cleanup::norm_path_key(Path::new(&t.path)))
                    })
                    .max_by_key(|t| crate::cleanup::norm_path_key(Path::new(&t.path)).len())
            });
            if let Some(sub) = node.and_then(|t| locate_subtree(t, &root)) {
                tree_matched = match granularity {
                    // 目录粒度：树上无截断时 find_matching_dirs_on_tree 与真实 walk
                    // 完全对齐；它在收集前后做祖先去重，树上匹配≠漏报。
                    RecycleGranularity::Directory => Some(find_matching_dirs_on_tree(
                        sub,
                        &set,
                        wxid_filter_owned.as_deref(),
                        env_filter_owned.as_deref(),
                    )),
                    // 文件粒度：树上的文件列表受 top-K 截断、文件 mtime 可能缺失，
                    // 在此树上匹配只会漏报——dry-run 预览漏报会把「用户确认的清单」
                    // 与「真实执行的集合」拉开（执行重新 walk 会多删）。因此先过
                    // tree_files_adjudicable 闸（无截断 + days 过滤时全区 mtime 齐
                    // 备）才敢用树；不过闸就回落真实 walk，行为与旧版一致。
                    RecycleGranularity::File
                        if tree_files_adjudicable(sub, older_than_days.is_some()) =>
                    {
                        Some(find_matching_files_on_tree(
                            sub,
                            &set,
                            wxid_filter_owned.as_deref(),
                            env_filter_owned.as_deref(),
                            older_than_days,
                        ))
                    }
                    _ => None,
                };
            }
        }
    }

    // root 随后会被 spawn_blocking move 走，日志用的 display 先存一份。
    let root_display = root.display().to_string();

    let matched: Vec<PathBuf> = match tree_matched {
        Some(m) => m,
        None => {
            let roots = exec_roots.clone();
            tokio::task::spawn_blocking(move || -> Vec<PathBuf> {
                let mut out: Vec<PathBuf> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for r in roots {
                    let hits: Vec<PathBuf> = match granularity {
                        RecycleGranularity::Directory => find_matching_dirs(
                            &r,
                            &set,
                            wxid_filter_owned.as_deref(),
                            env_filter_owned.as_deref(),
                            // 目录粒度 scope 不做「N 天前」过滤：目录 mtime = 最后
                            // 写入时间，装包/写缓存即刷新，按它判断"旧不旧"语义不
                            // 成立，且会让建议显示可清、执行却 0 匹配（"清理了 0b"）。
                            // 缓存目录整体可再生，按全量清理是安全的；文件粒度的
                            // older_than_days 保留原过滤逻辑（微信图片/临时文件按天）。
                            None,
                        ),
                        RecycleGranularity::File => {
                            let mut hits = Vec::new();
                            for entry in diskpilot_walker(&r).into_iter().flatten() {
                                if !entry.file_type().is_file() {
                                    continue;
                                }
                                let p = entry.path();
                                let metadata = match entry.metadata() {
                                    Ok(m) => m,
                                    Err(_) => continue,
                                };
                                if !path_passes_wxid(&p, wxid_filter_owned.as_deref())
                                    || !path_passes_env(&p, env_filter_owned.as_deref())
                                    || !mtime_older_than(&metadata, older_than_days)
                                {
                                    continue;
                                }
                                let s = p.to_string_lossy().replace('\\', "/");
                                if set.is_match(&s) {
                                    hits.push(p);
                                }
                            }
                            hits
                        }
                    };
                    for h in hits {
                        if seen.insert(h.clone()) {
                            out.push(h);
                        }
                    }
                }
                out
            })
            .await
            .map_err(|e| e.to_string())?
        }
    };

    if matched.is_empty() {
        tracing::info!(
            "execute_scope {}/{}: 0 matches (root {root_display})",
            scaffold.id,
            scope.id
        );
        return Ok(Vec::new());
    }
    tracing::info!(
        "execute_scope {}/{}: {} matches (root {root_display})",
        scaffold.id,
        scope.id,
        matched.len()
    );

    // Directory granularity is locked to Recycle regardless of scope.mode —
    // an entire directory removal is high cost to undo (rebuild conda env =
    // minutes to hours, project node_modules = redownload everything),
    // recoverability via Recycle Bin is non-negotiable. File granularity
    // honors the scope's declared mode (recycle/quarantine/delete) as before.
    let action = match (granularity, scope.mode) {
        (RecycleGranularity::Directory, _) => diskpilot_executor::Action::Recycle,
        (RecycleGranularity::File, diskpilot_scaffold::Mode::Recycle) => {
            diskpilot_executor::Action::Recycle
        }
        (RecycleGranularity::File, diskpilot_scaffold::Mode::Quarantine) => {
            diskpilot_executor::Action::Quarantine
        }
        (RecycleGranularity::File, diskpilot_scaffold::Mode::Delete) => {
            diskpilot_executor::Action::Delete
        }
    };
    let plan = Plan {
        action,
        paths: matched,
        reason: format!("DiskPilot scaffold {}/{} (Studio)", scaffold.id, scope.id),
    };
    let out = execute(&plan, dry_run, &state.undo_log, &state.quarantine_root)
        .map_err(|e| e.to_string())?;
    // 真实执行后按根做 cleanup_cache key 级失效，避免 30 分钟 TTL 内继续
    // 返回「已回收」的旧字节。
    if !dry_run {
        crate::cleanup::remove_cache_entries(&state, &invalidate_roots);
    }
    Ok(out)
}

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
    crate::cleanup::remove_cache_entries(&state, &invalidate_roots);
    Ok(out)
}

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
        crate::cleanup::remove_cache_entries(&state, &invalidate_roots);
    }
    out
}

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
fn undo_core(
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
        return Err(
            "撤销目标已变化（列表可能被其他清理操作刷新），请刷新后再试。".into(),
        );
    }
    if !matches!(entry.action, diskpilot_executor::Action::Quarantine) {
        return Ok(None);
    }
    let restored_source = restore_quarantined(&entry, quarantine_root)
        .map_err(|e| format!("undo failed: {e}"))?;
    remove_restored(undo_log, std::slice::from_ref(&restored_source))
        .map_err(|e| e.to_string())?;
    Ok(Some(entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskpilot_scanner::Node;

    /// 进程内临时目录（不同测试并发跑，各自创建独立子目录避免撞名）。
    fn tmp_sub(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("dp-executor-test-{}", std::process::id()));
        let d = base.join(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn confirm_gate_blocks_real_writes_without_confirmation() {
        // 预览（dry-run）是只读的，永远放行；真实落盘必须 confirmed == Some(true)。
        // `None` / `Some(false)` 都不算确认——绕过前端确认窗直接 invoke 要被拦下。
        assert!(confirm_gate(true, None).is_ok());
        assert!(confirm_gate(true, Some(false)).is_ok());
        assert!(confirm_gate(true, Some(true)).is_ok());
        assert!(confirm_gate(false, None).is_err());
        assert!(confirm_gate(false, Some(false)).is_err());
        assert!(confirm_gate(false, Some(true)).is_ok());
    }

    #[test]
    fn undo_core_restores_quarantined_entry() {
        let w = tmp_sub("undo-restore");
        let log = w.join("undo.jsonl");
        let qr = w.join("quarantine");
        let src = w.join("data.dat");
        std::fs::write(&src, "hello").unwrap();
        execute(
            &Plan { action: diskpilot_executor::Action::Quarantine, paths: vec![src.clone()], reason: "test".into() },
            false,
            &log,
            &qr,
        )
        .unwrap();
        assert!(!src.exists(), "隔离后原路径应消失");
        let r = undo_core(&log, &qr, 0, src.to_string_lossy().into_owned());
        assert!(r.is_ok(), "同源同 index 应恢复成功: {r:?}");
        assert!(src.exists(), "恢复后原文件应回来");
        assert_eq!(std::fs::read_to_string(&src).unwrap(), "hello");
        // 已恢复的条目应从日志移除（remove_restored 原子重写）
        let left = list_undo_log(&log).unwrap();
        assert_eq!(left.len(), 0, "已恢复条目应清出日志: {left:?}");
    }

    #[test]
    fn undo_core_rejects_stale_source_drift() {
        let w = tmp_sub("undo-drift");
        let log = w.join("undo.jsonl");
        let qr = w.join("quarantine");
        let a = w.join("a.dat");
        let b = w.join("b.dat");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        execute(
            &Plan { action: diskpilot_executor::Action::Quarantine, paths: vec![a.clone(), b.clone()], reason: "test".into() },
            false,
            &log,
            &qr,
        )
        .unwrap();
        // index 0 现在是 b.dat（最新在前），用户拿旧列表点 a.dat → 必须拒绝
        let r = undo_core(&log, &qr, 0, a.to_string_lossy().into_owned());
        let msg = match &r {
            Err(m) => m.clone(),
            Ok(_) => String::new(),
        };
        assert!(r.is_err() && msg.contains("撤销目标已变化"), "source 漂移必须拒绝，got: {r:?}");
        assert!(!a.exists() && !b.exists(), "漂移拒绝时不能恢复任何文件");
    }

    #[test]
    fn glob_roots_static_prefix_before_wildcard() {
        // `.../.cargo/registry/**` 展开后锚到 registry 目录
        let reg = tmp_sub("anchor-registry").join(".cargo/registry");
        std::fs::create_dir_all(&reg).unwrap();
        let pat = format!("{}/**", reg.to_string_lossy().replace('\\', "/"));
        let roots = scope_glob_roots(&pat, &[]);
        assert_eq!(roots, vec![reg]);
    }

    #[test]
    fn glob_roots_alternation_keeps_all_branches() {
        // `**/AppData/Local/{npm-cache,pnpm-cache}/**`：两个分支在 detect 里
        // 都存在时，两个锚点都要返回——只取一个会漏掉另一个（npm 空、
        // pnpm 706MB 时「建议可清、执行 0b」脱节的真因）。
        let base = tmp_sub("anchor-brace");
        let npm = base.join("AppData/Local/npm-cache");
        let pnpm = base.join("AppData/Local/pnpm-cache");
        std::fs::create_dir_all(&npm).unwrap();
        std::fs::create_dir_all(&pnpm).unwrap();
        let pat = "**/AppData/Local/{npm-cache,pnpm-cache}/**";
        let detect = vec![
            npm.to_string_lossy().into_owned(),
            pnpm.to_string_lossy().into_owned(),
        ];
        let roots = scope_glob_roots(pat, &detect);
        assert!(roots.contains(&npm), "应含 npm 锚点: {roots:?}");
        assert!(roots.contains(&pnpm), "应含 pnpm 锚点: {roots:?}");
        assert_eq!(roots.len(), 2, "两个分支都要保留: {roots:?}");
    }

    #[test]
    fn glob_roots_alternation_no_dir_falls_back_to_detect() {
        // 分支目录都不存在 → 回退 detect 第一条存在的目录
        let base = tmp_sub("anchor-nodir");
        let fallback = base.join("somewhere/exists");
        std::fs::create_dir_all(&fallback).unwrap();
        let pat = "**/AppData/Local/{npm-cache,pnpm-cache}/**";
        let detect = vec![fallback.to_string_lossy().into_owned()];
        let roots = scope_glob_roots(pat, &detect);
        assert_eq!(roots, vec![fallback]);
    }

    #[test]
    fn glob_roots_bare_double_star_no_detect_returns_empty() {
        // 裸 `**` + detect 全不存在 → 空（调用方用 requested_root 兜底）
        assert!(scope_glob_roots("**/AppData/Local/npm-cache/**", &[]).is_empty());
        assert!(scope_glob_roots("**/pkgs/*", &[]).is_empty());
    }

    #[test]
    fn glob_roots_tail_detect_finds_existing_dir() {
        // `**/AppData/Local/npm-cache/**` → 锚段 AppData/Local/npm-cache，
        // detect 里带该后缀且存在者命中。
        let target = tmp_sub("tail-detect").join("AppData/Local/npm-cache");
        std::fs::create_dir_all(&target).unwrap();
        let pat = "**/AppData/Local/npm-cache/**";
        let detect = vec![
            target.to_string_lossy().into_owned(),
            "C:/nonexistent/pnpm-cache".to_string(),
        ];
        let roots = scope_glob_roots(pat, &detect);
        assert_eq!(roots, vec![target]);
    }

    /// 执行侧真实数据回归：`%TEMP%/**` 展开后是 Windows 反斜杠，修复前
    /// executor 直接用它建 globset，与 walk 内部统一正斜杠的匹配目标不匹配
    /// → 真实执行永远 0 命中（「建议可清、清理 0 文件」执行侧根因）。
    /// 修复后 expand_env(...).replace('\\', "/")，这里用真实 TEMP 文件验证。
    #[test]
    #[cfg(windows)]
    fn execute_pattern_normalized_matches_real_temp() {
        use diskpilot_scaffold::expand_env;
        use diskpilot_scanner::find_matching_files_on_tree;
        // 构造一颗含真实 TEMP 顶层文件的小树（模拟 scan_tree 缓存树）。
        let temp = std::env::var("TEMP").expect("TEMP 环境变量");
        let mut children: Vec<Node> = Vec::new();
        for entry in std::fs::read_dir(&temp).unwrap().flatten() {
            let p = entry.path();
            if p.is_file() && children.len() < 50 {
                children.push(Node {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: p.to_string_lossy().into_owned(),
                    is_dir: false,
                    size: p.metadata().map(|m| m.len()).unwrap_or(0),
                    file_count: 0,
                    children: vec![],
                    scaffold_id: None,
                    top_extensions: vec![],
                    children_truncated: None,
                    mtime: None,
                });
            }
        }
        if children.is_empty() {
            eprintln!("skip: 真实 TEMP 无顶层文件");
            return;
        }
        let tree = Node {
            name: "Temp".into(),
            path: temp.clone(),
            is_dir: true,
            size: children.iter().map(|c| c.size).sum(),
            file_count: children.len() as u64,
            children,
            scaffold_id: None,
            top_extensions: vec![],
            children_truncated: None,
            mtime: None,
        };
        // 与 executor 修复一致：展开 + 正斜杠规范化。
        let pat = expand_env("%TEMP%/**").replace('\\', "/");
        let glob = globset::GlobBuilder::new(&pat)
            .literal_separator(false)
            .case_insensitive(true)
            .build()
            .unwrap();
        let mut bb = globset::GlobSetBuilder::new();
        bb.add(glob);
        let set = bb.build().unwrap();
        let hits = find_matching_files_on_tree(&tree, &set, None, None, None);
        assert!(
            !hits.is_empty(),
            "规范化后的 %TEMP%/** 必须命中真实 TEMP 文件（执行侧修复）"
        );
    }
}
