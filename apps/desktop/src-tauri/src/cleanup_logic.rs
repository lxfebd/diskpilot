//! 清理建议领域逻辑：scope 统计 / 内存树建议 / 缓存失效。
//!
//! 从 `cleanup.rs` 拆分（2026-10-12）：与 Tauri 命令层解耦，便于独立
//! 测试。命令层（cleanup_suggestions / chat_scan_context / scope_sizes_batch）
//! 留在 cleanup.rs。

use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diskpilot_scaffold::{expand_env, RecycleGranularity, Scaffold};
use diskpilot_scanner::{
    diskpilot_walker, mtime_older_than, path_passes_env, path_passes_wxid, tally_dir_scopes, Node,
};

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
    // 定位用户名所在段：Windows 盘符或 POSIX 根后跟 Users → 段 2；
    // 无盘符的裸 `Users/<名>` → 段 1；都不像则原样返回。
    let user_idx = if parts
        .get(1)
        .map(|s| s.eq_ignore_ascii_case("users"))
        .unwrap_or(false)
        && ((parts[0].len() == 2 && parts[0].ends_with(':')) || parts[0].is_empty())
    {
        Some(2)
    } else if parts
        .first()
        .map(|s| s.eq_ignore_ascii_case("users"))
        .unwrap_or(false)
    {
        Some(1)
    } else {
        None
    };
    match user_idx {
        Some(i) if parts.len() > i => {
            let mut out = parts;
            out[i] = "<USER>";
            out.join("/")
        }
        _ => s,
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
            });
        }
    }
    if rows.is_empty() {
        return Vec::new();
    }
    let mut tally: Vec<(u64, u64)> = vec![(0, 0); rows.len()];

    // 全部 scope（目录+文件粒度）合并进一个 GlobSet：每个节点一次
    // `matches(path)` 拿到全部命中的 scope 下标，替代「目录节点 × 目录粒度
    // scope 的逐行 is_match」（60 万文件的大树上 10 万目录 × 40 scope 曾实测
    // 50 秒，是总览建议卡「正在检查」卡半分钟的元凶）。两种粒度的 glob 构造
    // 参数一致（build_scope_builds 与下方重建均 literal_separator(false) +
    // case_insensitive(true)），合并后匹配语义逐条等价。
    let mut merged_builder = globset::GlobSetBuilder::new();
    for r in &rows {
        let g = globset::GlobBuilder::new(&r.pattern)
            .literal_separator(false)
            .case_insensitive(true)
            .build()
            .expect("pattern 已由 build_scope_builds 验证，重建不应失败");
        merged_builder.add(g);
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
    ) {
        if node.is_dir {
            let path_str = node.path.replace('\\', "/");
            let mut newly = Vec::new();
            for mi in merged.matches(&path_str) {
                let i = mi; // 合并集下标 = rows 下标（add 顺序即 rows 顺序）
                if rows[i].is_dir && !claims.contains(&i) {
                    tally[i].0 = tally[i].0.saturating_add(node.size);
                    tally[i].1 = tally[i].1.saturating_add(1);
                    newly.push(i);
                }
            }
            claims.extend_from_slice(&newly);
            for c in &node.children {
                walk(c, rows, tally, claims, merged);
            }
            claims.truncate(claims.len() - newly.len());
        } else {
            // 文件节点：一次合并集匹配拿到全部命中的文件粒度 scope。
            // 目录粒度 scope 由祖先目录承担（DFS 在目录节点上已命中，不在
            // 文件节点重复）；文件粒度每个文件独立匹配，不参与 claims。
            if node.size == 0 {
                return;
            }
            let path_str = node.path.replace('\\', "/");
            for mi in merged.matches(&path_str) {
                let i = mi;
                if !rows[i].is_dir {
                    tally[i].0 = tally[i].0.saturating_add(node.size);
                    tally[i].1 = tally[i].1.saturating_add(1);
                }
            }
        }
    }
    // 与 walk 版一致：跳过 root 自身的命中，只统计其子树。
    for child in &root.children {
        walk(child, &rows, &mut tally, &mut Vec::new(), &merged);
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
    pub(crate) scaffold_id: String,
    pub(crate) scope_id: String,
    pub(crate) label: String,
    pub(crate) desc: String,
    /// 可清理内容的真实锚点目录（scope glob 展开后的实际路径）。
    /// AI 出清单时直接引用这个路径，不得自行发明（否则会指向
    /// `C:\Users\Public\...` 这类不存在的目录 → 执行 0 B）。
    pub(crate) path: String,
    pub(crate) files: u64,
    pub(crate) total_bytes: u64,
}
