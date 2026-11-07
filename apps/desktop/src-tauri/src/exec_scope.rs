//! 作用域执行（从 executor.rs 拆分，2026-10-12）。
//! execute_scope：scope glob → 实际文件 → executor，dry_run 预览 / 确认落盘。

use std::path::{Path, PathBuf};

use diskpilot_executor::{execute, Plan, UndoEntry};
use diskpilot_scaffold::{expand_env, RecycleGranularity};
use diskpilot_scanner::{
    diskpilot_walker, find_matching_dirs, find_matching_dirs_on_tree, find_matching_files_on_tree,
    locate_subtree, mtime_older_than, path_passes_env, path_passes_wxid, tree_files_adjudicable,
};
use tauri::State;

use super::confirm_gate;
use super::exec_glob::scope_glob_roots;
use crate::AppState;

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
    // R505：真实执行（非 dry_run）同时要求后端权限 cleanup.execute——
    // 与 execute_ai_plan 同口径，防止只靠客户端 confirmed 被绕过。
    if !dry_run
        && !state
            .perm_grants
            .lock()
            .unwrap()
            .contains("cleanup.execute")
    {
        return Err(
            "execute:denied: 未开启「执行清理」权限（cleanup.execute）。请先在权限中心开启后重试。"
                .into(),
        );
    }
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
    // R505：执行期复验红线——scaffolds/*.toml 落盘后可能被改动，
    // 安装期的红线校验不能当作运行期唯一防线（与 CI lint 共用同一锚定空间）。
    if !dry_run {
        if let Some(reason) = crate::scaffold_cmds::scaffold_red_line_reject(&scaffold) {
            return Err(format!("execute:red-line: {reason}"));
        }
    }

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
            let req_key = crate::cleanup_logic::norm_path_key(&root);
            let node = trees.get(&req_key).or_else(|| {
                trees
                    .values()
                    .filter(|t| {
                        req_key
                            .starts_with(&crate::cleanup_logic::norm_path_key(Path::new(&t.path)))
                    })
                    .max_by_key(|t| crate::cleanup_logic::norm_path_key(Path::new(&t.path)).len())
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
        crate::cleanup_logic::remove_cache_entries(&state, &invalidate_roots);
    }
    Ok(out)
}
