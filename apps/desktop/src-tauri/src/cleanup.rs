//! 清理建议领域：scope 统计 / cleanup_suggestions / 内存树建议。
//! 从 lib.rs 拆分（拆分时逐函数核对，逻辑不变）。

use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diskpilot_scaffold::{expand_env, RecycleGranularity, Scaffold};
use diskpilot_scanner::{
    diskpilot_walker, mtime_older_than, path_passes_env, path_passes_wxid, tally_dir_scopes, Node,
};
use tauri::State;

use crate::AppState;

#[derive(Clone)]
pub(crate) struct CleanupCacheEntry {
    /// 缓存写入时刻，仅做粗略新鲜度参考（scan 后即失效重建）。
    pub(crate) at: std::time::Instant,
    pub(crate) suggestions: Vec<CleanupSuggestion>,
}

/// 缓存 key 归一化：统一大小写与分隔符，`C:\` 与 `c:/` 命中同一条目。
pub(crate) fn norm_path_key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase().replace('\\', "/")
}

/// 把路径里的真实用户名隐去，只给 AI 看目录结构而不泄漏账号名。
/// `C:\Users\alice\Downloads\x` → `C:\Users\<USER>\Downloads\x`。
/// 从路径本身推断盘符后 `Users/<真实名字>` 段（不读注册表/环境变量，纯
/// 路径处理跨平台安全；非 Windows 形状的路径原样返回）。
pub(crate) fn redact_home_path(p: &str) -> String {
    let s = p.replace('\\', "/");
    let parts: Vec<&str> = s.split('/').collect();
    // 形如 `C:/Users/alice/...`
    if parts.len() >= 3
        && parts[0].len() == 2
        && parts[0].ends_with(':')
        && parts[1].eq_ignore_ascii_case("users")
    {
        let mut out = parts;
        out[2] = "<USER>";
        out.join("/")
    // 形如 `/Users/alice/...`（macOS/Linux 布局，保守同样处理）
    } else if parts.len() >= 3 && parts[0].is_empty() && parts[1].eq_ignore_ascii_case("users") {
        let mut out = parts;
        out[2] = "<USER>";
        out.join("/")
    // 无盘符的裸 `Users/<名>`
    } else if parts.len() >= 2 && parts[0].eq_ignore_ascii_case("users") {
        let mut out = parts;
        out[1] = "<USER>";
        out.join("/")
    } else {
        s
    }
}

/// 清理后按路径移除 cleanup_cache 对应条目（key 级失效），避免清一个盘时把
/// 其他盘的 30 分钟建议缓存也全量清空、白白重建。路径会被归一化成与写入时
/// 相同的 key；请求路径不在缓存条目内时，把覆盖它的祖先条目一并清掉
/// （保守兜底：清理子目录后盘根缓存仍含旧字节）。
/// 返回缓存中所有与 `key` 相同、或**覆盖** `key`（是其后代路径的祖先缓存）
/// 的条目 key。前缀比较先把每个候选 key 尾斜杠去掉再拼 `/` 前缀：
/// 盘根 key `c:/` 若不处理会拼出 `c://`，永远匹配不上子目录 → 清不掉。
pub(crate) fn covering_cache_keys<'a>(
    cache_keys: impl Iterator<Item = &'a String>,
    key: &str,
) -> Vec<String> {
    cache_keys
        .filter(|k| {
            key == *k || {
                let base = k.trim_end_matches('/');
                key.starts_with(&format!("{}/", base))
            }
        })
        .cloned()
        .collect()
}

pub(crate) fn remove_cache_entries(state: &AppState, roots: &[PathBuf]) {
    let mut cache = state.cleanup_cache.lock().unwrap();
    for root in roots {
        let key = norm_path_key(root);
        if cache.remove(&key).is_some() {
            continue;
        }
        let covering: Vec<String> = covering_cache_keys(cache.keys(), &key);
        for k in covering {
            cache.remove(&k);
        }
    }
}

/// 从扫描得到的内存 Node 树上直接统计各 scope 的可清理字节，语义对齐
/// `cleanup_suggestions` 的真实 walk 版：
///
/// - 目录粒度：glob 命中目录节点即计其 `size`（整棵子树之和，扫描期已算好，
///   不依赖被截断的文件列表），并且同一 scope 的祖先命中后后代不再重复计数
///   （等价 walk 版的 `dedup_shallowest`）；计数口径 = 命中目录个数，与
///   `tally_dir_scopes` 一致。
/// - 文件粒度：逐个文件节点 glob 匹配累加。注意文件列表受 `keep_files_per_dir`
///   top-K 裁剪，这是下界估计（top-K 按大小保留，通常覆盖绝大部分字节）。
///
/// 树上没有 mtime，产出的是「不带保留期过滤」的口径：`bytes == total_bytes`。
/// 必须在 `tag_and_truncate` 之前对完整树调用，否则 breadth 截断会丢掉整个子树。
pub(crate) fn suggestions_from_tree(root: &Node, scaffolds: &[Scaffold]) -> Vec<CleanupSuggestion> {
    struct Row {
        scaffold_id: String,
        scope_id: String,
        label: String,
        desc: String,
        is_dir: bool,
        pattern: String,
        path: String,
        set: globset::GlobSet,
    }
    let mut rows: Vec<Row> = Vec::new();
    for sc in scaffolds {
        let Ok(builds) = build_scope_builds(sc) else {
            continue;
        };
        for (i, b) in builds.into_iter().enumerate() {
            let pattern = expand_env(&sc.scopes[i].glob).replace('\\', "/");
            let path = crate::executor::scope_glob_roots(&pattern, &sc.detect)
                .into_iter()
                .next()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            rows.push(Row {
                scaffold_id: sc.id.clone(),
                scope_id: b.id.clone(),
                label: sc.scopes[i].label.clone(),
                desc: sc.disclaimer.clone(),
                is_dir: b.granularity == RecycleGranularity::Directory,
                pattern,
                path,
                set: b.set,
            });
        }
    }
    if rows.is_empty() {
        return Vec::new();
    }
    let mut tally: Vec<(u64, u64)> = vec![(0, 0); rows.len()];

    // 文件粒度 scope 合并进一个 GlobSet：一次 `matches(path)` 拿到全部命中的
    // scope 下标，避免「每个文件 × 每个文件粒度 scope」的 N 次独立 glob 匹配
    // （190 万文件 × 48 scope 会到 9000 万次 is_match，实测占建议统计大头）。
    // 目录粒度 scope 的 `set` 仍各自独立（目录节点数量级远小于文件，逐行
    // is_match 可接受，且 directory 语义靠 claims 去重不适用合并集）。
    let mut merged_builder = globset::GlobSetBuilder::new();
    let mut file_scope_offsets: Vec<usize> = Vec::new(); // merged 下标 → rows 下标
    for (i, r) in rows.iter().enumerate() {
        if !r.is_dir {
            file_scope_offsets.push(i);
            let g = globset::GlobBuilder::new(&r.pattern)
                .literal_separator(false)
                .case_insensitive(true)
                .build()
                .expect("pattern 已由 build_scope_builds 验证，重建不应失败");
            merged_builder.add(g);
        }
    }
    let merged = merged_builder.build().expect("合并 GlobSet 构建失败");

    // `claims` = 当前 DFS 路径上已被「目录粒度」命中的 scope 下标（祖先去重）。
    // 目录粒度 scope 的 glob 命中某个目录即整棵子树归它，祖先/子孙目录不再重复
    // 计（每个字节只归属一次）。文件粒度 scope 不同：glob（`%TEMP%/**` 等）匹配
    // 的是具体文件，每个文件独立体检、独立计数，同一目录内可能有多个文件命中
    // 同一个文件粒度 scope（清任一文件都占这些字节），所以不做 claims 去重。
    fn walk(
        node: &Node,
        rows: &[Row],
        tally: &mut [(u64, u64)],
        claims: &mut Vec<usize>,
        merged: &globset::GlobSet,
        file_scope_offsets: &[usize],
    ) {
        if node.is_dir {
            let path_str = node.path.replace('\\', "/");
            let mut newly = Vec::new();
            for (i, r) in rows.iter().enumerate() {
                if r.is_dir && !claims.contains(&i) && r.set.is_match(&path_str) {
                    tally[i].0 = tally[i].0.saturating_add(node.size);
                    tally[i].1 = tally[i].1.saturating_add(1);
                    newly.push(i);
                }
            }
            claims.extend_from_slice(&newly);
            for c in &node.children {
                walk(c, rows, tally, claims, merged, file_scope_offsets);
            }
            claims.truncate(claims.len() - newly.len());
        } else {
            // 文件节点：一次合并集匹配拿到全部命中的文件粒度 scope 下标，
            // 再映射回 tally 行——避免「每个文件 × 每个文件粒度 scope」的
            // 9000 万次独立 glob 匹配。目录粒度 scope 由祖先目录承担
            // （DFS 在目录节点上已命中，不在文件节点重复）；文件粒度每个
            // 文件独立匹配，各 scope 各行其是，不参与 claims（无祖先归属）。
            if node.size == 0 {
                return;
            }
            let path_str = node.path.replace('\\', "/");
            for mi in merged.matches(&path_str) {
                let i = file_scope_offsets[mi];
                tally[i].0 = tally[i].0.saturating_add(node.size);
                tally[i].1 = tally[i].1.saturating_add(1);
            }
        }
    }
    // 与 walk 版一致：跳过 root 自身的命中，只统计其子树。
    for child in &root.children {
        walk(
            child,
            &rows,
            &mut tally,
            &mut Vec::new(),
            &merged,
            &file_scope_offsets,
        );
    }

    let mut out: Vec<CleanupSuggestion> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if tally[i].0 == 0 {
            continue;
        }
        out.push(CleanupSuggestion {
            scaffold_id: r.scaffold_id.clone(),
            scope_id: r.scope_id.clone(),
            label: r.label.clone(),
            desc: r.desc.clone(),
            path: r.path.clone(),
            bytes: tally[i].0,
            files: tally[i].1,
            total_bytes: tally[i].0,
        });
    }
    out.sort_by_key(|x| Reverse(x.bytes));
    out
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct ScopeSize {
    scope_id: String,
    /// Bytes that would be cleaned at the current `scope_days` setting (i.e.
    /// files matching the glob AND older than retention). This is what the
    /// "X 待清理" pill in the modal renders.
    bytes: u64,
    file_count: u64,
    /// Bytes inside the scope **regardless of retention** — useful so the UI
    /// can show "你共有 12 GB 视频，0 GB 超过 90 天可清". Without this the
    /// modal can't distinguish "scope is empty" from "everything is within
    /// retention" (which used to render as a misleading "空").
    total_bytes: u64,
    total_files: u64,
}

/// Walk `root_path` and tally how many bytes / files each `[[scope]]` glob
/// in `scaffold_id` would match. Used by the Studio panel to show per-scope
/// occupancy alongside the generic "largest sub-items" view. Files matching
/// multiple scopes are counted once per matching scope (the same physical
/// bytes — overlap means cleaning either scope reclaims them).
#[tauri::command]
pub(crate) async fn scope_sizes(
    state: State<'_, AppState>,
    scaffold_id: String,
    root_path: String,
    scope_days: Option<HashMap<String, u32>>,
    wxid_filter: Option<Vec<String>>,
    env_filter: Option<Vec<String>>,
) -> Result<Vec<ScopeSize>, String> {
    let scaffold = state
        .scaffolds
        .lock()
        .unwrap()
        .iter()
        .find(|s| s.id == scaffold_id)
        .cloned()
        .ok_or_else(|| format!("scaffold not found: {scaffold_id}"))?;
    let builds = build_scope_builds(&scaffold)?;

    let root = PathBuf::from(&root_path);
    tokio::task::spawn_blocking(move || {
        tally_scope_sizes(
            &root,
            &builds,
            scope_days.as_ref(),
            wxid_filter.as_deref(),
            env_filter.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Clone, Debug)]
pub(crate) struct ScopeBuild {
    pub(crate) id: String,
    pub(crate) set: globset::GlobSet,
    pub(crate) granularity: RecycleGranularity,
}

/// Compile every scope glob of a scaffold into a reusable set. This is the
/// expensive per-root setup; `scope_sizes_batch` builds it once per scaffold
/// and reuses it across all matched roots instead of recompiling per IPC call.
pub(crate) fn build_scope_builds(scaffold: &Scaffold) -> Result<Vec<ScopeBuild>, String> {
    let mut builds: Vec<ScopeBuild> = Vec::with_capacity(scaffold.scopes.len());
    for sc in &scaffold.scopes {
        // 展开 %TEMP% 等环境变量后路径分隔符是 Windows 反斜杠，而匹配目标
        // （Node.path / walk 路径）统一规范化为正斜杠，必须同步规范化，否则
        // 真实 scaffold 的 `%TEMP%/**` 等文件粒度 glob 永远匹配不上。
        let pattern = expand_env(&sc.glob).replace('\\', "/");
        let glob = globset::GlobBuilder::new(&pattern)
            .literal_separator(false)
            .case_insensitive(true)
            .build()
            .map_err(|e| format!("scope `{}` has invalid glob `{}`: {e}", sc.id, sc.glob))?;
        let mut b = globset::GlobSetBuilder::new();
        b.add(glob);
        builds.push(ScopeBuild {
            id: sc.id.clone(),
            set: b.build().map_err(|e| e.to_string())?,
            granularity: sc.recycle_granularity,
        });
    }
    Ok(builds)
}

/// Walk one root and tally per-scope occupancy. Honours days retention, wxid
/// and conda-env filters identically to `execute_scope` so the size preview
/// always matches what a cleanup would actually touch.
pub(crate) fn tally_scope_sizes(
    root: &Path,
    builds: &[ScopeBuild],
    scope_days: Option<&HashMap<String, u32>>,
    wxid_filter: Option<&[String]>,
    env_filter: Option<&[String]>,
) -> Vec<ScopeSize> {
    let mut results: Vec<ScopeSize> = Vec::with_capacity(builds.len());
    let file_indices: Vec<usize> = builds
        .iter()
        .enumerate()
        .filter(|(_, b)| b.granularity == RecycleGranularity::File)
        .map(|(i, _)| i)
        .collect();
    let dir_indices: Vec<usize> = builds
        .iter()
        .enumerate()
        .filter(|(_, b)| b.granularity == RecycleGranularity::Directory)
        .map(|(i, _)| i)
        .collect();

    // File scopes: single walk, tally per-file across all file scopes.
    // Track BOTH "eligible at the requested retention" (tally) and
    // "total in scope ignoring retention" (total) - UI uses the gap to
    // explain "X GB total - 0 GB older than 90d will be cleaned" instead
    // of the previous misleading "empty".
    let mut tally: Vec<(u64, u64)> = vec![(0, 0); builds.len()];
    let mut total: Vec<(u64, u64)> = vec![(0, 0); builds.len()];
    if !file_indices.is_empty() {
        for entry in diskpilot_walker(root).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !path_passes_wxid(&path, wxid_filter) || !path_passes_env(&path, env_filter) {
                continue;
            }
            let path_str = path.to_string_lossy().replace('\\', "/");
            let size = metadata.len();
            for &i in &file_indices {
                let b = &builds[i];
                if !b.set.is_match(&path_str) {
                    continue;
                }
                total[i].0 = total[i].0.saturating_add(size);
                total[i].1 = total[i].1.saturating_add(1);
                let days = scope_days.and_then(|m| m.get(&b.id).copied());
                if !mtime_older_than(&metadata, days) {
                    continue;
                }
                tally[i].0 = tally[i].0.saturating_add(size);
                tally[i].1 = tally[i].1.saturating_add(1);
            }
        }
    }

    // Directory scopes: 合并为一次全盘 walk（内存树 DP 汇总尺寸），
    // 替代原来每个 scope 各自 find_matching_dirs + dir_size_excluding 的 N 次遍历。
    let (d_total, d_tally) = tally_dir_scopes(
        root,
        |i: usize| &builds[i].set,
        |i: usize| &builds[i].id,
        &dir_indices,
        wxid_filter,
        env_filter,
        scope_days,
    );
    for (pos, &i) in dir_indices.iter().enumerate() {
        total[i] = d_total[pos];
        tally[i] = d_tally[pos];
    }

    // Build output preserving builds order.
    for (i, b) in builds.iter().enumerate() {
        results.push(ScopeSize {
            scope_id: b.id.clone(),
            bytes: tally[i].0,
            file_count: tally[i].1,
            total_bytes: total[i].0,
            total_files: total[i].1,
        });
    }
    results
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct CleanupSuggestion {
    /// 建议可清字节（reminder.rs 按盘累加算提醒阈值）。
    pub(crate) bytes: u64,
    scaffold_id: String,
    scope_id: String,
    label: String,
    desc: String,
    /// 可清理内容的真实锚点目录（scope glob 展开后的实际路径）。
    /// AI 出清单时直接引用这个路径，不得自行发明（否则会指向
    /// `C:\Users\Public\...` 这类不存在的目录 → 执行 0 B）。
    pub(crate) path: String,
    files: u64,
    total_bytes: u64,
}

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
mod tests {
    use super::*;
    use diskpilot_scaffold::{Mode, Risk, Scope};

    #[test]
    fn redact_home_path_hides_windows_username() {
        assert_eq!(
            redact_home_path(r"C:\Users\alice\Downloads\setup.exe"),
            "C:/Users/<USER>/Downloads/setup.exe"
        );
        // 大小写不敏感（Windows）
        assert_eq!(
            redact_home_path(r"c:\USERS\Bob\Desktop\x"),
            "c:/USERS/<USER>/Desktop/x"
        );
        // 非用户路径不动
        assert_eq!(
            redact_home_path(r"C:\Program Files\App"),
            "C:/Program Files/App"
        );
        assert_eq!(redact_home_path(r"D:\games\steam"), "D:/games/steam");
        // 盘符后无 Users 的绝对路径不动
        assert_eq!(redact_home_path(r"C:\work\repo"), "C:/work/repo");
        // 非 Windows 布局的 /Users 也隐去
        assert_eq!(
            redact_home_path("/Users/amy/Projects"),
            "/Users/<USER>/Projects"
        );
        // 裸 Users/<名> 前缀
        assert_eq!(
            redact_home_path("Users/tom/.config"),
            "Users/<USER>/.config"
        );
    }

    #[test]
    fn covering_keys_drive_root_matches_subdir_entries() {
        // 清理 detect 根（`c:/users/x/appdata/...`）后，盘根 key `c:/` 的缓存条目
        // 必须被覆盖清除，否则 30 分钟 TTL 内建议仍返回已回收的旧字节。
        let keys = vec![
            "c:/".to_string(),
            "c:/users/x/appdata/local".to_string(),
            "d:/".to_string(),
        ];
        let hit = covering_cache_keys(keys.iter(), "c:/users/x/appdata/local");
        assert!(
            hit.contains(&"c:/".to_string()),
            "盘根条目应被覆盖清除: {hit:?}"
        );
        assert!(hit.contains(&"c:/users/x/appdata/local".to_string()));
        assert!(!hit.contains(&"d:/".to_string()));
    }

    #[test]
    fn covering_keys_exact_match_wins() {
        let keys = vec!["c:/users/x".to_string(), "c:/users/x/sub".to_string()];
        let hit = covering_cache_keys(keys.iter(), "c:/users/x");
        assert_eq!(hit, vec!["c:/users/x".to_string()]);
    }

    #[test]
    fn covering_keys_trailing_slash_drive_root() {
        // 根 key 自带尾部斜杠（`c:/`），后代条目前缀也应命中，而不是拼出 `c://`。
        let keys = vec!["c:/".to_string(), "c:/program files".to_string()];
        let hit = covering_cache_keys(keys.iter(), "c:/program files/edge");
        assert!(hit.contains(&"c:/".to_string()), "盘根应命中: {hit:?}");
    }

    /// 文件粒度 scope 必须在建议统计里出数（回归：总览页对 temp-files 等
    /// 文件粒度 scope 永远统计 0——walk 的文件节点从不匹配，42GB TEMP 建议消失）。
    #[test]
    fn suggestions_from_tree_counts_file_granularity_scopes() {
        // 构造一棵扫描树：C:/Temp 下有两个文件。
        let tree = Node {
            name: "C:".into(),
            path: "C:/".into(),
            is_dir: true,
            size: 0,
            file_count: 0,
            children: vec![Node {
                name: "Temp".into(),
                path: "C:/Temp".into(),
                is_dir: true,
                size: 1024 + 2048,
                file_count: 2,
                children: vec![
                    Node {
                        name: "a.tmp".into(),
                        path: "C:/Temp/a.tmp".into(),
                        is_dir: false,
                        size: 1024,
                        file_count: 0,
                        children: vec![],
                        scaffold_id: None,
                        top_extensions: vec![],
                        children_truncated: None,
                        mtime: None,
                    },
                    Node {
                        name: "b.tmp".into(),
                        path: "C:/Temp/b.tmp".into(),
                        is_dir: false,
                        size: 2048,
                        file_count: 0,
                        children: vec![],
                        scaffold_id: None,
                        top_extensions: vec![],
                        children_truncated: None,
                        mtime: None,
                    },
                ],
                scaffold_id: None,
                top_extensions: vec![],
                children_truncated: None,
                mtime: None,
            }],
            scaffold_id: None,
            top_extensions: vec![],
            children_truncated: None,
            mtime: None,
        };

        // 文件粒度 scaffold：glob 匹配 C:/Temp/** 下的文件。
        let scaffolds = vec![Scaffold {
            id: "system-temp".into(),
            name: "系统临时文件".into(),
            homepage: None,
            risk: Risk::Low,
            disclaimer: "可再生缓存".into(),
            detect: vec![],
            matcher: Default::default(),
            scopes: vec![Scope {
                id: "temp-files".into(),
                label: "临时文件".into(),
                glob: "C:/Temp/**".into(),
                mode: Mode::Recycle,
                prompt: None,
                category: None,
                variant: None,
                recycle_granularity: RecycleGranularity::File,
            }],
        }];

        let out = suggestions_from_tree(&tree, &scaffolds);
        assert_eq!(out.len(), 1, "文件粒度 scope 必须出现在建议里");
        assert_eq!(out[0].scope_id, "temp-files");
        assert_eq!(out[0].bytes, 1024 + 2048, "两个文件字节应累计");
        assert_eq!(out[0].files, 2);
    }

    /// 真实 scaffold 回归：`glob = "%TEMP%/**"` 展开后是 Windows 反斜杠路径，
    /// 而扫描树 `Node.path` 也是反斜杠（匹配前走 walk 转正斜杠）。修复前
    /// build_scope_builds 没规范化展开后的 pattern，分隔符不一致导致真实
    /// system-temp scope 永远 0 字节——这才是「TEMP 42GB 建议消失」的真根因。
    /// 此测试用真实 TOML + 反斜杠路径复现，确保修复后能匹配。
    #[test]
    #[cfg(windows)]
    fn suggestions_from_tree_matches_real_backslash_temp_scope() {
        use diskpilot_scaffold::parse_toml;
        let toml_text = r#"
id = "system-temp"
name = "系统临时文件"
risk = "low"
disclaimer = "可再生缓存"
detect = []
[match]
name_contains = []
must_have_child = []
[[scope]]
id = "temp-files"
label = "临时文件"
glob = "%TEMP%/**"
mode = "recycle"
prompt = { kind = "days", default = 7, label = "清理多少天前的临时文件" }
"#;
        let scaffolds = vec![parse_toml(toml_text).expect("真实 TOML 应能解析")];
        let temp = std::env::var("TEMP").expect("TEMP 环境变量存在");
        // 真实扫描树路径是反斜杠（PathBuf.to_string_lossy 的结果）。
        let file_a = format!("{}\\a.tmp", temp);
        let file_b = format!("{}\\b.tmp", temp);
        let tree = Node {
            name: "C:".into(),
            path: "C:\\".into(),
            is_dir: true,
            size: 0,
            file_count: 0,
            children: vec![Node {
                name: "Temp".into(),
                path: temp.clone(),
                is_dir: true,
                size: 2048,
                file_count: 2,
                children: vec![
                    Node {
                        name: "a.tmp".into(),
                        path: file_a,
                        is_dir: false,
                        size: 1024,
                        file_count: 0,
                        children: vec![],
                        scaffold_id: None,
                        top_extensions: vec![],
                        children_truncated: None,
                        mtime: None,
                    },
                    Node {
                        name: "b.tmp".into(),
                        path: file_b,
                        is_dir: false,
                        size: 1024,
                        file_count: 0,
                        children: vec![],
                        scaffold_id: None,
                        top_extensions: vec![],
                        children_truncated: None,
                        mtime: None,
                    },
                ],
                scaffold_id: None,
                top_extensions: vec![],
                children_truncated: None,
                mtime: None,
            }],
            scaffold_id: None,
            top_extensions: vec![],
            children_truncated: None,
            mtime: None,
        };
        let out = suggestions_from_tree(&tree, &scaffolds);
        assert_eq!(out.len(), 1, "真实 %TEMP% scope 必须出现");
        assert_eq!(out[0].bytes, 2048, "反斜杠 TEMP 文件应匹配 %TEMP%/**");
        assert_eq!(out[0].files, 2);
    }

    /// 真实数据端到端：用 scanner 真实扫描 TEMP（max_depth=1，秒级，只扫顶层文件），
    /// 加载仓库真实 scaffolds，跑 suggestions_from_tree。验证文件粒度 scope 在
    /// 真实扫描树上真的出数——这是「清理从未成功」的最终防线，避免再只靠手搓树
    /// 的测试假阳性。
    #[test]
    #[cfg(windows)]
    fn suggestions_from_tree_e2e_real_temp_scan() {
        use diskpilot_scaffold::load_dir;
        use diskpilot_scanner::{scan_with_stats_cancellable, ScanOptions};
        let temp = std::env::var("TEMP").expect("TEMP 环境变量");
        let opts = ScanOptions {
            max_depth: Some(1),
            keep_files_per_dir: Some(2000),
            ..Default::default()
        };
        let (tree, _) =
            scan_with_stats_cancellable(&temp, opts, None, |_| {}).expect("真实 TEMP 扫描应成功");
        // 找仓库 scaffolds 目录（DISKPILOT_SCAFFOLDS 或相对路径）。
        let scaffolds_dir = std::env::var("DISKPILOT_SCAFFOLDS").unwrap_or_else(|_| {
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../../scaffolds").to_string()
        });
        let scaffolds = load_dir(std::path::Path::new(&scaffolds_dir)).expect("加载真实 scaffolds");
        assert!(
            scaffolds.iter().any(|s| s.id == "system-temp"),
            "应含 system-temp"
        );
        let out = suggestions_from_tree(&tree, &scaffolds);
        // 真实 TEMP 顶层一定有文件，system-temp 的 %TEMP%/** 必须命中至少一个。
        let temp_hit = out
            .iter()
            .find(|s| s.scaffold_id == "system-temp")
            .expect("真实 TEMP 扫描后 system-temp 必须出建议");
        eprintln!(
            "E2E: system-temp 命中 {} 字节 / {} 文件（真实 TEMP 顶层）",
            temp_hit.bytes, temp_hit.files
        );
        assert!(temp_hit.files > 0, "真实 TEMP 文件粒度 scope 必须有命中");
    }
}
