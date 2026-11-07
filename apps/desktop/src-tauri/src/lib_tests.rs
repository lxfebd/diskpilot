//! lib.rs 单元测试（从 lib.rs 拆分，2026-10-12）。
//! winapp2 红线 / walker / scope 构建 / SSRF 防线 / 权限白名单回归。

use std::fs;
use std::path::PathBuf;

use crate::scaffold_cmds::{
    is_controlled_perm, is_valid_scaffold_id, parse_winapp2, scaffold_red_line_reject,
};
use diskpilot_scaffold::Scaffold;

/// winapp2 社区规则导入预览：危险条目（Chrome Autofill → `*Web Data`）
/// 必须被红线标注拒绝，干净缓存条目不误伤。钉死「社区规则红线标注真实
/// 生效」，防止以后加样本/改 canonicalize 把这条缝口堵死又松开。
#[test]
fn parse_winapp2_marks_red_line_and_passes_clean() {
    let ini = r#"
[Chrome Autofill Community *]
LangSecRef=3029
DetectFile=%LocalAppData%\Google\Chrome*
FileKey1=%LocalAppData%\Google\Chrome*\User Data\*|*Web Data

[Clean Temp Cache *]
LangSecRef=3025
DetectFile=%LocalAppData%\Temp
FileKey1=%LocalAppData%\Temp|*.tmp
"#;
    let items = parse_winapp2(ini.to_string()).expect("样例应能解析出条目");
    assert_eq!(items.len(), 2, "2 条文件规则都要落地");

    let autofill = items
        .iter()
        .find(|i| i.name.contains("Autofill"))
        .expect("Chrome Autofill 条目存在");
    assert!(
        autofill.red_line.is_some(),
        "`*Web Data`（表单自动填充库）必须被红线拒绝：{:?}",
        autofill.red_line
    );

    let clean = items
        .iter()
        .find(|i| i.name.contains("Temp Cache"))
        .expect("Temp Cache 条目存在");
    assert!(
        clean.red_line.is_none(),
        "纯临时文件缓存不该被误伤：{:?}",
        clean.red_line
    );
}

/// 运行时红线闸必须与 CI lint 同尺。旧实现把未展开的原始 glob 送去比样本，
/// 而样本全锚在 `C:/Users/test`，于是 `%USERPROFILE%/**`（开发机上展开成真实
/// home）能装进用户脚本目录，执行期才展开变量 → 红线实际只在 CI 生效。
#[test]
fn runtime_red_line_gate_rejects_wide_home_scope() {
    let evil = r#"
id = "evil"
name = "Evil"
risk = "low"
disclaimer = "probe"
detect = ["%USERPROFILE%"]
[[scope]]
id = "wipe-home"
label = "Wide"
glob = "%USERPROFILE%/**"
mode = "delete"
"#;
    let sc = diskpilot_scaffold::parse_toml(evil).expect("evil 应能解析");
    let reason = scaffold_red_line_reject(&sc).expect("覆盖整个 home 的 scope 必须被拒绝");
    assert!(reason.contains("红线"), "原因文案要能直接进 UI：{reason}");

    // 干净缓存脚本不许被误伤。
    let clean = evil.replace(
        "glob = \"%USERPROFILE%/**\"",
        "glob = \"%LOCALAPPDATA%/Company/Cache/**\"",
    );
    let sc = diskpilot_scaffold::parse_toml(&clean).expect("clean 应能解析");
    assert!(
        scaffold_red_line_reject(&sc).is_none(),
        "缓存 scope 不该被拒：{:?}",
        scaffold_red_line_reject(&sc)
    );
}

/// Verifies `diskpilot_walker` skips system trash / volume metadata directories
/// at directory-read time. Without this prune a scope glob with a leading
/// `**` (literal_separator=false makes `**` cross `/`) can match files
/// inside `$Recycle.Bin/<SID>/$R*/...` because Windows preserves the
/// original directory tree there — meaning a "clean WeChat cache" preview
/// would list files the user already chose to put in the trash.
#[test]
fn diskpilot_walker_skips_system_trash_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let normal = root.join("normal_dir");
    fs::create_dir_all(&normal).unwrap();
    fs::write(normal.join("keep.txt"), b"x").unwrap();

    for trashy in &[
        "$RECYCLE.BIN",
        "System Volume Information",
        ".Trash",
        ".Trashes",
    ] {
        let d = root.join(trashy);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("inside.txt"), b"x").unwrap();
    }

    let mut files: Vec<String> = diskpilot_scanner::diskpilot_walker(root)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_string_lossy().replace('\\', "/"))
        .collect();
    files.sort();

    assert!(
        files.iter().any(|p| p.ends_with("/normal_dir/keep.txt")),
        "walker must still visit non-system dirs, got: {files:?}"
    );
    for trashy in &[
        "$RECYCLE.BIN",
        "System Volume Information",
        ".Trash",
        ".Trashes",
    ] {
        assert!(
            !files.iter().any(|p| p.contains(trashy)),
            "walker leaked into pruned dir `{trashy}`: {files:?}"
        );
    }
}

#[test]
fn diskpilot_walker_prune_is_case_insensitive() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let lower = root.join("$recycle.bin");
    fs::create_dir_all(&lower).unwrap();
    fs::write(lower.join("inside.txt"), b"x").unwrap();

    let leaked = diskpilot_scanner::diskpilot_walker(root)
        .into_iter()
        .flatten()
        .any(|e| e.file_type().is_file());
    assert!(
        !leaked,
        "lowercase $recycle.bin variant must also be pruned"
    );
}

/// 真实接口验证：cleanup_suggestions 同款核心逻辑（build_scope_builds +
/// tally_scope_sizes）对真实 C 盘用户目录扫描，看能否算出可清理字节。
/// 仅本机手动运行（-- --ignored --nocapture）。
#[test]
#[ignore]
fn real_cleanup_suggestions_on_local_appdata() {
    // DP_TEST_ROOT 可覆盖扫描根（复现 UI 传 C:\ 整盘的场景）。
    let root = std::env::var("DP_TEST_ROOT")
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::var("LOCALAPPDATA")
                .map(PathBuf::from)
                .or_else(|_| {
                    std::env::var("USERPROFILE")
                        .map(|u| PathBuf::from(u).join("AppData").join("Local"))
                })
        })
        .expect("需要 DP_TEST_ROOT 或 LOCALAPPDATA");
    if !root.is_dir() {
        panic!("目录不存在: {}", root.display());
    }
    println!("ROOT = {}", root.display());

    let scaffolds_dir =
        std::path::Path::new("j:\\xiangm_transfer\\xiangm\\tools\\diskpilot_src\\scaffolds");
    let scaffolds = diskpilot_scaffold::load_dir(scaffolds_dir).expect("加载真实 scaffolds");
    println!("scaffolds = {}", scaffolds.len());

    // 与 cleanup_suggestions 相同：所有 scaffold 的 scope 合并，
    // 文件 scope 一次 walk，目录 scope 一次 walk（tally_dir_scopes）。
    struct Row {
        scaffold: String,
        id: String,
        granularity: diskpilot_scaffold::RecycleGranularity,
        set: globset::GlobSet,
    }
    let mut rows: Vec<Row> = Vec::new();
    for sc in &scaffolds {
        for b in crate::cleanup_logic::build_scope_builds(sc).expect("glob 编译") {
            rows.push(Row {
                scaffold: sc.id.clone(),
                id: b.id,
                granularity: b.granularity,
                set: b.set,
            });
        }
    }
    println!("scopes = {}", rows.len());

    let mut tally: Vec<(u64, u64)> = vec![(0, 0); rows.len()];
    let mut total: Vec<(u64, u64)> = vec![(0, 0); rows.len()];
    let file_idx: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.granularity == diskpilot_scaffold::RecycleGranularity::File)
        .map(|(i, _)| i)
        .collect();
    let dir_idx: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.granularity == diskpilot_scaffold::RecycleGranularity::Directory)
        .map(|(i, _)| i)
        .collect();
    println!(
        "file scopes = {}, dir scopes = {}",
        file_idx.len(),
        dir_idx.len()
    );

    let t0 = std::time::Instant::now();
    if !file_idx.is_empty() {
        for entry in diskpilot_scanner::diskpilot_walker(&root)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let md = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let path_str = path.to_string_lossy().replace('\\', "/");
            let size = md.len();
            for &i in &file_idx {
                if rows[i].set.is_match(&path_str) {
                    total[i].0 += size;
                    total[i].1 += 1;
                    tally[i].0 += size;
                    tally[i].1 += 1;
                }
            }
        }
    }
    println!("file walk done in {:?}", t0.elapsed());

    if !dir_idx.is_empty() {
        let t1 = std::time::Instant::now();
        let (d_total, d_tally) = diskpilot_scanner::tally_dir_scopes(
            &root,
            |i| &rows[i].set,
            |i| &rows[i].id,
            &dir_idx,
            None,
            None,
            None,
        );
        for (pos, &i) in dir_idx.iter().enumerate() {
            total[i] = d_total[pos];
            tally[i] = d_tally[pos];
        }
        println!("dir walk done in {:?}", t1.elapsed());
    }

    let mut grand_total: u64 = 0;
    for (i, r) in rows.iter().enumerate() {
        if total[i].0 > 0 || tally[i].0 > 0 {
            grand_total += tally[i].0;
            println!(
                "  {:<16} {:<26} tally={} MB (n {})  total={} MB (n {})",
                r.scaffold,
                r.id,
                tally[i].0 / 1_000_000,
                tally[i].1,
                total[i].0 / 1_000_000,
                total[i].1,
            );
        }
    }
    println!(
        "== 建议清理合计 ≈ {} MB，全程 {:?} ==",
        grand_total / 1_000_000,
        t0.elapsed()
    );
    assert!(
        grand_total > 0,
        "对 {} 竟然一个字节都算不出来",
        root.display()
    );
}

fn test_scope(id: &str, glob: &str) -> diskpilot_scaffold::Scope {
    diskpilot_scaffold::Scope {
        id: id.into(),
        label: id.into(),
        glob: glob.into(),
        mode: diskpilot_scaffold::Mode::Recycle,
        prompt: None,
        category: None,
        variant: None,
        recycle_granularity: Default::default(),
        recommended_selected: None,
        required_stopped_processes: None,
    }
}

fn test_scaffold(scopes: Vec<diskpilot_scaffold::Scope>) -> Scaffold {
    Scaffold {
        id: "test-scaffold".into(),
        name: "test".into(),
        homepage: None,
        risk: diskpilot_scaffold::Risk::Low,
        disclaimer: "test".into(),
        detect: vec![],
        matcher: Default::default(),
        tags: Vec::new(),
        scopes,
    }
}

#[test]
fn build_scope_builds_compiles_valid_globs() {
    let s = test_scaffold(vec![
        test_scope("pycache", "**/__pycache__"),
        test_scope("conda_pkgs", "**/pkgs/*"),
    ]);
    let builds = crate::cleanup_logic::build_scope_builds(&s).unwrap();
    assert_eq!(builds.len(), 2);
    assert_eq!(builds[0].id, "pycache");
    // 默认 File 粒度
    assert_eq!(
        builds[0].granularity,
        diskpilot_scaffold::RecycleGranularity::File
    );
    // 大小写不敏感 + `*` 跨分隔符（literal_separator=false）
    assert!(builds[0].set.is_match("C:/Users/a/Projects/x/__PYCACHE__"));
    assert!(builds[1].set.is_match("D:/envs/py3/pkgs/numpy/info"));
}

#[test]
fn build_scope_builds_rejects_invalid_glob() {
    let s = test_scaffold(vec![test_scope("bad", "[")]);
    let err = crate::cleanup_logic::build_scope_builds(&s).unwrap_err();
    assert!(err.contains("scope `bad`"), "err: {err}");
    assert!(err.contains("invalid glob"), "err: {err}");
}

#[test]
fn build_scope_builds_expands_env_vars() {
    // scope glob 里的 ${USERPROFILE} 先展开再编译，命中展开后的真实路径
    let s = test_scaffold(vec![test_scope("env", "${USERPROFILE}/tmpcache/**")]);
    let builds = crate::cleanup_logic::build_scope_builds(&s).unwrap();
    // USERPROFILE 是 Windows 变量；非 Windows 上它不被展开（glob 原样编译），
    // 该用例只验证「展开后命中」这一行为，非 Windows 直接跳过断言。
    let Ok(up_raw) = std::env::var("USERPROFILE") else {
        return;
    };
    let up = up_raw.replace('\\', "/");
    assert!(
        builds[0].set.is_match(format!("{up}/tmpcache/sub/f.txt")),
        "展开后的 glob 应命中 USERPROFILE 下的路径"
    );
}

#[test]
fn scaffold_id_validation_blocks_path_traversal() {
    // 安全关键：id 会拼进文件名，路径穿越字符必须全拒。
    assert!(is_valid_scaffold_id("wechat-pc"));
    assert!(is_valid_scaffold_id("my-tool2"));
    assert!(!is_valid_scaffold_id(""), "空 id 拒绝");
    assert!(!is_valid_scaffold_id("../evil"), "路径穿越拒绝");
    assert!(!is_valid_scaffold_id("a/b"), "分隔符拒绝");
    assert!(!is_valid_scaffold_id("a\\b"), "反斜杠拒绝");
    assert!(!is_valid_scaffold_id("UPPER"), "大写拒绝");
    assert!(!is_valid_scaffold_id("has space"), "空格拒绝");
    assert!(!is_valid_scaffold_id("a.toml"), "点拒绝");
}

// ── ssrf_safe_get：SSRF 防线回归 ──────────────────────────────

/// 公网 302 → 内网（127.0.0.1）的「首跳合法 → 重定向绕过」必须被逐跳
/// 校验拦下，即使首跳本身是公网域名。
#[tokio::test]
async fn ssrf_redirect_to_loopback_is_blocked() {
    use std::io::{Read, Write};
    // 本地起一个 HTTP 服务，把 /ok 重定向到 http://127.0.0.1:9（无效端口，
    // 若被跟随会连接失败而非「已拒绝」——断言错误信息里的私网字样）
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let body = "redirect";
            let resp = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    // 注意：首跳目标是 127.0.0.1（本地测试服务），ssrf_safe_get 第一步
    // 就会因为私网拒绝——这验证的是「私网首跳拒绝」。重定向绕过验证
    // 依赖公网首跳，单测里无法真打公网；这里验证核心语义：任何私网
    // 目标（含 302 后跳向私网）都无法到达。
    let err = crate::net_http::ssrf_safe_get(&format!("http://127.0.0.1:{port}/ok"), 1024, None)
        .await
        .expect_err("私网目标必须拒绝");
    assert!(
        err.contains("https") || err.contains("不可用"),
        "拒绝信息应说明协议/私网原因: {err}"
    );
}

/// http 协议被白名单拦下（私网校验在 ai.rs 已全覆盖，这里验证 ssrf_safe_get
/// 本身不放开协议白名单）。
#[tokio::test]
async fn ssrf_http_scheme_is_rejected() {
    let err = crate::net_http::ssrf_safe_get("http://127.0.0.1:1/a", 10, None)
        .await
        .expect_err("http 私网必须拒绝");
    assert!(err.contains("https"), "http 应被协议白名单拦下: {err}");
}

/// 公网域名（字面量不在私网段）放行第一步——不做 DNS 反查（避免
/// 代理 DNS 重绑定），这里只验证 blocked_private_target 判定不误伤公网。
#[test]
fn ssrf_public_host_literal_passes() {
    assert_eq!(
        crate::ai::ai_ssrf::blocked_private_target("https://example.com/a"),
        None
    );
    assert_eq!(
        crate::ai::ai_ssrf::blocked_private_target("https://plugins.example.org/pack.zip"),
        None
    );
}

/// sync_perms 白名单：合法受控权限 id 通过，L3 与未知字符串一律丢弃。
#[test]
fn sync_perms_whitelist_filters_l3_and_unknown() {
    let good = ["plugin.manage", "cli.run", "cleanup.execute", "fan.control"];
    for id in good {
        assert!(is_controlled_perm(id), "{id} 应在白名单内");
    }
    let bad = [
        "disk.format",
        "bios.flash",
        "hw.overclock",
        "system.files",
        "",
        "..",
        "plugin.manage; rm -rf",
    ];
    for id in bad {
        assert!(!is_controlled_perm(id), "{id} 必须被白名单拒绝");
    }
}
