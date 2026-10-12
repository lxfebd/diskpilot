//! scaffold lib.rs 单元测试（从 lib.rs 拆分，2026-10-12）。
//! 红线样本 / 规范化 / 目录粒度 Recycle 锁 / 编译匹配 / 真实微信 TOML。

use super::*;

#[test]
fn red_line_samples_are_all_hit_by_catchall() {
    // `**/*` 命中一切；它必须把每条红线样本都召回来，保证清单本身可验证。
    let hits = glob_hits_red_line("**/*");
    assert_eq!(
        hits.len(),
        red_line_samples().len(),
        "catchall must recall every red-line sample, got {hits:?}"
    );
}

#[test]
fn red_line_hits_on_specific_families() {
    // 单条红线族必须被各自命中。
    assert!(!glob_hits_red_line("**/Msg/**").is_empty(), "Msg");
    assert!(
        !glob_hits_red_line("**/db_storage/**").is_empty(),
        "db_storage"
    );
    assert!(!glob_hits_red_line("**/config/**").is_empty(), "config");
    assert!(
        !glob_hits_red_line("**/Favorite*/**").is_empty(),
        "Favorite"
    );
    assert!(!glob_hits_red_line("**/key/**").is_empty(), "key");
    // winapp2 社区规则 `*Web Data` / `*Form Data` 只认文件名后缀——
    // 表单自动填充库也是红线，补样本后这类绕过必须命中
    assert!(!glob_hits_red_line("*Web Data").is_empty(), "*Web Data");
    assert!(!glob_hits_red_line("*Form Data").is_empty(), "*Form Data");
}

#[test]
fn red_line_clean_globs_have_no_hits() {
    // 正常缓存类 glob 不允许误伤红线样本。
    for glob in [
        "**/Cache/**",
        "**/logs/**",
        "**/temp/**",
        "**/Code/Cache/**",
        "**/node_modules/.cache/**",
    ] {
        assert!(
            glob_hits_red_line(glob).is_empty(),
            "`{glob}` must not hit red lines"
        );
    }
}

#[test]
fn red_line_check_is_machine_env_independent() {
    // 关键回归：红线用写死的锚，不读本机 env。开发者机上 USERPROFILE 是
    // C:/Users/<真名>，若按真实展开，`%USERPROFILE%/**` 这种覆盖整个 home
    // 的 delete 级 scope 会零命中从而溜过校验。
    for wide in [
        "%USERPROFILE%/**",
        "$HOME/**",
        "${HOME}/**",
        "~/**",
        "%USERPROFILE%\\.ssh\\**",
        "%APPDATA%/**",
        "%LOCALAPPDATA%/Packages/**",
    ] {
        assert!(
            !red_line_violations(wide).is_empty(),
            "宽 scope `{wide}` 必须命中红线"
        );
    }
}

#[test]
fn red_line_boundary_drive_root_vs_home() {
    // 边界文档化（G-中2）：样本都锚定在 C:/Users/test 之下，
    // 所以 `C:/Users/**` 命中，盘根字面 `C:/`（无 **）不命中，
    // 而 `C:/**` 这种盘级宽扫描命中全部样本。
    assert!(
        !red_line_violations("C:/Users/**").is_empty(),
        "`C:/Users/**` 必须命中（覆盖整个 home 的宽 glob）"
    );
    assert!(
        red_line_violations("C:/").is_empty(),
        "盘根字面 `C:/`（无 **）不应误命中样本"
    );
    assert!(
        !red_line_violations("C:/**").is_empty(),
        "`C:/**` 全程扫描应命中系统文件与 home 样本"
    );
}

#[test]
fn canonicalize_for_red_line_forms() {
    assert_eq!(canonicalize_for_red_line("%USERPROFILE%"), "C:/Users/test");
    assert_eq!(
        canonicalize_for_red_line("%LOCALAPPDATA%\\Google\\Chrome\\**"),
        "C:/Users/test/AppData/Local/Google/Chrome/**"
    );
    assert_eq!(
        canonicalize_for_red_line("~/Downloads/**"),
        "C:/Users/test/Downloads/**"
    );
    assert_eq!(canonicalize_for_red_line("$HOME/**"), "C:/Users/test/**");
    assert_eq!(canonicalize_for_red_line("${HOME}/**"), "C:/Users/test/**");
    // 未知变量原样保留（不猜值，避免误判成命中）。
    assert_eq!(
        canonicalize_for_red_line("%SOME_APP_HOME%/Cache/**"),
        "%SOME_APP_HOME%/Cache/**"
    );
}

#[test]
fn red_line_clean_env_anchored_cache_globs() {
    // 真实 scaffold 的缓存类 glob 规范化后不得误伤红线。
    for glob in [
        "%TEMP%/**",
        r#"{"%LOCALAPPDATA%/Google/Chrome/User Data,${HOME}/.config/google-chrome}/**/{Cache,Code Cache,GPUCache,GrShaderCache,DawnGraphiteCache,DawnWebGPUCache,ShaderCache}"#,
        "%LOCALAPPDATA%/pip/Cache/**",
        "%APPDATA%/Code/CachedData/**",
    ] {
        assert!(
            red_line_violations(glob).is_empty(),
            "缓存 glob `{glob}` 不该命中红线，got {:?}",
            red_line_violations(glob)
        );
    }
}

#[test]
fn scaffold_red_line_violations_reports_offending_scope_only() {
    let toml = r#"
id = "demo"
name = "Demo"
risk = "low"
disclaimer = "test"
detect = ["**/Demo"]
[[scope]]
id = "safe-cache"
label = "Safe"
glob = "%LOCALAPPDATA%/Demo/Cache/**"
mode = "recycle"
[[scope]]
id = "wide-home"
label = "Wide"
glob = "%USERPROFILE%/**"
mode = "delete"
"#;
    let sc: Scaffold = toml::from_str(toml).unwrap();
    let v = scaffold_red_line_violations(&sc);
    assert_eq!(v.len(), 1, "只应报出越界的那一条 scope，got {v:?}");
    assert_eq!(v[0].0, "wide-home");
    assert!(!v[0].1.is_empty());
}

#[test]
fn parses_minimal_scaffold() {
    let toml = r#"
id = "demo"
name = "Demo"
risk = "low"
disclaimer = "test"
detect = ["**/Demo"]
[[scope]]
id = "s1"
label = "S1"
glob = "**/*"
mode = "recycle"
"#;
    let s: Scaffold = toml::from_str(toml).unwrap();
    assert_eq!(s.id, "demo");
    assert_eq!(s.scopes.len(), 1);
}

/// 一个坏 .toml（非 UTF-8 / 解析失败）绝不能把整目录规则清空——原实现
/// `read_to_string()?` 遇到第一个坏文件就中止，清理页失去全部脚手架
/// （2026-10-11 修复）。
#[test]
fn load_dir_skips_bad_files_and_keeps_good_ones() {
    let dir = std::env::temp_dir().join(format!("scaffold-load-dir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // 好文件
    let good = "id = \"good\"\nname = \"Good\"\nrisk = \"low\"\n\
                disclaimer = \"test\"\ndetect = [\"**/Good\"]\n\
                [[scope]]\nid = \"s\"\nlabel = \"S\"\nglob = \"**/Cache/**\"\nmode = \"recycle\"\n";
    std::fs::write(dir.join("good.toml"), good).unwrap();
    // 坏文件 1：非 UTF-8 字节（模拟编码坏/被截断的下载文件）
    std::fs::write(dir.join("bad-utf8.toml"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    // 坏文件 2：语法错误（解析失败走 warn 跳过）
    std::fs::write(dir.join("bad-syntax.toml"), "id = [broken").unwrap();
    // 非 toml 扩展名不该被读
    std::fs::write(dir.join("notes.txt"), "whatever").unwrap();

    let out = load_dir(&dir).expect("load_dir must not abort on a single bad file");
    let ids: Vec<_> = out.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["good"], "好文件必须被加载，坏文件只跳过");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn detect_compiled_matches_simple_recursive_glob() {
    // Reproduce the wechat-pc detect surface as minimally as possible.
    let toml = r#"
id = "wechat-pc"
name = "WeChat (PC)"
risk = "low"
disclaimer = "test"
detect = [
  "**/xwechat_files",
  "**/WeChat Files",
]
[match]
name_contains = ["xwechat_files", "WeChat Files", "xwechat", "WeChat"]
"#;
    let s: Scaffold = toml::from_str(toml).unwrap();
    let compiled = compile_all(&[s]);
    let path = Path::new("C:/Users/lvjin/Documents/xwechat_files");
    assert_eq!(
        detect_compiled(&compiled, path, None),
        Some("wechat-pc".to_string()),
        "detect_compiled missed `**/xwechat_files` against {:?}",
        path,
    );
}

#[test]
fn detect_compiled_must_isolate_bad_patterns_like_detect_for() {
    // If any single detect pattern fails to compile, compile_all should
    // not nuke the whole scaffold's detect surface — that would diverge
    // from detect_for, which silently skips each bad pattern individually.
    // This guards against regressions where one quirky pattern (e.g. a
    // shell-expansion result containing `[` or `{` in a real user env)
    // poisons every other pattern in the same scaffold.
    let toml = r#"
id = "wechat-pc"
name = "WeChat (PC)"
risk = "low"
disclaimer = "test"
detect = [
  "[invalid-glob",
  "**/xwechat_files",
]
[match]
name_contains = []
"#;
    let s: Scaffold = toml::from_str(toml).unwrap();
    let scaffolds = vec![s];
    let compiled = compile_all(&scaffolds);
    let path = Path::new("C:/Users/lvjin/Documents/xwechat_files");
    assert_eq!(
        detect_compiled(&compiled, path, None),
        detect_for(&scaffolds, path),
        "compile_all dropped good pattern when sibling pattern is bad",
    );
    assert_eq!(
        detect_compiled(&compiled, path, None).as_deref(),
        Some("wechat-pc"),
    );
}

#[test]
fn detect_compiled_matches_actual_wechat_toml() {
    // Load the real scaffolds/wechat-pc.toml (same path as production
    // load_dir) and confirm detect_compiled tags a typical 4.x data dir.
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scaffolds/wechat-pc.toml");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {:?} failed: {}", path, e));
    let s: Scaffold = toml::from_str(&text).expect("wechat-pc.toml parse");
    eprintln!(
        "wechat-pc loaded: detect={:?}, name_contains={:?}, must_have_child={:?}",
        s.detect, s.matcher.name_contains, s.matcher.must_have_child,
    );
    let compiled = compile_all(std::slice::from_ref(&s));

    for p in [
        "C:/Users/lvjin/Documents/xwechat_files",
        "C:\\Users\\lvjin\\Documents\\xwechat_files",
        "/some/path/xwechat_files",
    ] {
        let path = Path::new(p);
        let got = detect_compiled(&compiled, path, None);
        let oracle = detect_for(std::slice::from_ref(&s), path);
        eprintln!("path={:?} compiled={:?} oracle={:?}", p, got, oracle);
        assert_eq!(got, oracle, "divergence at {:?}", p);
        assert_eq!(got.as_deref(), Some("wechat-pc"), "missed at {:?}", p);
    }
}

#[test]
fn detect_compiled_equivalent_to_detect_for_on_wechat_pc() {
    // Full wechat-pc detect/match block, including %VAR% expansion
    // patterns, to verify compile_all isn't silently dropping any pattern.
    let toml = r#"
id = "wechat-pc"
name = "WeChat (PC)"
risk = "low"
disclaimer = "test"
detect = [
  "%USERPROFILE%/Documents/xwechat_files",
  "%USERPROFILE%/Documents/WeChat Files",
  "%APPDATA%/Tencent/xwechat",
  "%APPDATA%/Tencent/WeChat",
  "**/xwechat_files",
  "**/WeChat Files",
]
[match]
name_contains = ["xwechat_files", "WeChat Files", "xwechat", "WeChat"]
"#;
    let s: Scaffold = toml::from_str(toml).unwrap();
    let scaffolds = vec![s];
    let compiled = compile_all(&scaffolds);

    for path_str in [
        "C:/Users/lvjin/Documents/xwechat_files",
        "C:\\Users\\lvjin\\Documents\\xwechat_files",
        "/home/foo/Documents/xwechat_files",
        "C:/Users/lvjin/Documents/WeChat Files",
    ] {
        let path = Path::new(path_str);
        assert_eq!(
            detect_compiled(&compiled, path, None),
            detect_for(&scaffolds, path),
            "compile_all vs detect_for diverged on {:?}",
            path_str,
        );
    }
}
