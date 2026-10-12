//! 目录粒度 scope 的单遍合并统计（从 walker.rs 拆分，2026-10-12）。
//! tally_dir_scopes：一次全盘 walk 同时收集 own 字节与 scope glob 命中，
//! 内存树 DP 深→浅累加子树大小，再按 scope 祖先去重聚合。依赖父模块
//! walker 的 diskpilot_walker / path_passes_wxid / path_passes_env /
//! dedup_shallowest / ScopeTotals（子模块可访问父模块私有与 pub 项）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{dedup_shallowest, diskpilot_walker, path_passes_env, path_passes_wxid, ScopeTotals};

/// 低端盘上要几分钟。这里合并为**一次**全盘 walk：
///
/// 1. 遍历时记录每个目录的「直接文件大小之和」(own) 与「自身 mtime」；
///    同时一次性对所有目录 scope 做 glob 匹配，记下命中集合。
/// 2. 内存里按「目录深度 深→浅」做树 DP，算出每个目录的完整子树大小 subtree。
/// 3. 对每个 scope 分别对「全部匹配」「保留期内匹配」两个集合做祖先去重，
///    把去重后每个单元(最浅匹配者)的 subtree 求和 —— 与旧版
///    `find_matching_dirs` + `dir_size_excluding` 的结果完全一致。
///
/// total/eligible 相同（目录粒度不做保留期过滤，详见 Phase 3 注释）。
pub fn tally_dir_scopes<'a, F: Fn(usize) -> &'a globset::GlobSet>(
    root: &Path,
    get_set: F,
    _get_id: impl Fn(usize) -> &'a str,
    dir_indices: &[usize],
    wxid_filter: Option<&[String]>,
    env_filter: Option<&[String]>,
    _scope_days: Option<&HashMap<String, u32>>,
) -> ScopeTotals {
    // ── Phase 1: 一次全盘 walk，收集 own / 命中 scope ──
    let mut own: HashMap<PathBuf, u64> = HashMap::new();
    let mut hits: HashMap<PathBuf, Vec<usize>> = HashMap::new();

    for entry in diskpilot_walker(root).into_iter().flatten() {
        let ft = entry.file_type();
        let path = entry.path();
        if path == root {
            continue;
        }
        if ft.is_file() {
            // 尺寸无条件累加进父目录：目录 scope 一旦命中，统计的是整棵子树
            // （对齐旧版 `dir_size_excluding(d, &[])` 的行为 —— wxid/env
            // 过滤只作用于"哪个目录被命中"，不影响命中目录的内部字节数）。
            if let Some(parent) = path.parent() {
                *own.entry(parent.to_path_buf()).or_insert(0) +=
                    entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
            continue;
        }
        if !ft.is_dir() {
            continue;
        }
        if !path_passes_wxid(&path, wxid_filter) || !path_passes_env(&path, env_filter) {
            continue;
        }
        let path_str = path.to_string_lossy().replace('\\', "/");
        for &i in dir_indices {
            if get_set(i).is_match(&path_str) {
                hits.entry(path.to_path_buf()).or_default().push(i);
            }
        }
    }

    if hits.is_empty() {
        return (
            vec![(0, 0); dir_indices.len()],
            vec![(0, 0); dir_indices.len()],
        );
    }

    // ── Phase 2: 内存树 DP —— 按目录深度 深→浅 累加出 subtree ──
    // 参与 DP 的目录集合 = 所有出现过文件的目录 + 所有命中 scope 的目录 +
    // 它们的全部祖先链（一直推到 root 为止）。不能只取 `own` 的 key：
    // 纯中间目录（只有子目录、没有直接文件的）也必须出现在列表里，否则
    // 它的子树结果不会向上传播，祖先命中时的尺寸会被少算。
    let mut dirs: Vec<PathBuf> = Vec::new();
    for p in own.keys().chain(hits.keys()) {
        let mut cur = p.as_path();
        while cur != root {
            dirs.push(cur.to_path_buf());
            match cur.parent() {
                Some(par) => cur = par,
                None => break,
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    let mut subtree: HashMap<PathBuf, u64> = HashMap::new();
    for p in dirs {
        let acc = subtree.entry(p.clone()).or_insert(0);
        *acc = acc.saturating_add(own.get(&p).copied().unwrap_or(0));
        let summed = *acc;
        if let Some(parent) = p.parent() {
            *subtree.entry(parent.to_path_buf()).or_insert(0) += summed;
        }
    }

    // ── Phase 3: 每个 scope 单独聚合（total / eligible 各自去重) ──
    // 返回向量按 dir_indices 的**顺序/长度**紧凑对齐（pos = 0..dir_indices.len()），
    // 由调用方在写回 total/tally 时映射回全局下标。
    //
    // 目录粒度 scope 不做「N 天前」过滤：目录 mtime 是"最后一次写入"时间，
    // 装包/写缓存即刷新，按它判断缓存"旧不旧"语义不成立，且会让建议显示
    // 可清、执行却 0 匹配（"清理了 0b"）。缓存目录整体可再生，清理是安全
    // 的；文件粒度 scope 的 scope_days 保留原过滤逻辑（微信图片/临时文件按天）。
    let mut total: Vec<(u64, u64)> = vec![(0, 0); dir_indices.len()];
    let mut tally: Vec<(u64, u64)> = vec![(0, 0); dir_indices.len()];
    for (pos, &i) in dir_indices.iter().enumerate() {
        let mut m_all: Vec<PathBuf> = Vec::new();
        for (p, idxs) in &hits {
            if !idxs.contains(&i) {
                continue;
            }
            m_all.push(p.clone());
        }
        m_all.sort_by_key(|p| p.components().count());
        let all = dedup_shallowest(m_all);
        total[pos] = (
            all.iter()
                .map(|p| subtree.get(p).copied().unwrap_or(0))
                .sum(),
            all.len() as u64,
        );
        tally[pos] = total[pos];
    }
    (total, tally)
}
