//! 清理建议命令层（Tauri）：cleanup_suggestions / chat_scan_context /
//! scope_sizes_batch。领域逻辑（scope 统计 / 内存树建议 / 缓存失效）已拆到
//! `cleanup_logic` 模块。

use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diskpilot_scaffold::{expand_env, RecycleGranularity};
use diskpilot_scanner::{diskpilot_walker, mtime_older_than, tally_dir_scopes, Node};
use tauri::State;

use crate::cleanup_logic::{
    build_scope_builds, norm_path_key, redact_home_path, suggestions_from_tree, tally_scope_sizes,
    CleanupCacheEntry, CleanupSuggestion, ScopeBuild, ScopeSize,
};
use crate::AppState;

/// 总览页"建议清理项目"的批量接口：一次全盘 walk 同时统计所有 scaffold
/// 的所有 scope，替代前端逐 scaffold 串行调用 `scope_sizes`（每次都是
/// 一次全盘遍历 → 全部分类跑完要几分钟、且没有进度反馈）。
///
/// 目录粒度 scope 无法在单遍文件 walk 里统计（需要按目录聚合尺寸），
/// 已由 `tally_dir_scopes` 合并为一次全盘遍历；文件粒度则全部合并
/// 到这一次 walk 里按 glob 匹配 + mtime 判断，IO 只跑一遍。
///
/// `cached_only`：只读缓存/内存树（秒回），都不命中时返回空列表而不是
/// 回退全盘 walk。总览页用它——应用刚启动还没扫过盘时，绝不为了展示
/// 建议就把整块磁盘遍历一遍（否则一开就卡）。AI 工具可真算（不传）。
#[tauri::command]
pub(crate) async fn cleanup_suggestions(
    state: State<'_, AppState>,
    root_path: String,
    scope_days: Option<HashMap<String, u32>>,
    cached_only: Option<bool>,
) -> Result<Vec<CleanupSuggestion>, String> {
    let cached_only = cached_only.unwrap_or(false);
    let scaffolds = state.scaffolds.lock().unwrap().clone();
    let root = PathBuf::from(&root_path);

    // 优先命中最近一次 scan_path 预热好的树级缓存（秒回）。树级统计没有
    // mtime，给不出「按保留期过滤」的口径，所以只有未传 days 过滤时才复用。
    if scope_days.as_ref().is_none_or(|m| m.is_empty()) {
        let cached = state
            .cleanup_cache
            .lock()
            .unwrap()
            .get(&norm_path_key(&root))
            .filter(|e| e.at.elapsed() < std::time::Duration::from_secs(30 * 60))
            .map(|e| e.suggestions.clone());
        if let Some(s) = cached {
            return Ok(s);
        }
    }

    // 缓存未命中：优先用最近一次扫描的完整树（state.scan_tree）在内存上秒算，
    // 只有从未扫过该范围（树不存在或目标不在树内）才回退真实全盘 walk。
    // 树上的统计没有 mtime，给不出「按保留期过滤」口径，所以只在未传 days
    // 过滤时启用；与上方 cleanup_cache 命中的口径完全一致。
    if scope_days.as_ref().is_none_or(|m| m.is_empty()) {
        let req_key = norm_path_key(&root);
        let tree_suggestions = {
            let trees = state.scan_tree.lock().unwrap();
            // 优先按盘 key 精确命中该盘的完整树。
            let exact = trees.get(&req_key);
            let node = exact.or_else(|| {
                // 兜底：请求路径被某棵更大盘的树覆盖（如请求子目录、或将来
                // 的「全部磁盘」合并树），取覆盖它的最深根，避免误用别的盘。
                let covering = trees.values().filter(|t| {
                    let root_key = norm_path_key(Path::new(&t.path));
                    req_key.starts_with(&root_key)
                });
                covering.max_by_key(|t| norm_path_key(Path::new(&t.path)).len())
            });
            node.map(|t| suggestions_from_tree(t, &scaffolds))
        };
        if let Some(s) = tree_suggestions {
            // 回写 cleanup_cache：树命中结果与 commit_scan 写入的口径一致，
            // 30 分钟 TTL 内重复请求秒回（大树重算要秒级到几十秒，不回写的话
            // 每次切页/重进总览都要全量重算，总览建议卡会长时间停在「正在检查」）。
            if let Ok(mut cache) = state.cleanup_cache.lock() {
                cache.insert(
                    req_key.clone(),
                    CleanupCacheEntry {
                        at: std::time::Instant::now(),
                        suggestions: s.clone(),
                    },
                );
            }
            return Ok(s);
        }
    }

    // cached_only：缓存和内存树都没有 → 直接空列表，绝不全盘 walk。
    // 这是「总览页展示建议」与「AI/用户主动查建议」的关键分界。
    if cached_only {
        return Ok(Vec::new());
    }

    // 预编译全部 scaffold 的 scope glob，避免在 walk 里逐条匹配时重复编译。
    let mut all: Vec<(String, String, String, String, String, ScopeBuild)> = Vec::new(); // (scaffold_id, scope_id, label, desc, path, build)
    for sc in &scaffolds {
        let builds = build_scope_builds(sc)?;
        for (i, b) in builds.iter().enumerate() {
            let scope = &sc.scopes[i];
            let pattern = expand_env(&scope.glob).replace('\\', "/");
            let path = crate::executor::scope_glob_roots(&pattern, &sc.detect)
                .into_iter()
                .next()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            all.push((
                sc.id.clone(),
                b.id.clone(),
                scope.label.clone(),
                sc.disclaimer.clone(),
                path,
                ScopeBuild {
                    id: b.id.clone(),
                    set: b.set.clone(),
                    granularity: b.granularity,
                },
            ));
        }
    }
    let file_indices: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, (_, _, _, _, _, b))| b.granularity == RecycleGranularity::File)
        .map(|(i, _)| i)
        .collect();
    let dir_indices: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, (_, _, _, _, _, b))| b.granularity == RecycleGranularity::Directory)
        .map(|(i, _)| i)
        .collect();

    let mut tally: Vec<(u64, u64)> = vec![(0, 0); all.len()];
    let mut total: Vec<(u64, u64)> = vec![(0, 0); all.len()];

    tokio::task::spawn_blocking(move || -> Result<Vec<CleanupSuggestion>, String> {
        // 文件粒度 scope：单遍 walk，一次匹配所有文件 scope。
        if !file_indices.is_empty() {
            for entry in diskpilot_walker(&root).into_iter().flatten() {
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                let metadata = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let path_str = path.to_string_lossy().replace('\\', "/");
                let size = metadata.len();
                for &i in &file_indices {
                    let b = &all[i].5;
                    if !b.set.is_match(&path_str) {
                        continue;
                    }
                    total[i].0 = total[i].0.saturating_add(size);
                    total[i].1 = total[i].1.saturating_add(1);
                    let days = scope_days.as_ref().and_then(|m| m.get(&b.id).copied());
                    if !mtime_older_than(&metadata, days) {
                        continue;
                    }
                    tally[i].0 = tally[i].0.saturating_add(size);
                    tally[i].1 = tally[i].1.saturating_add(1);
                }
            }
        }

        // 目录粒度 scope：合并为一次全盘 walk（内存树 DP 汇总尺寸），
        // 替代原来每个 scope 各自 find_matching_dirs + dir_size_excluding 的 N 次遍历。
        if !dir_indices.is_empty() {
            let (d_total, d_tally) = tally_dir_scopes(
                &root,
                |i: usize| &all[i].5.set,
                |i: usize| &all[i].5.id,
                &dir_indices,
                None,
                None,
                scope_days.as_ref(),
            );
            for (pos, &i) in dir_indices.iter().enumerate() {
                total[i] = d_total[pos];
                tally[i] = d_tally[pos];
            }
        }

        let mut out: Vec<CleanupSuggestion> = Vec::new();
        for (i, (scaffold_id, scope_id, label, desc, path, _)) in all.iter().enumerate() {
            if !(tally[i].0 > 0) {
                continue;
            }
            out.push(CleanupSuggestion {
                scaffold_id: scaffold_id.clone(),
                scope_id: scope_id.clone(),
                label: label.clone(),
                desc: desc.clone(),
                path: path.clone(),
                bytes: tally[i].0,
                files: tally[i].1,
                total_bytes: total[i].0,
            });
        }
        out.sort_by_key(|x| Reverse(x.bytes));
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// AI 总览上下文：从 AppState 的最近一棵扫描树直接算结构化摘要，
/// 替代前端把整棵树回传 / 浏览器侧手写 flatten+sort。返回：
/// root 元信息、Top N 目录与文件、各 scope 可清理字节合计。
#[tauri::command]
pub(crate) fn chat_scan_context(
    state: State<'_, AppState>,
    top_n: Option<usize>,
) -> Option<serde_json::Value> {
    let n = top_n.unwrap_or(25).clamp(1, 100);
    // 多盘缓存里选信息最全的一棵（size 最大），与「最近扫了哪块盘看哪块」
    // 的旧语义足够接近，且总览对大盘更有代表性。
    let tree = {
        let trees = state.scan_tree.lock().ok()?;
        trees.values().max_by_key(|t| t.size).cloned()?
    };

    #[derive(serde::Serialize)]
    struct Rank {
        path: String,
        name: String,
        size: u64,
        is_dir: bool,
        depth: usize,
    }

    // 深度 ≤2 的层级全量收集（同前端 buildOverviewSummary 的 flatten 深度），
    // 按大小降序取 Top N。
    let mut all: Vec<Rank> = Vec::new();
    let mut stack: Vec<(usize, &Node)> = vec![(0, &tree)];
    while let Some((depth, node)) = stack.pop() {
        if depth > 0 {
            all.push(Rank {
                path: node.path.clone(),
                name: node.name.clone(),
                size: node.size,
                is_dir: node.is_dir,
                depth,
            });
        }
        if depth < 3 {
            for c in node.children.iter().rev() {
                stack.push((depth + 1, c));
            }
        }
    }
    all.sort_by_key(|x| Reverse(x.size));
    all.truncate(n);

    // 各 scope 可清理字节合计（复用扫描时已算好的统计逻辑的树版函数）。
    let scope_bytes: u64 = suggestions_from_tree(&tree, &state.scaffolds.lock().ok()?)
        .iter()
        .map(|s| s.bytes)
        .sum();

    // 子目录 Top 5（若 root 有 children）：AI 总览里最常用的「这盘里什么最大」。
    let mut subdirs: Vec<Rank> = tree
        .children
        .iter()
        .filter(|c| c.is_dir)
        .map(|c| Rank {
            path: c.path.clone(),
            name: c.name.clone(),
            size: c.size,
            is_dir: true,
            depth: 1,
        })
        .collect();
    subdirs.sort_by_key(|x| Reverse(x.size));
    subdirs.truncate(5);

    // AI 侧只应看到目录结构，不暴露 Windows 真实用户名——所有 path 字段
    // 过 redact_home_path（`C:\Users\<名字>\…` → `C:\Users\<USER>\…`）。
    fn redact_rank(r: &Rank) -> Rank {
        Rank {
            path: redact_home_path(&r.path),
            name: r.name.clone(),
            size: r.size,
            is_dir: r.is_dir,
            depth: r.depth,
        }
    }
    let all_redacted: Vec<Rank> = all.iter().map(redact_rank).collect();
    let subdirs_redacted: Vec<Rank> = subdirs.iter().map(redact_rank).collect();

    Some(serde_json::json!({
        "root": redact_home_path(&tree.path),
        "root_name": tree.name,
        "total_size": tree.size,
        "total_files": tree.file_count,
        "top_entries": all_redacted,
        "top_dirs": subdirs_redacted,
        "reclaimable_bytes": scope_bytes,
    }))
}

#[derive(serde::Serialize)]
pub(crate) struct ScopeSizesForRoot {
    root_path: String,
    sizes: Vec<ScopeSize>,
}

/// Batch variant used by the cleanup modal: one globset build + one blocking
/// task covers every matched root (WeChat locations, Conda envs, ...) instead
/// of fanning out one `scope_sizes` IPC per root - each of those recompiled
/// every scope glob and spawned its own walk. Roots are disjoint subtrees so
/// each still gets its own walk, but the per-root compile + IPC overhead
/// collapses.
#[tauri::command]
pub(crate) async fn scope_sizes_batch(
    state: State<'_, AppState>,
    scaffold_id: String,
    root_paths: Vec<String>,
    scope_days: Option<HashMap<String, u32>>,
    wxid_filter: Option<Vec<String>>,
    env_filter: Option<Vec<String>>,
) -> Result<Vec<ScopeSizesForRoot>, String> {
    let scaffold = state
        .scaffolds
        .lock()
        .unwrap()
        .iter()
        .find(|s| s.id == scaffold_id)
        .cloned()
        .ok_or_else(|| format!("scaffold not found: {scaffold_id}"))?;
    let builds = build_scope_builds(&scaffold)?;
    let roots: Vec<PathBuf> = root_paths.iter().map(PathBuf::from).collect();
    tokio::task::spawn_blocking(move || -> Vec<ScopeSizesForRoot> {
        roots
            .into_iter()
            .map(|root| ScopeSizesForRoot {
                root_path: root.to_string_lossy().into_owned(),
                sizes: tally_scope_sizes(
                    &root,
                    &builds,
                    scope_days.as_ref(),
                    wxid_filter.as_deref(),
                    env_filter.as_deref(),
                ),
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
