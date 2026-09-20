//! 磁盘扫描领域：scan_path / scan_path_usn / cancel_scan / tag_and_truncate / 扫描事件。
//! 从 lib.rs 拆分（拆分时逐函数核对，逻辑不变）。
//!
//! USN 增量接线说明：
//! - 全量扫描（`run_full_scan`）完成后把当前 journal 状态作为**基线游标**记进
//!   `AppState.usn_cursors`（key = 归一化盘根）。没有基线就没有增量起点。
//! - `scan_path_usn` 在「有缓存完整树 + 有基线游标」时走增量：`scan_with_usn`
//!   只回放 Change Journal 差异并 merge 进缓存树，亚秒级完成；增量不可用
//!   （非 NTFS / journal 未启用 / 已回绕 / 扫描失败）时静默回退全量并重记基线。
//! - 两种路径最终都走 `commit_scan` 统一落盘：scan-tree / cleanup-suggestions /
//!   usn-cursors / scan-stats 事件。前端只感知到一个完整 `Node` 返回。

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use diskpilot_scaffold::{compile_all, detect_compiled, CompiledScaffold};
#[cfg(windows)]
use diskpilot_scanner::scan_with_usn;
#[cfg(windows)]
use diskpilot_scanner::usn::UsnCursor;
use diskpilot_scanner::ScanOptions;
use diskpilot_scanner::{scan_with_stats_cancellable, Node, ScanStats};
use tauri::{AppHandle, Emitter, State};

use crate::cleanup::{suggestions_from_tree, CleanupSuggestion};
use crate::{AppState, CleanupCacheEntry};

/// 一次扫描（全量或增量）的产出。`node` 是截断后返回前端的树，`node_full`
/// 是完整树（tag_and_truncate 之前），供 suggestions / scan_tree 缓存使用。
struct ScanOutcome {
    node: Node,
    node_full: Node,
    stats: ScanStats,
    tag_ms: u64,
    suggestions: Vec<CleanupSuggestion>,
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct ScanProgressEvent {
    files_seen: u64,
    bytes_seen: u64,
    current_path: String,
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct ScanStatsEvent {
    mode: String,
    mft_attempted: bool,
    mft_succeeded: bool,
    mft_ms: u64,
    walk_ms: u64,
    build_tree_ms: u64,
    /// Time inside `scan_with_stats` (mft_ms + walk_ms + build_tree_ms + overhead).
    scanner_total_ms: u64,
    /// Time spent in the post-scan tag-and-truncate pass (replaces frontend tagScaffolds).
    tag_ms: u64,
    /// Time inside the `scan_path` command (scanner_total_ms + tag_ms + spawn_blocking overhead).
    cmd_total_ms: u64,
    files_seen: u64,
    bytes_seen: u64,
    dirs_in_acc: u64,
}

impl From<(u64, u64, &ScanStats)> for ScanStatsEvent {
    fn from((cmd_ms, tag_ms, s): (u64, u64, &ScanStats)) -> Self {
        Self {
            mode: s.mode.clone(),
            mft_attempted: s.mft_attempted,
            mft_succeeded: s.mft_succeeded,
            mft_ms: s.mft_ms,
            walk_ms: s.walk_ms,
            build_tree_ms: s.build_tree_ms,
            scanner_total_ms: s.total_ms,
            tag_ms,
            cmd_total_ms: cmd_ms,
            files_seen: s.files_seen,
            bytes_seen: s.bytes_seen,
            dirs_in_acc: s.dirs_in_acc,
        }
    }
}

/// Walk `node` in place, filling `scaffold_id` for directories and truncating
/// each level's children to the same depth-based caps the old frontend
/// `tagScaffolds` applied (depth<2 → 100, depth<4 → 50, else → 20).
///
/// Order matters: we recurse into ALL children first to propagate scaffold tags
/// up, then truncate. `detect_compiled` receives the child names captured
/// during the scan so `must_have_child` checks are in-memory (no disk exists).
pub(crate) fn tag_and_truncate(node: &mut Node, compiled: &[CompiledScaffold], depth: usize) {
    if node.is_dir {
        // 内存 children 名替代磁盘 exists：must_have_child 直接在子目录列表里查，
        // 消除对每盘数十万目录的 path.join(child).exists() 磁盘 IO（C 盘实测
        // tag 阶段 229s → 毫秒级）。
        let child_names: Vec<String> = node.children.iter().map(|c| c.name.clone()).collect();
        node.scaffold_id = detect_compiled(compiled, Path::new(&node.path), Some(&child_names));
    }
    for c in &mut node.children {
        tag_and_truncate(c, compiled, depth + 1);
    }
    let cap = if depth < 2 {
        100
    } else if depth < 4 {
        50
    } else {
        20
    };
    if node.children.len() > cap {
        // 记录截断前原始子项数：前端展开时据此判断「这层被砍过」，
        // 从后端内存树按需取回完整子项（按需加载子树）。
        node.children_truncated = Some(node.children.len() as u64);
        // Partition tagged subtrees first (preserving their original size-desc
        // order), then fill remaining slots from the rest. Cap the tagged group
        // at `cap` too, so a freak case with > cap matches at one level still
        // produces a bounded tree.
        let (tagged, rest): (Vec<Node>, Vec<Node>) =
            node.children.drain(..).partition(has_scaffold_tag);
        let mut survivors: Vec<Node> = tagged.into_iter().take(cap).collect();
        let need = cap.saturating_sub(survivors.len());
        survivors.extend(rest.into_iter().take(need));
        // Restore size-desc order for display (partition mixed tagged in front).
        survivors.sort_by_key(|c| std::cmp::Reverse(c.size));
        node.children = survivors;
    }
}

/// True if `n` itself, or any descendant, has a scaffold_id assigned. Used by
/// `tag_and_truncate` to decide which subtrees must survive truncation.
fn has_scaffold_tag(n: &Node) -> bool {
    n.scaffold_id.is_some() || n.children.iter().any(has_scaffold_tag)
}

/// 全量扫描入口（保持原 API 语义不变）：从零扫一棵新树。USN 基线游标在
/// `run_full_scan` 结尾记录，供后续 `scan_path_usn` 使用。
#[tauri::command]
pub(crate) async fn scan_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    keep_files_per_dir: Option<u32>,
) -> Result<Node, String> {
    run_full_scan(
        app,
        state,
        std::path::PathBuf::from(&path),
        keep_files_per_dir,
    )
    .await
}

/// USN 增量扫描入口（Windows / NTFS）。优先回放 Change Journal 合并进缓存树；
/// 增量不可用（非 NTFS / journal 未启用 / 已回绕 / 无缓存树或游标 / 增量报错）
/// 时静默回退全量扫描并重记基线游标。前端拿到的永远是完整 `Node`，
/// 扫描路径通过 `scan-stats` 事件的 mode 字段区分（"mft"/"walkdir"/"usn"）。
#[tauri::command]
#[cfg(windows)]
pub(crate) async fn scan_path_usn(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    keep_files_per_dir: Option<u32>,
) -> Result<Node, String> {
    let p = PathBuf::from(&path);
    let key = crate::cleanup::norm_path_key(&p);

    // 增量需要两样东西，且必须同盘（同 key）：该盘的完整缓存树 + 该盘的基线
    // 游标。缺一即回退全量。按 key 取树保证不会把别的盘的树当成本盘的缓存
    // （旧单槽实现跨盘切换时会用 D 盘树 + C 盘游标 merge，污染两棵数据）。
    let cache_tree = state.scan_tree.lock().unwrap().get(&key).cloned();
    let cursor = state.usn_cursors.lock().unwrap().get(&key).cloned();
    let (Some(cache_tree), Some(cursor)) = (cache_tree, cursor) else {
        return run_full_scan(app, state, p, keep_files_per_dir).await;
    };

    // 前端取消标记：与全量扫描共用同一把 cancel flag。
    let app2 = app.clone();
    let app_for_progress = app2.clone();
    let cancel = state.scan_cancel.clone();
    state.scan_cancel.store(false, Ordering::Relaxed);
    let p2 = p.clone();

    let result = tokio::task::spawn_blocking(move || {
        scan_with_usn(&p2, cache_tree, &cursor, Some(&cancel), &|progress| {
            let _ = app_for_progress.emit(
                "scan-progress",
                ScanProgressEvent {
                    files_seen: progress.files_seen,
                    bytes_seen: progress.bytes_seen,
                    current_path: progress.current_path.clone(),
                },
            );
        })
    })
    .await
    .map_err(|e| e.to_string());

    match result {
        // 增量成功：merge 出的树就是新的完整树，做 tag/truncate + suggestions
        // 后 commit，并把新游标写回（注意：不能复用 run_full_scan，那会全盘重扫）。
        Ok(Ok((tree, new_cursor))) => {
            let compiled = compile_all(&state.scaffolds.lock().unwrap().clone());
            let scaffolds_for_stats = state.scaffolds.lock().unwrap().clone();
            let node = tree;
            let node_full = node.clone();
            let suggestions = suggestions_from_tree(&node, &scaffolds_for_stats);
            let tag_t0 = std::time::Instant::now();
            let mut truncated = node.clone();
            tag_and_truncate(&mut truncated, &compiled, 0);
            let tag_ms = tag_t0.elapsed().as_millis() as u64;
            let stats = ScanStats {
                mode: "usn".into(),
                ..ScanStats::default()
            };
            let outcome = ScanOutcome {
                node: truncated,
                node_full,
                stats,
                tag_ms,
                suggestions,
            };
            commit_scan(app, state, &key, outcome, Some(&new_cursor)).await
        }
        // 用户取消：透传，别触发全量回退把取消失败的扫描又跑一遍。
        Err(e) if e.starts_with("scan:cancelled:") => Err(e),
        // 增量不可用（journal 缺失/回绕/扫描失败）：回退全量，重记基线游标。
        _ => run_full_scan(app, state, p, keep_files_per_dir).await,
    }
}

/// 全量扫描共享实现：MFT/walkdir 从零扫 → tag/truncate → 记录 USN 基线游标 →
/// commit。`scan_path` 与 `scan_path_usn`（增量不可用时回退）共用。
async fn run_full_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    p: PathBuf,
    keep_files_per_dir: Option<u32>,
) -> Result<Node, String> {
    let app_for_progress = app.clone();

    // Compile scaffolds outside spawn_blocking so we don't carry the AppState
    // Mutex across thread boundaries. The compiled form is `Send` and used by
    // the post-scan walk to fill `Node.scaffold_id`.
    let compiled = compile_all(&state.scaffolds.lock().unwrap().clone());
    // 统计需要 raw Scaffold（含 label/disclaimer/scope 元数据），一并克隆后 move 进闭包。
    let scaffolds_for_stats = state.scaffolds.lock().unwrap().clone();
    // 闭包会 move `p`（scan_with_stats_cancellable 按值接收 root），外层还要用它写缓存 key。
    let p_for_key = p.clone();

    // 前端「每目录保留文件数」直接映射到 ScanOptions；未传或非法时用默认。
    let opts = ScanOptions {
        keep_files_per_dir: keep_files_per_dir
            .map(|v| v as usize)
            .filter(|v| *v >= 50 && *v <= 2000),
        ..ScanOptions::default()
    };

    // Reset the shared abort flag so a stale cancel from a previous scan
    // doesn't immediately abort this one, then hand a clone to the worker.
    state.scan_cancel.store(false, Ordering::Relaxed);
    let cancel = state.scan_cancel.clone();

    // 在扫描**开始前**采样 USN 基线：扫描窗口内新建/删除的文件的 USN 都落在
    // 这个 start 之后，下次增量从 start 回放就能补上 walker 没见到的变更；
    // 若在扫描完成后才采样（旧实现），窗口内新建的文件 USN 落在基线之前，
    // 下次增量永远错过，永久漏统计。
    let baseline = query_baseline_cursor(&p_for_key);

    let result = tokio::task::spawn_blocking(move || {
        let scan_result = scan_with_stats_cancellable(p, opts, Some(&cancel), |progress| {
            let _ = app_for_progress.emit(
                "scan-progress",
                ScanProgressEvent {
                    files_seen: progress.files_seen,
                    bytes_seen: progress.bytes_seen,
                    current_path: progress.current_path.clone(),
                },
            );
        });
        // After the scan returns, walk the tree once to fill scaffold_id and
        // apply the depth-based breadth caps that the frontend's tagScaffolds
        // used to apply. Doing both here in one pass with pre-compiled
        // GlobSets replaces the previous per-directory IPC storm.
        let tag_t0 = std::time::Instant::now();
        scan_result.map(|(node, stats)| {
            // 建议统计必须在 breadth 截断之前跑：`tag_and_truncate` 会按深度
            // 丢弃部分子树，之后的树只能得到偏小的估计。完整树同时保留给
            // `state.scan_tree`（见外层），让 cleanup_suggestions / AI 总览 /
            // dry_run 目录匹配都能在内存树上秒算，不再每次切页都全盘 walk。
            let full = node.clone();
            let suggestions = suggestions_from_tree(&node, &scaffolds_for_stats);
            let mut node = node;
            tag_and_truncate(&mut node, &compiled, 0);
            (
                node,
                stats,
                tag_t0.elapsed().as_millis() as u64,
                suggestions,
                full,
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?;

    let outcome = result.map(
        |(node, stats, tag_ms, suggestions, node_full)| ScanOutcome {
            node,
            node_full,
            stats,
            tag_ms,
            suggestions,
        },
    );
    match outcome {
        Ok(outcome) => {
            // 基线游标在扫描开始前已采样（见上）；这里连同扫描结果一起提交。
            let key = crate::cleanup::norm_path_key(&p_for_key);
            commit_scan(app, state, &key, outcome, baseline.as_ref()).await
        }
        Err(e) => Err(e.to_string()),
    }
}

/// 查询当前卷的 USN journal 状态作为基线游标。非 Windows / journal 不可用
/// 时返回 None（调用方跳过游标写入，行为同全量扫描时代一致）。
#[cfg(windows)]
fn query_baseline_cursor(root: &Path) -> Option<UsnCursor> {
    let letter = drive_letter_of(root)?;
    let st = diskpilot_scanner::usn::win::query_journal(letter).ok()?;
    Some(UsnCursor {
        journal_id: st.journal_id,
        next_usn: st.next_usn,
    })
}

#[cfg(not(windows))]
fn query_baseline_cursor(_root: &Path) -> Option<()> {
    None
}

#[cfg(windows)]
fn drive_letter_of(p: &Path) -> Option<char> {
    let s = p.to_string_lossy();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        let c = bytes[0] as char;
        if c.is_ascii_alphabetic() {
            return Some(c);
        }
    }
    None
}

/// 扫描结果统一落盘：emit scan-stats 事件、更新 cleanup-suggestions 缓存、
/// 覆盖 scan_tree 完整树、写回 USN 游标。`new_cursor` 为 None 时游标不动
/// （非 Windows 或调用方明确不续游标）。游标类型按平台别名：Windows 为
/// `UsnCursor`，非 Windows 用 `()` 占位（`query_baseline_cursor` 返回同型）。
#[cfg(windows)]
type CursorParam<'a> = Option<&'a diskpilot_scanner::usn::UsnCursor>;
#[cfg(not(windows))]
type CursorParam<'a> = Option<&'a ()>;

async fn commit_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    key: &str,
    outcome: ScanOutcome,
    new_cursor: CursorParam<'_>,
) -> Result<Node, String> {
    let cmd_ms = outcome.stats.total_ms; // 增量路径无独立 cmd 计时，近似用 scanner_total
    tracing::info!(
        "scan: mode={} cmd_ms={} scanner_ms={} tag_ms={}",
        outcome.stats.mode,
        cmd_ms,
        outcome.stats.total_ms,
        outcome.tag_ms,
    );
    let _ = app.emit(
        "scan-stats",
        &ScanStatsEvent::from((cmd_ms, outcome.tag_ms, &outcome.stats)),
    );

    // 建议统计与完整树：cleanup_suggestions / AI 总览 / dry_run 目录匹配都从
    // 内存树秒算，替代每次切页都重新全盘 walk。
    if let Ok(mut cache) = state.cleanup_cache.lock() {
        cache.insert(
            key.to_string(),
            CleanupCacheEntry {
                at: std::time::Instant::now(),
                suggestions: outcome.suggestions,
            },
        );
    }
    // 返回前端的树是截断后的；这里存的是完整克隆，按盘 key 入多槽缓存，
    // 切盘后每盘都能命中自己的树做增量/秒算，互不覆盖。
    let node_full = outcome.node_full.clone();
    if let Ok(mut trees) = state.scan_tree.lock() {
        trees.insert(key.to_string(), node_full);
    }
    // 游标与树生命周期一致：树被覆盖时同步更新，保证增量始终有有效起点。
    if let Some(c) = new_cursor {
        if let Ok(mut cursors) = state.usn_cursors.lock() {
            cursors.insert(key.to_string(), c.clone());
        }
    }
    // R6 磁盘空间趋势：扫描完成后追加一条卷容量快照（只读展示数据，失败静默）。
    // key 是规范化的扫描根（"C:\" / "D:\dir"），用它直接查卷容量；同一卷下
    // 扫不同子目录不会重复记（record_space_point 按 root 去重——root 是扫描根，
    // 不同扫描根各自成列，总览按盘聚合）。
    crate::space_history::record_after_scan(&app, key);
    Ok(outcome.node)
}

/// Ask any in-flight `scan_path` / `scan_path_usn` to abort. The next walk/MFT
/// progress check sees the flag and returns a "cancelled" error; the frontend
/// treats that as a normal interrupt instead of an error toast.
#[tauri::command]
pub(crate) fn cancel_scan(state: State<'_, AppState>) {
    state.scan_cancel.store(true, Ordering::Relaxed);
}

/// 按需加载子树：从前端可见的截断树里展开某目录时，从 `state.scan_tree`
/// 内存完整树中按路径取回该目录的**完整未截断**子树（含全部子项，不受
/// 深度 cap 限制）。纯内存查找零磁盘 IO；找不到（路径不在扫描范围/已被
/// 回收剪除）返回 None，前端显示「子树不可用」而非报错。
#[tauri::command]
pub(crate) fn tree_subtree(
    state: State<'_, AppState>,
    path: String,
) -> Result<Option<Node>, String> {
    let trees = state.scan_tree.lock().unwrap();
    // 路径可能来自任意已扫盘：遍历多槽缓存树，找第一棵包含该路径的。
    Ok(trees.values().find_map(|root| find_subtree(root, &path)))
}

/// 在完整树中按（大小写不敏感、`/`→`\` 归一化后的）绝对路径查找目录并
/// 返回其完整子树。纯函数，供 `tree_subtree` 与单元测试共用。
fn find_subtree(root: &Node, path: &str) -> Option<Node> {
    let norm = |p: &str| p.replace('/', "\\").to_lowercase();
    let needle = norm(path);

    // 直接命中根。
    if norm(&root.path) == needle {
        return Some(root.clone());
    }
    // BFS 查找目标目录；命中即返回其完整子树。
    let mut queue: Vec<&Node> = root.children.iter().collect();
    while let Some(n) = queue.pop() {
        if norm(&n.path) == needle {
            return Some(n.clone());
        }
        if n.is_dir {
            queue.extend(n.children.iter());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, path: &str, is_dir: bool, children: Vec<Node>) -> Node {
        Node {
            name: name.into(),
            path: path.into(),
            is_dir,
            size: 0,
            file_count: 0,
            children,
            scaffold_id: None,
            top_extensions: Vec::new(),
            children_truncated: None,
            mtime: None,
        }
    }

    fn sample_tree() -> Node {
        node(
            "C:",
            "C:\\",
            true,
            vec![
                node(
                    "Users",
                    "C:\\Users",
                    true,
                    vec![node(
                        "alice",
                        "C:\\Users\\alice",
                        true,
                        vec![
                            node("doc.txt", "C:\\Users\\alice\\doc.txt", false, vec![]),
                            node(
                                "AppData",
                                "C:\\Users\\alice\\AppData",
                                true,
                                vec![node(
                                    "Local",
                                    "C:\\Users\\alice\\AppData\\Local",
                                    true,
                                    vec![],
                                )],
                            ),
                        ],
                    )],
                ),
                node("Windows", "C:\\Windows", true, vec![]),
            ],
        )
    }

    #[test]
    fn find_subtree_returns_full_subtree_for_existing_path() {
        let tree = sample_tree();
        let hit = find_subtree(&tree, "c:\\users\\alice").expect("应命中 alice");
        assert_eq!(hit.name, "alice");
        assert_eq!(hit.path, "C:\\Users\\alice");
        // 完整子树：AppData 也返回，不受截断影响
        assert_eq!(hit.children.len(), 2, "alice 下 doc.txt + AppData");
        let appdata = find_subtree(&tree, "C:/Users/alice/AppData").expect("斜杠归一化应命中");
        assert_eq!(appdata.children[0].name, "Local");
    }

    #[test]
    fn find_subtree_is_case_insensitive_and_matches_root() {
        let tree = sample_tree();
        let hit = find_subtree(&tree, "C:\\WINDOWS").expect("大小写不敏感");
        assert_eq!(hit.name, "Windows");
        let root = find_subtree(&tree, "c:\\").expect("根命中");
        assert_eq!(root.path, "C:\\");
    }

    #[test]
    fn find_subtree_returns_none_for_missing_path() {
        let tree = sample_tree();
        assert!(find_subtree(&tree, "C:\\nope").is_none());
        assert!(find_subtree(&tree, "C:\\Users\\bob").is_none());
    }

    #[test]
    fn tag_and_truncate_marks_children_truncated_and_stores_original_count() {
        // 构造 130 个孩子：深度 0 的 cap 是 100，必然触发截断并记录原始数。
        let mut root = node(
            "C:",
            "C:\\",
            true,
            (0..130)
                .map(|i| node(&format!("d{i}"), &format!("C:\\d{i}"), true, vec![]))
                .collect(),
        );
        tag_and_truncate(&mut root, &[], 0);
        assert_eq!(root.children.len(), 100, "深度 0 应截到 100");
        assert_eq!(root.children_truncated, Some(130), "应记录截断前原始数 130");
        // 未截断的目录保持 None
        let mut small = node("small", "C:\\small", true, vec![]);
        tag_and_truncate(&mut small, &[], 0);
        assert_eq!(small.children_truncated, None);
    }
}
