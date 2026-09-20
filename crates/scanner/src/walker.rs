//! 共享的目录 walk 原语（W5：walk 单一归属）。
//!
//! 这里收编了原 desktop `lib.rs` 里的 scaffold 侧扫描工具——desktop 的
//! cleanup 估算 / 回收计划与 scanner 的磁盘扫描此前各自维护一份剪枝黑名单
//! 和 walker 构造器，规则漂移是真实的 bug 源（回收计划撞上回收站子树）。
//! 现在两条线共用：
//!
//! - `PRUNED_SYSTEM_DIRS`：系统级垃圾/卷元数据目录名（整树跳过）。
//! - `diskpilot_walker`：scaffold 清理侧 walker（进 dotted 目录、prune 上表）。
//! - `find_matching_dirs` / `tally_dir_scopes`：目录粒度 scope 的匹配与
//!   单遍合并统计（祖先去重 + 树 DP）。
//!
//! scanner 主扫描（`scan_with_stats*`）的 Node tree 也走同一份黑名单
//! （`is_pruned_system_dir_at` 额外处理 `windows\installer` 父目录上下文）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::Node;

/// 目录粒度 scope 合并统计的输出：每 scope 的 (total_bytes, eligible_bytes)。
pub type ScopeTotals = (Vec<(u64, u64)>, Vec<(u64, u64)>);

/// 系统级"垃圾/卷元数据"目录名（lowercase，比较时不区分大小写）。
/// 不只是性能：让回收站进 Node tree 会被前端 fallback 的 name_contains
/// 当成"伪 app 数据 root"——用户曾把 xwechat_files 删进回收站后，
/// `C:\$Recycle.Bin\<SID>\$R*\xwechat_files` 会被识别为活的 WeChat 数据
/// 触发误删。System Volume Information 同时还能避开 VSS 权限拒绝噪音。
/// WinSxS 是几十 GB 级系统组件存储，扫进来既慢又不可清理。
const PRUNED_SYSTEM_DIRS: &[&str] = &[
    "$recycle.bin",
    "system volume information",
    ".trash",
    ".trashes",
    "winsxs",
];

/// 纯目录名剪枝判断（walker 的 `process_read_dir` 里只有 `file_name`，
/// 没有父目录上下文，所以这里不带 installer 特判）。
pub fn is_pruned_system_dir(name: &std::ffi::OsStr) -> bool {
    let Some(s) = name.to_str() else { return false };
    let lower = s.to_ascii_lowercase();
    PRUNED_SYSTEM_DIRS.iter().any(|p| *p == lower)
}

/// 带父目录上下文的剪枝判断。`Windows\Installer` 是几十 GB 的系统组件
/// 存储，必须跳过；但 `installer` 作为目录名在别处（下载安装包、项目
/// 脚手架）很常见，任意层级都拦会把用户自己的目录静默漏出扫描，所以
/// 只对父目录为 `windows` 的 installer 生效。
pub fn is_pruned_system_dir_at(parent: &std::ffi::OsStr, name: &std::ffi::OsStr) -> bool {
    let Some(s) = name.to_str() else { return false };
    let lower = s.to_ascii_lowercase();
    if lower == "installer" {
        return parent
            .to_str()
            .map(|p| p.eq_ignore_ascii_case("windows"))
            .unwrap_or(false);
    }
    PRUNED_SYSTEM_DIRS.iter().any(|p| *p == lower)
}

/// scaffold 侧路径扫描共用的 walker 构造器。两条策略集中在这里：
/// (a) `skip_hidden(false)`——很多 app cache 落在 dotted 目录里，必须能进；
/// (b) `process_read_dir` 在读目录时直接 prune 系统垃圾箱/卷元数据子树，
///     比"扫完再过滤路径"省一个数量级 IO，并彻底排除"glob 撞回收站"事故面。
pub fn diskpilot_walker(root: &Path) -> jwalk::WalkDir {
    jwalk::WalkDir::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .process_read_dir(|_, _, _, children| {
            children.retain(|res| {
                let Ok(entry) = res else { return true };
                if !entry.file_type.is_dir() {
                    return true;
                }
                !is_pruned_system_dir(&entry.file_name)
            });
        })
}

/// Walk `root` and return directories whose path matches `glob_set`, after
/// pruning any candidate whose ancestor is also matched. Used by directory-
/// granularity scopes (recycle the directory as one unit, not file-by-file).
///
/// **Why ancestor dedup**: globset is configured with `literal_separator(false)`
/// for backwards compat with media-bucket file scopes — meaning `*` crosses
/// `/`. A glob like `**/pkgs/*` matches both `pkgs/numpy` AND `pkgs/numpy/info`;
/// dedup keeps only the shallowest match per subtree so the recycle plan
/// touches each logical unit exactly once. `path == root` is dropped
/// unconditionally — even if a misconfigured glob hits root, recycling the
/// scan root would nuke the user's whole conda install.
pub fn find_matching_dirs(
    root: &Path,
    glob_set: &globset::GlobSet,
    wxid_filter: Option<&[String]>,
    env_filter: Option<&[String]>,
    older_than_days: Option<u32>,
) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in diskpilot_walker(root).into_iter().flatten() {
        if !entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        if path == root {
            continue;
        }
        if !path_passes_wxid(&path, wxid_filter) || !path_passes_env(&path, env_filter) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !mtime_older_than(&metadata, older_than_days) {
            continue;
        }
        let path_str = path.to_string_lossy().replace('\\', "/");
        if glob_set.is_match(&path_str) {
            candidates.push(path);
        }
    }
    candidates.sort_by_key(|p| p.as_os_str().len());
    dedup_shallowest(candidates)
}

/// Keep only the shallowest ancestor per subtree (path with the fewest
/// components).
pub fn dedup_shallowest(candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut keep: Vec<PathBuf> = Vec::with_capacity(candidates.len());
    for c in candidates {
        if !keep.iter().any(|k| c.starts_with(k)) {
            keep.push(c);
        }
    }
    keep
}

// ── 扫描树上的目录匹配（execute_scope dry_run 免二次全盘）───────────────
// Node 树在扫描时已剪枝系统目录、目录 children 完整（文件列表受 top-K），
// 目录粒度的 glob 匹配可以完全在内存树上做。树上没有 mtime，调用方必须
// 自行保证 older_than_days 为 None；实际执行（dry_run=false）永远走真实
// 文件系统，删除决策不依赖可能过期的快照。

fn norm_rel(p: &str) -> String {
    p.to_lowercase()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string()
}

/// 在扫描树里定位 `target` 路径对应的子树节点（大小写/分隔符不敏感，
/// 逐段下钻）。`target` 必须等于 `root.path` 或严格位于其之下。
pub fn locate_subtree<'a>(root: &'a Node, target: &Path) -> Option<&'a Node> {
    let root_key = norm_rel(&root.path);
    let target_key = norm_rel(&target.to_string_lossy());
    if root_key == target_key {
        return Some(root);
    }
    // target 必须在 root 之下（前缀 + '/' 边界），剩余段逐层匹配 children
    let rest = target_key
        .strip_prefix(&root_key)
        .filter(|r| r.starts_with('/'))?;
    let mut cur = root;
    for seg in rest.split('/').filter(|s| !s.is_empty()) {
        let mut next = None;
        for c in &cur.children {
            if c.is_dir && c.name.eq_ignore_ascii_case(seg) {
                next = Some(c);
                break;
            }
        }
        cur = next?;
    }
    Some(cur)
}

/// `find_matching_dirs` 的扫描树版本：从 `root_node`（不与 walk 版的 `root`
/// 参数对应——这里是**已定位的子树根**，其自身天然不参与匹配）递归目录
/// children，语义逐条对齐 walk 版：
/// - wxid/env 过滤不过 → 不收集但**继续深入**其子目录；
/// - glob 命中 → 收集且不再深入（等价 walk 版"全收集再 dedup_shallowest"，
///   祖先命中后后代必然被去重丢弃）；
/// - 最后按路径长度排序 + dedup_shallowest 兜底，与 walk 版产出形态一致。
pub fn find_matching_dirs_on_tree(
    root_node: &Node,
    glob_set: &globset::GlobSet,
    wxid_filter: Option<&[String]>,
    env_filter: Option<&[String]>,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    fn walk(
        node: &Node,
        out: &mut Vec<PathBuf>,
        glob_set: &globset::GlobSet,
        wxid_filter: Option<&[String]>,
        env_filter: Option<&[String]>,
    ) {
        for c in &node.children {
            if !c.is_dir {
                continue;
            }
            let p = PathBuf::from(&c.path);
            if !path_passes_wxid(&p, wxid_filter) || !path_passes_env(&p, env_filter) {
                walk(c, out, glob_set, wxid_filter, env_filter);
                continue;
            }
            let s = c.path.replace('\\', "/");
            if glob_set.is_match(&s) {
                out.push(p);
            } else {
                walk(c, out, glob_set, wxid_filter, env_filter);
            }
        }
    }
    walk(root_node, &mut out, glob_set, wxid_filter, env_filter);
    out.sort_by_key(|p| p.as_os_str().len());
    dedup_shallowest(out)
}

/// 树上文件匹配能否给出**完整**裁决：子树内无任何文件 top-K 截断标记，
/// 且要求 mtime（带天数过滤）时每个文件节点都携带时间戳。
///
/// 树不是扫描仪镜像（截断目录的文件清单不完整、MFT/serializer 路径可能缺
/// mtime），用它做文件粒度匹配只会漏报——dry-run 预览漏报会把「用户确认的
/// 清单」与「真实执行的集合」拉开（执行重新 walk 会多删）。因此调用方在
/// 用 `find_matching_files_on_tree` 的结果决定展示前，必须先过这道闸；不过
/// 闸就回落真实 walk，行为与旧版一致。
pub fn tree_files_adjudicable(root_node: &Node, require_mtimes: bool) -> bool {
    if root_node.children_truncated.is_some() {
        return false;
    }
    for c in &root_node.children {
        if c.is_dir {
            if !tree_files_adjudicable(c, require_mtimes) {
                return false;
            }
        } else if require_mtimes && c.mtime.is_none() {
            return false;
        }
    }
    true
}

/// 树上的**文件粒度** glob 匹配（execute_scope dry_run 免二次全盘的补充）。
///
/// 与 `find_matching_dirs_on_tree` 的关系：目录粒度匹配收集「目录树」，这里
/// 收集「文件树」。语义与 walk 版 `find_matching_dirs` 的 File 分支对齐：
/// - 只匹配文件节点（`!is_dir`）；
/// - 递归深入所有目录节点；
/// - wxid/env 过滤不过的路径跳过；
/// - `older_than_days`：文件 mtime 缺失（`None`）时**跳过该文件**（保守——
///   树上没有时间戳就无法证明它够旧，宁可漏匹配不可误删新文件）；
/// - glob 命中 → 收集。
///
/// 关键不变量：**树上文件列表受 keep_files_per_dir top-K 截断**，截断目录
/// （`children_truncated.is_some()`）的文件清单不完整，在此树上匹配会漏报。
/// 因此凡遇到 `children_truncated.is_some()` 的目录节点，**不信任其文件
/// children、不纳入匹配**。调用方不应依赖本函数在截断/缺 mtime 树上的
/// 结果做展示——先用 `tree_files_adjudicable` 确认子树完整，否则回落
/// 真实 walk。
pub fn find_matching_files_on_tree(
    root_node: &Node,
    glob_set: &globset::GlobSet,
    wxid_filter: Option<&[String]>,
    env_filter: Option<&[String]>,
    older_than_days: Option<u32>,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    fn walk(
        node: &Node,
        out: &mut Vec<PathBuf>,
        glob_set: &globset::GlobSet,
        wxid_filter: Option<&[String]>,
        env_filter: Option<&[String]>,
        older_than_days: Option<u32>,
    ) {
        // 目录自身被截断 → 文件清单不完整，不信任此层文件（仍深入子目录，
        // 子目录的 top-K 是独立的）。
        let files_trusted = node.children_truncated.is_none();
        for c in &node.children {
            let p = PathBuf::from(&c.path);
            if !path_passes_wxid(&p, wxid_filter) || !path_passes_env(&p, env_filter) {
                continue;
            }
            if !c.is_dir {
                if !files_trusted {
                    continue;
                }
                if let Some(days) = older_than_days {
                    let Some(mt) = c.mtime else {
                        continue; // 无时间戳无法证明够旧，保守跳过
                    };
                    if !mtime_older_than_t(
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(mt),
                        days,
                    ) {
                        continue;
                    }
                }
                let s = c.path.replace('\\', "/");
                if glob_set.is_match(&s) {
                    out.push(p);
                }
            } else {
                walk(c, out, glob_set, wxid_filter, env_filter, older_than_days);
            }
        }
    }
    walk(
        root_node,
        &mut out,
        glob_set,
        wxid_filter,
        env_filter,
        older_than_days,
    );
    out
}

/// True when `metadata.modified()` is older than `now - days * 86400s`.
/// `days = None` skips the filter. Files whose mtime can't be read pass —
/// we don't silently drop data the user expects to see because of a transient
/// OS error.
pub fn mtime_older_than(metadata: &std::fs::Metadata, days: Option<u32>) -> bool {
    let Some(d) = days else { return true };
    let Ok(modified) = metadata.modified() else {
        return true;
    };
    mtime_older_than_t(modified, d)
}

/// `mtime_older_than` 的 SystemTime 直通版本（已读过 metadata 时避免重建）。
pub fn mtime_older_than_t(modified: SystemTime, days: u32) -> bool {
    let Some(threshold) = SystemTime::now().checked_sub(Duration::from_secs(days as u64 * 86_400))
    else {
        return false;
    };
    modified <= threshold
}

// wxid / env 路径过滤是 desktop scaffold 配置的概念，但 walk 原语与它们
// 绑定得最紧（匹配前过滤），随函数一起收编，避免跨 crate 回调。

/// True when `path`'s first `wxid_*` segment is in `allow`. Paths with no
/// `wxid_*` segment (e.g. `all_users/`, `%APPDATA%/Tencent/xwechat/log/`)
/// always pass — those are cross-account or roaming-only data that aren't
/// wxid-scoped. `None` or an empty allow-list disables the filter entirely.
pub fn path_passes_wxid(path: &Path, wxid_filter: Option<&[String]>) -> bool {
    let Some(allowed) = wxid_filter else {
        return true;
    };
    if allowed.is_empty() {
        return true;
    }
    for component in path.components() {
        if let Some(s) = component.as_os_str().to_str() {
            if s.starts_with("wxid_") {
                return allowed.iter().any(|w| w == s);
            }
        }
    }
    true
}

/// True when `path` has an `envs/<name>` segment whose `<name>` is in `allow`,
/// OR has no `envs/` segment at all (paths from non-env scopes pass through —
/// `pkgs/cache/foo`, `<conda-root>/python.exe`, etc.). Mirrors `path_passes_wxid`'s
/// "filter only narrows the targeted layer" semantics so a single
/// `execute_scope` call can carry both filters across mixed scopes.
/// `None` or empty allow-list disables the filter entirely.
pub fn path_passes_env(path: &Path, env_filter: Option<&[String]>) -> bool {
    let Some(allowed) = env_filter else {
        return true;
    };
    if allowed.is_empty() {
        return true;
    }
    let mut comps = path.components().peekable();
    while let Some(c) = comps.next() {
        if let Some(s) = c.as_os_str().to_str() {
            if s.eq_ignore_ascii_case("envs") {
                if let Some(next) = comps.peek() {
                    if let Some(name) = next.as_os_str().to_str() {
                        return allowed.iter().any(|n| n == name);
                    }
                }
                return false;
            }
        }
    }
    true
}

/// 目录粒度 scope 的单遍合并统计。
///
/// 旧实现里每个目录 scope 单独 `find_matching_dirs` 全盘走一遍、再对匹配目录
/// 逐个 `dir_size_excluding` 子树求和 —— N 个目录 scope = 2N 次全盘读取，
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tempdir_path() -> PathBuf {
        // 进程内原子计数器保证并发测试线程不撞名（SystemTime 精度在 Windows
        // 上不够用，见 lib.rs 同名 helper 的注释）。
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "diskpilot-walker-test-{}-{}",
            std::process::id(),
            seq
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn set_of(glob: &str) -> globset::GlobSet {
        let mut b = globset::GlobSetBuilder::new();
        b.add(
            globset::GlobBuilder::new(glob)
                .literal_separator(false)
                .case_insensitive(true)
                .build()
                .unwrap(),
        );
        b.build().unwrap()
    }

    /// 构造 pkgs/{numpy/{a.txt=100B, info/b.txt=50B}, requests/c.txt=30B}。
    fn pkg_tree(root: &Path) {
        let numpy = root.join("pkgs").join("numpy");
        let info = numpy.join("info");
        let requests = root.join("pkgs").join("requests");
        fs::create_dir_all(&info).unwrap();
        fs::create_dir_all(&requests).unwrap();
        fs::write(numpy.join("a.txt"), vec![0u8; 100]).unwrap();
        fs::write(info.join("b.txt"), vec![0u8; 50]).unwrap();
        fs::write(requests.join("c.txt"), vec![0u8; 30]).unwrap();
    }

    #[test]
    fn tally_dir_scopes_sums_subtrees_and_dedups_ancestors() {
        let tmp = tempdir_path();
        let root = tmp.clone();
        pkg_tree(&root);

        // scope0 `**/pkgs/*`（`*` 跨分隔符）同时命中 numpy、numpy/info、requests；
        // 祖先去重后只保留 numpy + requests —— info 的 50B 由 numpy 的子树带出，
        // 不能重复计入。scope1 `**/info` 只命中叶子。
        let sets = [set_of("**/pkgs/*"), set_of("**/info")];
        let indices = [0usize, 1];
        let ids = ["pkgs", "info"];
        let (total, tally) =
            tally_dir_scopes(&root, |i| &sets[i], |i| ids[i], &indices, None, None, None);
        assert_eq!(
            total[0],
            (180, 2),
            "pkgs scope = 180 B，去重后 2 个单元(numpy+requests)"
        );
        assert_eq!(total[1], (50, 1), "info scope = 50 B / 1 个单元");
        // days = None 时 eligible 与 total 完全一致
        assert_eq!(tally, total);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tally_dir_scopes_ignores_retention_days() {
        // 目录粒度 scope 不做「N 天前」过滤：目录 mtime = 最后写入时间，
        // 按它判断"缓存旧不旧"语义不成立，且会导致建议显示可清、执行却
        // 0 匹配（"清理了 0b"）。目录整体可再生，清理按全量口径统计，
        // eligible 必须与 total 一致（与 find_matching_dirs 执行侧对齐）。
        let tmp = tempdir_path();
        let root = tmp.clone();
        pkg_tree(&root);

        let sets = [set_of("**/pkgs/*")];
        let indices = [0usize];
        let ids = ["pkgs"];
        // 刚写入的文件 mtime 是现在，即使设 1 天保留期也全部计入（不按
        // 目录 mtime 过滤）。
        let mut days = std::collections::HashMap::new();
        days.insert("pkgs".to_string(), 1u32);
        let (total, tally) = tally_dir_scopes(
            &root,
            |i| &sets[i],
            |i| ids[i],
            &indices,
            None,
            None,
            Some(&days),
        );
        assert_eq!(total[0], (180, 2), "total 不受保留期影响");
        assert_eq!(tally[0], total[0], "目录粒度忽略 days，eligible == total");

        // days = 0 同样不影响
        let mut days0 = std::collections::HashMap::new();
        days0.insert("pkgs".to_string(), 0u32);
        let (_total, tally0) = tally_dir_scopes(
            &root,
            |i| &sets[i],
            |i| ids[i],
            &indices,
            None,
            None,
            Some(&days0),
        );
        assert_eq!(tally0[0], (180, 2));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_matching_dirs_dedups_to_shallowest_and_never_returns_root() {
        let tmp = tempdir_path();
        let root = tmp.clone();
        pkg_tree(&root);

        // `**` 连 root 也命中，但 path == root 被无条件丢弃；剩余目录去重到
        // 最浅的 pkgs 一个单元。
        let all = set_of("**");
        let got = find_matching_dirs(&root, &all, None, None, None);
        assert_eq!(got, vec![root.join("pkgs")]);

        // 精确 glob 保留每个叶子
        let info = set_of("**/info");
        let got = find_matching_dirs(&root, &info, None, None, None);
        assert_eq!(got, vec![root.join("pkgs").join("numpy").join("info")]);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn mtime_older_than_handles_none_and_fresh_files() {
        let tmp = tempdir_path();
        let f = tmp.join("fresh.txt");
        fs::write(&f, b"x").unwrap();
        let md = fs::metadata(&f).unwrap();
        assert!(mtime_older_than(&md, None), "days=None 跳过过滤");
        assert!(!mtime_older_than(&md, Some(1)), "刚写入的文件不在 1 天之前");
        assert!(mtime_older_than(&md, Some(0)), "days=0 一切都算过期");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ── 扫描树上的目录匹配（find_matching_dirs_on_tree / locate_subtree）──

    fn node(path: &str, name: &str, is_dir: bool, children: Vec<Node>) -> Node {
        Node {
            path: path.into(),
            name: name.into(),
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

    /// 内存树：C:/ → pkgs → { numpy → { info }, requests }，另有 wxid_bad/outer。
    fn mem_tree() -> Node {
        node(
            "C:/",
            "C:/",
            true,
            vec![
                node(
                    "C:/pkgs",
                    "pkgs",
                    true,
                    vec![
                        node(
                            "C:/pkgs/numpy",
                            "numpy",
                            true,
                            vec![node("C:/pkgs/numpy/info", "info", true, vec![])],
                        ),
                        node("C:/pkgs/requests", "requests", true, vec![]),
                    ],
                ),
                node(
                    "C:/wxid_bad",
                    "wxid_bad",
                    true,
                    vec![node(
                        "C:/wxid_bad/outer",
                        "outer",
                        true,
                        vec![node("C:/wxid_bad/outer/deep", "deep", true, vec![])],
                    )],
                ),
            ],
        )
    }

    #[test]
    fn locate_subtree_matches_case_insensitively_and_rejects_outside() {
        let tree = mem_tree();
        // 大小写/反斜杠变体命中
        let hit = locate_subtree(&tree, Path::new("c:\\PKGS\\numpy"));
        assert_eq!(hit.map(|n| n.path.as_str()), Some("C:/pkgs/numpy"));
        // 树根自身
        let hit = locate_subtree(&tree, Path::new("C:/"));
        assert!(hit.is_some());
        // 不在树下的路径
        assert!(locate_subtree(&tree, Path::new("C:/other")).is_none());
        assert!(locate_subtree(&tree, Path::new("D:/pkgs")).is_none());
    }

    #[test]
    fn tree_matching_dedups_shallowest_and_respects_filters() {
        let tree = mem_tree();
        // `**/pkgs/*`：numpy、info、requests 都字面命中，但 numpy 命中后不再
        // 深入 → info 不出现；requests 是独立单元保留。
        let set = set_of("**/pkgs/*");
        let got = find_matching_dirs_on_tree(&tree, &set, None, None);
        assert_eq!(
            got,
            vec![
                PathBuf::from("C:/pkgs/numpy"),
                PathBuf::from("C:/pkgs/requests")
            ],
            "祖先命中后后代不重复，兄弟单元保留"
        );

        // wxid 过滤：allow 只含 wxid_ok → wxid_bad 子树一个都不收集
        let allow = vec!["wxid_ok".to_string()];
        let got = find_matching_dirs_on_tree(&tree, &set, Some(&allow), None);
        assert!(
            got.iter().all(|p| !p.starts_with("C:/wxid_bad")),
            "wxid 不过滤的子树不收集，got: {got:?}"
        );

        // env 过滤透传：无 envs 段的路径全部通过
        let got = find_matching_dirs_on_tree(&tree, &set, None, Some(&["base".to_string()]));
        assert_eq!(got.len(), 2);
    }

    // ── 树上的文件匹配（find_matching_files_on_tree）──

    /// 文件节点：path/name + 可选 mtime（秒）。size 只影响展示，匹配不看它。
    fn file_node(path: &str, name: &str, mtime: Option<u64>) -> Node {
        let mut n = node(path, name, false, vec![]);
        n.mtime = mtime;
        n
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    /// 内存文件树：C:/cache → { logs: { app.log(旧), new.log(无 mtime) },
    /// keep(截断): { data.bin } }。
    fn mem_file_tree() -> Node {
        let mut keep = node(
            "C:/cache/keep",
            "keep",
            true,
            vec![file_node(
                "C:/cache/keep/data.bin",
                "data.bin",
                Some(now_secs()),
            )],
        );
        keep.children_truncated = Some(3); // top-K 截断 → 此目录文件清单不可信
        node(
            "C:/cache",
            "cache",
            true,
            vec![
                node(
                    "C:/cache/logs",
                    "logs",
                    true,
                    vec![
                        file_node(
                            "C:/cache/logs/app.log",
                            "app.log",
                            Some(now_secs() - 10 * 86400),
                        ),
                        file_node("C:/cache/logs/new.log", "new.log", None),
                    ],
                ),
                keep,
            ],
        )
    }

    #[test]
    fn file_tree_matching_matches_leaf_files_recursively() {
        let tree = mem_file_tree();
        let set = set_of("**/*.log");
        let got = find_matching_files_on_tree(&tree, &set, None, None, None);
        assert_eq!(
            got,
            vec![
                PathBuf::from("C:/cache/logs/app.log"),
                PathBuf::from("C:/cache/logs/new.log")
            ]
        );
    }

    #[test]
    fn file_tree_matching_skips_truncated_dirs_files() {
        let tree = mem_file_tree();
        // data.bin 在截断目录 keep 下：即使 glob 命中也不得收集
        let set = set_of("**/*.bin");
        let got = find_matching_files_on_tree(&tree, &set, None, None, None);
        assert!(
            got.is_empty(),
            "截断目录的文件清单不可信，整目录跳过: {got:?}"
        );
    }

    #[test]
    fn file_tree_matching_requires_mtime_for_days_filter() {
        let tree = mem_file_tree();
        let set = set_of("**/*.log");
        // 10 天前的 app.log 够旧 → 收集；new.log 缺 mtime → 保守跳过
        let got = find_matching_files_on_tree(&tree, &set, None, None, Some(5));
        assert_eq!(got, vec![PathBuf::from("C:/cache/logs/app.log")]);
    }

    #[test]
    fn file_tree_matching_respects_wxid_filter() {
        let tree = mem_file_tree();
        let set = set_of("**/*.log");
        // wxid_ok 过滤：路径无 wxid_* 段 → 全部通过
        let got =
            find_matching_files_on_tree(&tree, &set, Some(&["wxid_ok".to_string()]), None, None);
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn tree_files_adjudicable_rejects_truncation_and_missing_mtime() {
        // 无截断、文件全带 mtime → 两种要求都可裁决
        let full = node(
            "C:/cache",
            "cache",
            true,
            vec![node(
                "C:/cache/logs",
                "logs",
                true,
                vec![
                    file_node(
                        "C:/cache/logs/app.log",
                        "app.log",
                        Some(now_secs() - 10 * 86400),
                    ),
                    file_node("C:/cache/logs/fresh.log", "fresh.log", Some(now_secs())),
                ],
            )],
        );
        assert!(tree_files_adjudicable(&full, false));
        assert!(tree_files_adjudicable(&full, true));

        // 无截断但一个文件缺 mtime → 只在不要求 mtime 时可裁决
        let mut partial = full.clone();
        partial.children[0].children[1].mtime = None;
        assert!(tree_files_adjudicable(&partial, false));
        assert!(!tree_files_adjudicable(&partial, true));

        // 任意层级有截断 → 永远不可裁决
        let mut truncated = full.clone();
        truncated.children_truncated = Some(2);
        assert!(!tree_files_adjudicable(&truncated, false));
        // 深层截断同样挡住
        let mut deep = full.clone();
        deep.children[0].children_truncated = Some(2);
        assert!(!tree_files_adjudicable(&deep, false));
    }
}
