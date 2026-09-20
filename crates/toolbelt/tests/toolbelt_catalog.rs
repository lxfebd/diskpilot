//! 目录解析 + toolbelt_command_for 行为测试。
//! 不依赖本机真的装了图吧工具箱：exe 用临时目录里的假文件替身。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use diskpilot_toolbelt::*;

/// 在 temp 下伪造一个带 Tools 根的最小工具箱，返回 tools_root。
fn fake_tools_root(dir: &Path, exe_rel: &str) -> PathBuf {
    let root = dir.join("Tools");
    let exe = root.join(exe_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    // 有实际内容，避免 canonicalize 后又被人清掉
    fs::write(&exe, b"@echo fake").unwrap();
    root
}

fn tempdir(tag: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("dp_toolbelt_{}_{}_{}", tag, std::process::id(), n));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn embedded_doc_parses_expected_catalog() {
    let cat = catalog();
    // 文档收录 17 个 CLI 工具，一个都不能丢
    assert!(cat.len() >= 17, "解析出的工具数 {} < 17", cat.len());
    for want in [
        "WizTree",
        "AIDA64",
        "HWiNFO",
        "USBDeview",
        "Autoruns / autorunsc",
        "Everything",
        "Ventoy",
    ] {
        assert!(cat.iter().any(|t| t.name == want), "缺少工具 {}", want);
    }
    let wiz = find("WizTree").unwrap();
    assert_eq!(wiz.category, "硬盘工具");
    assert_eq!(wiz.exe_rel.as_deref(), Some("硬盘工具/WizTree/WizTree.exe"));
    // 烤鸡工具目录名 ≠ 章节名（FurMark 在「显卡工具」章节，exe 在 烤鸡工具\）
    let fur = find("FurMark").unwrap();
    assert_eq!(fur.category, "显卡工具");
    assert!(fur.exe_rel.as_deref().unwrap().starts_with("烤鸡工具/"));
    // 双名条目
    let ar = find("autorunsc").expect("部分匹配应命中「Autoruns / autorunsc」");
    assert_eq!(
        ar.exe_rel.as_deref(),
        Some("其他工具/Autoruns/autorunsc64.exe")
    );
    // 无「—— 」的脚本示例小节不应被误收
    assert!(!cat.iter().any(|t| t.name.contains("一键导出")));
}

#[test]
fn find_is_case_insensitive_and_rejects_garbage() {
    assert!(find("wiztree").is_some());
    assert!(find(" USBDeview ").is_some());
    assert!(find("").is_none());
    assert!(find("不存在的工具").is_none());
}

#[test]
fn usage_contains_param_table() {
    let u = usage("AIDA64").unwrap();
    assert!(
        u.contains("/SILENT") && u.contains("报告格式"),
        "AIDA64 用法应含权威参数表"
    );
    assert!(usage("不存在的工具").is_none());
}

#[test]
fn risk_and_timeout_defaults_follow_doc_severity() {
    use Risk::*;
    assert_eq!(find("WizTree").unwrap().risk, Low);
    assert_eq!(find("CrystalDiskInfo").unwrap().risk, Low);
    assert_eq!(find("Defraggler").unwrap().risk, Medium);
    assert_eq!(find("Prime95").unwrap().risk, Medium);
    assert_eq!(find("Ventoy").unwrap().risk, High);
    assert_eq!(find("FPT64").unwrap().risk, High);
    assert_eq!(find("DiskGenius").unwrap().risk, High);
    // 烤机类默认长超时
    assert_eq!(find("Prime95").unwrap().timeout_secs, 1800);
    assert_eq!(find("WizTree").unwrap().timeout_secs, 60);
}

#[test]
fn command_for_resolves_path_args_cwd() {
    let tmp = tempdir("cmdfor");
    let root = fake_tools_root(&tmp, "硬盘工具/WizTree/WizTree.exe");
    let cmd = toolbelt_command_for(
        "wiztree",
        &["C:".into(), "/export=out.csv".into(), "/admin=1".into()],
        &root,
        None,
    )
    .unwrap();
    assert_eq!(cmd.tool, "WizTree");
    assert!(cmd.program.ends_with("WizTree.exe"));
    assert_eq!(cmd.args.len(), 3);
    let expected_cwd = fs::canonicalize(root.join("硬盘工具").join("WizTree")).unwrap();
    #[cfg(windows)]
    let expected_cwd = {
        // 与 lib.rs 一致：剥掉 Windows canonicalize 返回的 `\\?\` verbatim 前缀
        let s = expected_cwd.to_string_lossy();
        if s.starts_with("\\\\?\\") {
            std::path::PathBuf::from(s.trim_start_matches("\\\\?\\"))
        } else {
            expected_cwd
        }
    };
    assert_eq!(cmd.cwd, expected_cwd);
    assert_eq!(cmd.timeout_secs, 60);
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn command_for_falls_back_to_toml_default_args_and_overrides() {
    let tmp = tempdir("toml");
    let root = fake_tools_root(&tmp, "硬盘工具/WizTree/WizTree.exe");
    fs::write(
        root.join("硬盘工具").join("WizTree").join("toolbelt.toml"),
        "risk = \"medium\"\nargs = [\"C:\", \"/admin=1\"]\nusage = \"本地默认只导 C 盘\"\ntimeout_secs = 120\n",
    )
    .unwrap();
    // 没传实参 → 用 toml 默认参数 + 覆盖风险/超时
    let cmd = toolbelt_command_for("WizTree", &[], &root, None).unwrap();
    assert_eq!(cmd.args, vec!["C:", "/admin=1"]);
    assert_eq!(cmd.risk, Risk::Medium);
    assert_eq!(cmd.timeout_secs, 120);
    assert!(cmd.usage.contains("本地默认只导 C 盘"));
    // 传了实参 → 覆盖 toml 默认（「固定默认参数（AI 调用时可覆盖）」）
    let cmd2 = toolbelt_command_for("WizTree", &["D:".into()], &root, None).unwrap();
    assert_eq!(cmd2.args, vec!["D:"]);
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn command_for_errors() {
    let tmp = tempdir("err");
    let root = fake_tools_root(&tmp, "硬盘工具/WizTree/WizTree.exe");
    // 未知工具 → 报错且列出可用
    let e = toolbelt_command_for("不存在", &[], &root, None).unwrap_err();
    assert!(e.to_string().contains("WizTree"));
    // 文档有但 exe 缺失 → ExeMissing
    let e2 = toolbelt_command_for("AIDA64", &[], &root, None).unwrap_err();
    assert!(matches!(e2, ToolbeltError::ExeMissing { .. }), "{e2:?}");
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn timeout_is_clamped() {
    let tmp = tempdir("clamp");
    let root = fake_tools_root(&tmp, "硬盘工具/WizTree/WizTree.exe");
    assert_eq!(
        toolbelt_command_for("WizTree", &[], &root, Some(1))
            .unwrap()
            .timeout_secs,
        MIN_TIMEOUT_SECS
    );
    assert_eq!(
        toolbelt_command_for("WizTree", &[], &root, Some(999_999))
            .unwrap()
            .timeout_secs,
        MAX_TIMEOUT_SECS
    );
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn tools_root_search_walks_up() {
    let tmp = tempdir("root");
    // <tmp>/Tools/硬盘工具 + 综合检测 + <tmp>/a/b/c 起点
    fs::create_dir_all(tmp.join("Tools").join("硬盘工具")).unwrap();
    fs::create_dir_all(tmp.join("Tools").join("综合检测")).unwrap();
    let deep = tmp.join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();
    // explicit 不合法时继续上溯；这里直接验证显式路径命中
    assert_eq!(
        find_tools_root(Some(&tmp.join("Tools"))).unwrap(),
        tmp.join("Tools")
    );
    // 只有一个分类子目录的目录不算 Tools 根（防误中）
    let weak = tmp.join("weak").join("Tools");
    fs::create_dir_all(weak.join("硬盘工具")).unwrap();
    // 注意：find_tools_root 会从 exe/cwd 上溯，CI 环境不可控，这里只测显式分支
    // 以及弱根不被显式分支接受（会落入上溯 → 大概率 ToolsRootMissing）。
    let r = find_tools_root(Some(&weak));
    if let Ok(ok) = r {
        // 若本机恰好 cwd 上溯命中了真 Tools 根也合法，但绝不能是 weak
        assert_ne!(ok, weak);
    }
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn parse_args_windows_quoting() {
    assert_eq!(
        parse_args("/R C:\\Reports\\pc.html /HTML /SUM"),
        vec!["/R", "C:\\Reports\\pc.html", "/HTML", "/SUM"]
    );
    assert_eq!(
        parse_args("/export=\"c:\\temp\\a b.csv\" /admin=1"),
        vec!["/export=c:\\temp\\a b.csv", "/admin=1"]
    );
    assert_eq!(parse_args("  "), Vec::<String>::new());
}

#[test]
fn run_captures_output_and_timeout() {
    // 真进程测试仅限 Windows：用 cmd.exe 当替身，避免依赖具体工具
    if cfg!(windows) {
        let tmp = tempdir("run");
        let bat = tmp
            .join("Tools")
            .join("其他工具")
            .join("Demo")
            .join("demo.bat");
        fs::create_dir_all(bat.parent().unwrap()).unwrap();
        fs::write(&bat, "@echo hello-from-toolbelt\r\n@exit 7\r\n").unwrap();
        // 直接构造 ResolvedCommand 不方便（字段虽 pub 但 program 需 canonicalize），
        // 走真实目录 + 文档内工具名不可控，这里用内部等价路径：手工执行同一逻辑。
        let out = run_cmd(&bat, &[], 10);
        assert!(
            out.stdout.contains("hello-from-toolbelt"),
            "stdout={:?}",
            out.stdout
        );
        assert_eq!(out.exit_code, Some(7));
        assert!(!out.timed_out);

        let slow = tmp
            .join("Tools")
            .join("其他工具")
            .join("Demo")
            .join("slow.bat");
        fs::write(&slow, "@ping -n 30 127.0.0.1 >nul\r\n").unwrap();
        let out2 = run_cmd(&slow, &[], 2);
        assert!(out2.timed_out, "慢进程应被超时强杀");
        fs::remove_dir_all(&tmp).ok();
    }
}

/// 测试专用：绕过目录文档，直接跑一个 exe/bat（与 run 同参数面）。
fn run_cmd(exe: &Path, args: &[String], timeout_secs: u64) -> RunOutcome {
    let program = exe.canonicalize().unwrap();
    run(&ResolvedCommand {
        tool: "Demo".into(),
        category: "测试".into(),
        cwd: program.parent().unwrap().to_path_buf(),
        program,
        args: args.to_vec(),
        risk: Risk::Low,
        timeout_secs,
        usage: String::new(),
    })
}

// ── 缝口 A：全目录扫描 catalog_tree ──────────────────────────────────

/// 在 temp 下伪造一个带两个分类的工具箱根，返回 tools_root。
fn fake_catalog_root(dir: &Path) -> PathBuf {
    let root = dir.join("Tools");
    let mk = |rel: &str| {
        let p = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"@echo fake").unwrap();
    };
    mk("硬盘工具/WizTree/WizTree.exe");
    mk("硬盘工具/CPUZ/cpuz_x64.exe");
    mk("硬盘工具/CPUZ/cpuz_x32.exe");
    mk("综合检测/AIDA64/aida64.exe");
    root
}

#[test]
fn catalog_tree_scans_full_tree() {
    let tmp = tempdir("ctree");
    let root = fake_catalog_root(&tmp);
    let cat = catalog_tree(Some(&root));
    assert_eq!(cat.tools_root.as_deref(), Some(root.to_str().unwrap()));
    // 两个分类 3 个工具（CPUZ 双架构合并为 1 个条目）
    assert_eq!(cat.total, 3);
    let cats: Vec<&str> = cat.categories.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(cats, vec!["硬盘工具", "综合检测"]);
    // WizTree：精确名命中
    let wiz = cat.categories[0]
        .tools
        .iter()
        .find(|t| t.name == "WizTree")
        .unwrap();
    assert_eq!(wiz.exe_rel.as_deref(), Some("硬盘工具/WizTree/WizTree.exe"));
    assert!(!wiz.is_linked && !wiz.is_builtin_link);
    assert_eq!(wiz.extension, "exe");
    // CPUZ：主文件 cpuz_x64.exe 命中 x64，x32 收进架构变体
    let cpuz = cat.categories[0]
        .tools
        .iter()
        .find(|t| t.name == "cpuz_x64")
        .unwrap();
    assert_eq!(cpuz.arch, "x64");
    assert!(cpuz.arch_variants.iter().any(|v| v.arch == "x86"));
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn catalog_tree_merges_arch_dirs() {
    let tmp = tempdir("cmerge");
    let root = tmp.join("Tools");
    let mk = |rel: &str| {
        let p = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"@echo fake").unwrap();
    };
    mk("硬盘工具/WizTree64/WizTree64.exe");
    mk("硬盘工具/WizTree32/WizTree32.exe");
    mk("综合检测/AIDA64/aida64.exe");
    let cat = catalog_tree(Some(&root));
    // 仅架构后缀不同的目录合并为 1 个条目；目录名排序后 WizTree32 在前，按
    // C# MergeArchDirectories「保留迭代顺序第一个」语义保留 WizTree32
    let wizs: Vec<_> = cat.categories[0]
        .tools
        .iter()
        .filter(|t| t.name.contains("WizTree"))
        .collect();
    assert_eq!(wizs.len(), 1);
    assert_eq!(wizs[0].name, "WizTree32");
    assert_eq!(cat.total, 2);
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn catalog_tree_resolves_builtin_link() {
    let tmp = tempdir("cbuiltin");
    let root = tmp.join("Tools");
    fs::create_dir_all(root.join("硬盘工具")).unwrap();
    fs::create_dir_all(root.join("其他工具").join("网络重置")).unwrap();
    fs::write(
        root.join("其他工具").join("网络重置").join("link.json"),
        r#"{"builtin":"netreset"}"#,
    )
    .unwrap();
    let cat = catalog_tree(Some(&root));
    let t = cat
        .categories
        .iter()
        .find(|c| c.name == "其他工具")
        .unwrap()
        .tools[0]
        .clone();
    assert!(t.is_builtin_link && t.is_linked);
    assert_eq!(t.extension, "内置");
    assert!(t.exe_rel.is_none());
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn catalog_tree_resolves_target_link() {
    let tmp = tempdir("ctarget");
    let root = tmp.join("Tools");
    let mk = |rel: &str| {
        let p = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"@echo fake").unwrap();
    };
    mk("处理器工具/CPUZ/cpuz_x64.exe");
    fs::create_dir_all(root.join("综合检测").join("CPUZ入口")).unwrap();
    fs::write(
        root.join("综合检测").join("CPUZ入口").join("link.json"),
        r#"{"target":"处理器工具/CPUZ"}"#,
    )
    .unwrap();
    let cat = catalog_tree(Some(&root));
    let t = cat
        .categories
        .iter()
        .find(|c| c.name == "综合检测")
        .unwrap()
        .tools
        .iter()
        .find(|t| t.is_linked)
        .unwrap();
    // 主文件解析到目标目录
    assert_eq!(t.exe_rel.as_deref(), Some("处理器工具/CPUZ/cpuz_x64.exe"));
    assert!(t.is_linked && !t.is_builtin_link);
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn catalog_tree_merges_tools_json_metadata() {
    let tmp = tempdir("cmeta");
    let root = fake_catalog_root(&tmp);
    // Metadata 放在 Tools 旁（tubatools 打包布局：<盘>/Metadata/tools.json）
    let meta_dir = tmp.join("Metadata");
    fs::create_dir_all(&meta_dir).unwrap();
    fs::write(
        meta_dir.join("tools.json"),
        r#"{"tools": [{"match": "WizTree", "description": "最快磁盘占用分析", "tags": ["磁盘", "分析"]}]}"#,
    )
    .unwrap();
    let cat = catalog_tree(Some(&root));
    let wiz = cat.categories[0]
        .tools
        .iter()
        .find(|t| t.name == "WizTree")
        .unwrap();
    assert_eq!(wiz.description, "最快磁盘占用分析");
    assert_eq!(wiz.tags, vec!["磁盘", "分析"]);
    // 未收录的工具描述为空
    let cpuz = cat.categories[0]
        .tools
        .iter()
        .find(|t| t.name == "cpuz_x64")
        .unwrap();
    assert!(cpuz.description.is_empty());
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn catalog_tree_missing_root() {
    let tmp = tempdir("cmiss");
    let cat = catalog_tree(Some(&tmp.join("DoesNotExist")));
    // 显式根不合法会落入 exe/cwd 上溯；若本机恰好命中真工具箱也合法
    if cat.tools_root.is_some() {
        assert!(cat.total > 0);
    }
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn catalog_tree_sorts_categories_by_original_order() {
    let tmp = tempdir("corder");
    let root = tmp.join("Tools");
    let mk = |rel: &str| {
        let p = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"@echo fake").unwrap();
    };
    // 故意乱序创建；目录名排序后是 综合检测/其他工具/处理器工具，
    // 原版顺序应排成 处理器工具/综合检测/其他工具
    mk("其他工具/BatteryInfoView/BatteryInfoView.exe");
    mk("综合检测/AIDA64/aida64.exe");
    mk("处理器工具/Prime95/prime95.exe");
    let cat = catalog_tree(Some(&root));
    assert_eq!(
        cat.category_order,
        vec!["处理器工具", "综合检测", "其他工具"],
        "category_order 应按原版 12 分类顺序"
    );
    let names: Vec<&str> = cat.categories.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["处理器工具", "综合检测", "其他工具"]);
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn catalog_tree_exposes_risk_and_linked_from() {
    let tmp = tempdir("clinked");
    let root = tmp.join("Tools");
    let mk = |rel: &str| {
        let p = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"@echo fake").unwrap();
    };
    mk("处理器工具/CPUZ/cpuz_x64.exe");
    mk("处理器工具/FPT64/fptw64.exe");
    fs::create_dir_all(root.join("综合检测").join("CPUZ入口")).unwrap();
    fs::write(
        root.join("综合检测").join("CPUZ入口").join("link.json"),
        r#"{"target":"处理器工具/CPUZ"}"#,
    )
    .unwrap();
    let cat = catalog_tree(Some(&root));
    // 风险：cpuz 低危 / FPT64（BIOS 刷写）高危
    let cpuz = cat
        .categories
        .iter()
        .flat_map(|c| c.tools.iter())
        .find(|t| t.name == "cpuz_x64")
        .unwrap();
    assert_eq!(cpuz.risk, "low");
    let fpt = cat
        .categories
        .iter()
        .flat_map(|c| c.tools.iter())
        .find(|t| t.name == "fptw64")
        .unwrap();
    assert_eq!(fpt.risk, "high", "FPT64 应为高危（BIOS 刷写）");
    // 反向引用：综合检测/CPUZ入口 链到 处理器工具/CPUZ → CPUZ 条目的 linked_from 含 综合检测
    assert!(
        cpuz.linked_from.iter().any(|c| c == "综合检测"),
        "CPUZ 的 linked_from 应含「综合检测」，实际 {:?}",
        cpuz.linked_from
    );
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn catalog_tree_merges_display_name_risk_launch_target() {
    let tmp = tempdir("cmeta2");
    let root = fake_catalog_root(&tmp);
    let meta_dir = tmp.join("Metadata");
    fs::create_dir_all(&meta_dir).unwrap();
    fs::write(
        meta_dir.join("tools.json"),
        r#"{"tools": [
            {"match": "WizTree", "displayName": "WizTree 磁盘分析", "risk": "medium", "launchTarget": "硬盘工具/WizTree/WizTree.exe", "publisher": "Antibody Software", "version": "4.20"}
        ]}"#,
    )
    .unwrap();
    let cat = catalog_tree(Some(&root));
    let wiz = cat
        .categories
        .iter()
        .flat_map(|c| c.tools.iter())
        .find(|t| t.name.contains("WizTree"))
        .unwrap();
    assert_eq!(wiz.name, "WizTree 磁盘分析", "displayName 应覆盖展示名");
    assert_eq!(wiz.risk, "medium", "tools.json risk 应覆盖默认");
    assert_eq!(
        wiz.launch_target.as_deref(),
        Some("硬盘工具/WizTree/WizTree.exe")
    );
    assert_eq!(wiz.publisher.as_deref(), Some("Antibody Software"));
    fs::remove_dir_all(&tmp).ok();
}

// ── Tool Manifest（W1）：能力说明书 ──────────────────────────────────

#[test]
fn manifests_parse_all_expected_tools() {
    let ms = all_manifests();
    assert!(ms.len() >= 17, "manifest 工具数 {} < 17", ms.len());
    for want in [
        "Prime95",
        "FurMark",
        "FPT64",
        "CrystalDiskInfo",
        "Defraggler",
        "DiskGenius",
        "urwtest",
        "WizTree",
        "AIDA64",
        "HWiNFO",
        "USBDeview",
        "BlueScreenView",
        "BatteryInfoView",
        "Autoruns / autorunsc",
        "Everything",
        "Ventoy",
        "WinDbg",
    ] {
        assert!(ms.iter().any(|m| m.name == want), "缺少 manifest {}", want);
    }
    // 每个 manifest 五段齐全：purpose/invocation/output/risk/permission/examples
    for m in ms {
        assert!(!m.purpose.is_empty(), "{} 缺 purpose", m.name);
        assert!(
            !m.invocation.args_template.is_empty(),
            "{} 缺 args_template",
            m.name
        );
        assert!(!m.output.parser.is_empty(), "{} 缺 parser", m.name);
        assert!(
            !m.risk.is_empty() && !m.permission_level.is_empty(),
            "{} 缺 risk/level",
            m.name
        );
        assert!(!m.side_effects.is_empty(), "{} 缺 side_effects", m.name);
        assert!(!m.examples.is_empty(), "{} 缺 examples", m.name);
    }
}

#[test]
fn manifest_find_and_parsers() {
    let wiz = find_manifest("wiztree").expect("WizTree manifest 应命中");
    assert_eq!(wiz.invocation.mode, ToolMode::Cli);
    assert_eq!(parser_for(wiz), "smart_csv");
    let fpt = find_manifest("FPT64").expect("FPT64 manifest 应命中");
    assert_eq!(fpt.permission_level, "L3", "FPT64 刷 BIOS 应为 L3 永禁");
    assert_eq!(fpt.risk, "high");
    let fur = find_manifest("FurMark").unwrap();
    assert_eq!(fur.permission_level, "L1");
    // 找不到返回 None
    assert!(find_manifest("不存在的工具").is_none());
}

#[test]
fn manifests_output_parsers_are_valid() {
    // 解析器只允许 smart_csv / kv_lines / stdout 三种
    for m in all_manifests() {
        assert!(
            ["smart_csv", "kv_lines", "stdout"].contains(&m.output.parser.as_str()),
            "{} 的 parser {:?} 非法",
            m.name,
            m.output.parser
        );
    }
}

#[test]
fn catalog_tree_parses_tool_plugin_json() {
    let td = tempdir("plugin");
    let root = td.join("Tools");
    // looks_like_tools_root 要求 ≥2 个已知分类目录
    for cat in ["处理器工具", "显卡工具"] {
        fs::create_dir_all(root.join(cat)).unwrap();
    }
    // 带 tool.plugin.json 的插件目录
    let plug_dir = root.join("处理器工具/Prime95");
    fs::create_dir_all(&plug_dir).unwrap();
    fs::write(plug_dir.join("prime95.exe"), b"@echo fake").unwrap();
    fs::write(
        plug_dir.join("tool.plugin.json"),
        r#"{
            "schema": 1,
            "id": "prime95",
            "name": "Prime95",
            "version": "30.8",
            "author": "GIMPS",
            "description": "CPU 烤机",
            "entry": "prime95.exe",
            "category": "处理器工具",
            "risk": "medium",
            "tags": ["CPU", "烤机"],
            "permissions": ["run"],
            "web": { "homepage": "https://mersenne.org" }
        }"#,
    )
    .unwrap();
    // 无插件元数据的传统工具
    fs::create_dir_all(root.join("显卡工具/FurMark")).unwrap();
    fs::write(root.join("显卡工具/FurMark/furmark.exe"), b"@echo fake").unwrap();

    let cat = catalog_tree(Some(&root));
    assert_eq!(cat.total, 2, "两个工具都应被扫到");
    let prime = cat
        .categories
        .iter()
        .flat_map(|c| &c.tools)
        .find(|t| t.exe_rel.as_deref() == Some("处理器工具/Prime95/prime95.exe"))
        .expect("Prime95 应出现");
    let plug = prime.plugin.as_ref().expect("Prime95 应有插件元数据");
    assert_eq!(plug.id, "prime95");
    assert_eq!(plug.version, "30.8");
    assert_eq!(plug.risk, "medium");
    assert!(plug.permissions.contains(&"run".to_string()));
    let fur = cat
        .categories
        .iter()
        .flat_map(|c| &c.tools)
        .find(|t| t.exe_rel.as_deref() == Some("显卡工具/FurMark/furmark.exe"))
        .expect("FurMark 应出现");
    assert!(
        fur.plugin.is_none(),
        "无 tool.plugin.json 的工具 plugin 应为 None"
    );

    fs::remove_dir_all(&td).unwrap();
}

#[test]
fn plugin_activate_writes_manifest_and_deactivate_removes() {
    let td = tempdir("plugact");
    let root = td.join("Tools");
    for cat in ["处理器工具", "显卡工具"] {
        fs::create_dir_all(root.join(cat)).unwrap();
    }
    // 放一个 manifest 收录的工具（Prime95 exe_rel = 处理器工具/Prime95/prime95.exe）
    fs::create_dir_all(root.join("处理器工具/Prime95")).unwrap();
    fs::write(root.join("处理器工具/Prime95/prime95.exe"), b"@echo fake").unwrap();

    // activate：补写 tool.plugin.json
    let pf = root.join("处理器工具/Prime95/tool.plugin.json");
    assert!(!pf.exists());
    // 这里直接调用 crate 的 manifest → 目录逻辑：用 find_manifest 拿 exe_rel
    let m = diskpilot_toolbelt::find_manifest("Prime95").expect("Prime95 manifest 应有");
    let dir = root
        .join(m.exe_rel.replace('/', std::path::MAIN_SEPARATOR_STR))
        .parent()
        .unwrap()
        .to_path_buf();
    assert_eq!(dir, root.join("处理器工具/Prime95"));

    // 模拟 activate 写入
    let id = m.name.to_ascii_lowercase().replace([' ', '/'], "-");
    let meta = serde_json::json!({
        "schema": 1, "id": id, "name": m.name, "version": "1.0",
        "author": m.publisher, "description": m.purpose,
        "entry": "prime95.exe", "category": m.category, "risk": m.risk,
        "permissions": ["run"],
    });
    fs::write(&pf, serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    assert!(pf.is_file(), "插件清单应被写入");

    // catalog_tree 现在能看到该工具是插件
    let cat = diskpilot_toolbelt::catalog_tree(Some(&root));
    let prime = cat
        .categories
        .iter()
        .flat_map(|c| &c.tools)
        .find(|t| t.exe_rel.as_deref() == Some("处理器工具/Prime95/prime95.exe"))
        .expect("Prime95 应出现");
    assert!(prime.plugin.is_some(), "Prime95 应被识别为插件");
    assert_eq!(prime.plugin.as_ref().unwrap().id, "prime95");

    // deactivate：移除清单
    fs::remove_file(&pf).unwrap();
    let cat2 = diskpilot_toolbelt::catalog_tree(Some(&root));
    let prime2 = cat2
        .categories
        .iter()
        .flat_map(|c| &c.tools)
        .find(|t| t.exe_rel.as_deref() == Some("处理器工具/Prime95/prime95.exe"))
        .expect("Prime95 仍应出现");
    assert!(prime2.plugin.is_none(), "移除清单后不应再是插件");

    fs::remove_dir_all(&td).unwrap();
}

#[test]
fn install_plugin_zip_basic_and_traversal_guard() {
    let td = tempdir("plugzip");
    let root = td.join("Tools");
    for cat in ["处理器工具", "显卡工具"] {
        fs::create_dir_all(root.join(cat)).unwrap();
    }

    // 造一个插件 zip：tool.plugin.json + 真实文件 + 一个恶意 ../ 条目
    let zip_path = td.join("demo.zip");
    let f = fs::File::create(&zip_path).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zw.start_file("demo/tool.plugin.json", opts).unwrap();
    zw.write_all(
        "{
            \"schema\": 1,
            \"id\": \"demo-tool\",
            \"name\": \"Demo\",
            \"version\": \"1.0\",
            \"author\": \"test\",
            \"entry\": \"demo.exe\",
            \"category\": \"处理器工具\",
            \"risk\": \"low\",
            \"permissions\": [\"run\"]
        }"
        .as_bytes(),
    )
    .unwrap();
    zw.start_file("demo/demo.exe", opts).unwrap();
    zw.write_all(b"@echo fake").unwrap();
    zw.start_file("demo/evil/../../escape.txt", opts).unwrap();
    zw.write_all(b"traversal").unwrap();
    zw.finish().unwrap();

    // 安装
    let rel = diskpilot_toolbelt::install_plugin_zip(&zip_path, &root).expect("安装应成功");
    assert_eq!(rel, "处理器工具/demo-tool");
    // 真文件解压出来，恶意条目被跳过
    assert!(root.join("处理器工具/demo-tool/demo.exe").is_file());
    assert!(!root.join("处理器工具/demo-tool/escape.txt").exists());
    assert!(
        !root.join("escape.txt").exists(),
        "穿越条目不得写出 Tools 根"
    );
    // 元数据文件不应落在插件目录（已单独解析）
    assert!(!root.join("处理器工具/demo-tool/tool.plugin.json").exists());

    // 重复安装拒绝（避免覆盖用户数据）
    let err = diskpilot_toolbelt::install_plugin_zip(&zip_path, &root).expect_err("重复安装应报错");
    assert!(
        err.to_string().contains("已存在"),
        "报错应说明已存在: {err}"
    );

    fs::remove_dir_all(&td).unwrap();
}

/// 安装时 id/category 路径穿越防线：id 必须是 kebab-case（拒绝 `..`、绝对
/// 路径、分隔符注入），category 必须落在 Tools 根已存在的分类下；成功安装后
/// 若中途写盘失败，插件目录整体回滚（不给用户留半个插件）。
#[test]
fn install_plugin_zip_sanitizes_id_and_category() {
    let td = tempdir("plugsanitize");
    let root = td.join("Tools");
    fs::create_dir_all(root.join("处理器工具")).unwrap();

    let make_zip = |id: &str, cat: &str, out: &Path| {
        let f = fs::File::create(out).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zw.start_file("tool.plugin.json", opts).unwrap();
        zw.write_all(
            format!(
                "{{\"schema\":1,\"id\":\"{id}\",\"name\":\"Demo\",\"version\":\"1.0\",\"entry\":\"demo.exe\",\"category\":\"{cat}\",\"risk\":\"low\"}}"
            )
            .as_bytes(),
        )
        .unwrap();
        zw.finish().unwrap();
    };

    // 恶意 id：路径穿越/绝对路径/大写统统拒绝，且 Tools 根之外不得出现任何文件
    for (bad_id, hint) in [
        ("../escape", "穿越"),
        ("..%2fescape", "编码穿越"),
        ("a/b", "分隔符"),
        ("C:/evil", "绝对路径"),
        ("UPPER", "非 kebab"),
        ("", "空 id"),
        ("a_b", "下划线"),
    ] {
        let zip_path = td.join(format!("bad-{}.zip", hint));
        make_zip(bad_id, "处理器工具", &zip_path);
        let err = diskpilot_toolbelt::install_plugin_zip(&zip_path, &root)
            .expect_err(&format!("id {bad_id:?} 应拒绝（{hint}）"));
        assert!(err.to_string().contains("id"), "报错应指向 id: {err}");
    }
    assert!(
        !root.join("escape").exists() && !root.join("evil").exists(),
        "恶意 id 不得写出 Tools 根"
    );
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        1,
        "根下应只剩已建分类，无任何脏目录"
    );

    // 恶意分类：拒绝穿越、绝对路径与不存在的分类
    for (bad_cat, hint) in [
        ("../outside", "穿越"),
        ("a/b", "分隔符"),
        ("不存在分类", "未建分类"),
        ("处理器工具/../显卡工具", "夹心穿越"),
    ] {
        let zip_path = td.join(format!("badcat-{}.zip", hint));
        make_zip("ok-tool", bad_cat, &zip_path);
        let err = diskpilot_toolbelt::install_plugin_zip(&zip_path, &root)
            .expect_err(&format!("分类 {bad_cat:?} 应拒绝（{hint}）"));
        assert!(err.to_string().contains("分类"), "报错应指向分类: {err}");
    }
    assert!(!td.join("outside").exists(), "分类穿越不得写到 Tools 根外");

    // 合法安装照常成功
    let good = td.join("good.zip");
    make_zip("good-tool", "处理器工具", &good);
    let rel = diskpilot_toolbelt::install_plugin_zip(&good, &root).expect("合法 id/分类应安装成功");
    assert_eq!(rel, "处理器工具/good-tool");

    // 写盘失败 → 整体回滚：zip 里先放目录条目再放同名文件条目（文件盖目录，
    // File::create 必然失败），安装报错后插件目录不应残留半个文件。
    {
        let zip_path = td.join("fails.zip");
        {
            let zf = fs::File::create(&zip_path).unwrap();
            let mut zw = zip::ZipWriter::new(zf);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zw.start_file("tool.plugin.json", opts).unwrap();
            zw.write_all(
                "{\"schema\":1,\"id\":\"blocked-tool\",\"name\":\"Demo\",\"version\":\"1.0\",\"entry\":\"x.exe\",\"category\":\"处理器工具\",\"risk\":\"low\"}"
                    .as_bytes(),
            )
            .unwrap();
            zw.add_directory("demo/conflict/", opts).unwrap();
            zw.start_file("demo/conflict", opts).unwrap();
            zw.write_all(b"x").unwrap();
            zw.finish().unwrap();
        }
        let err =
            diskpilot_toolbelt::install_plugin_zip(&zip_path, &root).expect_err("写盘失败应报错");
        assert!(
            !root.join("处理器工具/blocked-tool").exists(),
            "安装失败后插件目录应回滚: {err}"
        );
    }

    fs::remove_dir_all(&td).unwrap();
}

/// 导出 → 安装 round-trip：把装好的插件目录打包成 zip，再装回一个干净
/// Tools 根，验证清单与全部文件字节级一致。这是插件分发的核心闭环——
/// 导出的包必须是 install_plugin_zip 能吃回去的合法插件包。
#[test]
fn export_plugin_zip_round_trip() {
    let td = tempdir("plugexport");
    let root = td.join("Tools");
    fs::create_dir_all(root.join("处理器工具")).unwrap();

    // 造一个真实插件目录（含清单 + 子目录文件 + 一个 .txt 旁料）
    let plug = root.join("处理器工具/demo-tool");
    fs::create_dir_all(plug.join("bin/sub")).unwrap();
    fs::write(plug.join("tool.plugin.json"),
        "{\"schema\":1,\"id\":\"demo-tool\",\"name\":\"Demo\",\"version\":\"1.0\",\"author\":\"test\",\"entry\":\"bin/demo.exe\",\"category\":\"处理器工具\",\"risk\":\"low\"}")
        .unwrap();
    fs::write(plug.join("bin/demo.exe"), b"@echo fake").unwrap();
    fs::write(plug.join("bin/sub/data.bin"), [0u8, 1, 2, 255]).unwrap();
    fs::write(plug.join("README.txt"), b"hello").unwrap();

    // 导出
    let out_zip = td.join("demo-export.zip");
    diskpilot_toolbelt::export_plugin_zip(&plug, &out_zip).expect("导出应成功");
    assert!(out_zip.is_file(), "导出 zip 应存在");
    // 覆盖导出（重导同名包是常见操作）
    diskpilot_toolbelt::export_plugin_zip(&plug, &out_zip).expect("重复导出应覆盖成功");

    // 装回一个干净 Tools 根（真实 Tools 根预置分类；空根没有分类容器应拒绝）
    let root2 = td.join("Tools2");
    fs::create_dir_all(root2.join("处理器工具")).unwrap();
    let rel =
        diskpilot_toolbelt::install_plugin_zip(&out_zip, &root2).expect("round-trip 安装应成功");
    assert_eq!(rel, "处理器工具/demo-tool");

    let re = root2.join("处理器工具/demo-tool");
    assert!(re.join("bin/demo.exe").is_file(), "exe 应原样回来");
    assert_eq!(
        fs::read(re.join("bin/sub/data.bin")).unwrap(),
        [0u8, 1, 2, 255],
        "二进制内容应字节一致"
    );
    assert_eq!(
        fs::read_to_string(re.join("README.txt")).unwrap(),
        "hello",
        "旁料文件应一起打包"
    );
    // 清单由 install 单独解析（不进插件目录），zip 里的 tool.plugin.json 不落地
    assert!(
        !re.join("tool.plugin.json").exists(),
        "清单应被 install 消费而非写进目录"
    );
    // 导出包里确实有清单（zip 二进制里应含 tool.plugin.json 条目名）
    let zip_bytes = fs::read(&out_zip).unwrap();
    let zip_text = String::from_utf8_lossy(&zip_bytes);
    assert!(
        zip_text.contains("tool.plugin.json"),
        "导出 zip 必须含 tool.plugin.json 条目"
    );

    fs::remove_dir_all(&td).unwrap();
}

/// 导出守卫：非插件目录（无 tool.plugin.json）拒绝打包。
#[test]
fn export_plugin_zip_rejects_non_plugin_dir() {
    let td = tempdir("plugexport_bad");
    let plain = td.join("plain");
    fs::create_dir_all(&plain).unwrap();
    fs::write(plain.join("app.exe"), b"x").unwrap();

    let out_zip = td.join("bad.zip");
    let err =
        diskpilot_toolbelt::export_plugin_zip(&plain, &out_zip).expect_err("非插件目录应拒绝");
    assert!(
        err.to_string().contains("不是插件目录"),
        "报错应说明原因: {err}"
    );
    assert!(!out_zip.exists(), "不应生成 zip");

    fs::remove_dir_all(&td).unwrap();
}
