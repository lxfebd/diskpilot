//! walker.rs 单元测试（从 walker.rs 拆分，2026-10-12）。
//! 目录 scope 单遍合并统计 / 树上目录+文件匹配 / 剪枝与 mtime 过滤。

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
    let got = find_matching_files_on_tree(&tree, &set, Some(&["wxid_ok".to_string()]), None, None);
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
