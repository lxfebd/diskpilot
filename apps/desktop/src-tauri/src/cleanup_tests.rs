//! cleanup.rs 单元测试（从 cleanup.rs 拆分，2026-10-12）。
//! 脱敏 / covering keys / 树建议文件粒度与真实临时目录 e2e。

use super::*;
use crate::cleanup_logic::covering_cache_keys;
use diskpilot_scaffold::{Mode, Risk, Scaffold, Scope};

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
    let keys = [
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
    let keys = ["c:/users/x".to_string(), "c:/users/x/sub".to_string()];
    let hit = covering_cache_keys(keys.iter(), "c:/users/x");
    assert_eq!(hit, vec!["c:/users/x".to_string()]);
}

#[test]
fn covering_keys_trailing_slash_drive_root() {
    // 根 key 自带尾部斜杠（`c:/`），后代条目前缀也应命中，而不是拼出 `c://`。
    let keys = ["c:/".to_string(), "c:/program files".to_string()];
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
        tags: Vec::new(),
        scopes: vec![Scope {
            id: "temp-files".into(),
            label: "临时文件".into(),
            glob: "C:/Temp/**".into(),
            mode: Mode::Recycle,
            prompt: None,
            category: None,
            variant: None,
            recycle_granularity: RecycleGranularity::File,
            recommended_selected: None,
            required_stopped_processes: None,
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
    let scaffolds_dir = std::env::var("DISKPILOT_SCAFFOLDS")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../../scaffolds").to_string());
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
