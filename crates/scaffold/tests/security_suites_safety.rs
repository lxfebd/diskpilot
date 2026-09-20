//! Safety test for security-suites scaffold — 红线断言保证只清理安全软件的
//! Chromium 内核浏览器渲染缓存，绝不碰登录/收藏/账号/安全软件本体。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test security_suites_safety`
//!
//! 设计要点：
//! - 正向：360ChromeX / 360Chrome / 360se6 的 User Data 下 Cache / Code Cache /
//!   GPUCache / ShaderCache 等渲染缓存命中。
//! - 红线：User Data 下的 Login Data / Cookies / Bookmarks / History、*.db、
//!   360 安全卫士本体（360safe）、腾讯电脑管家本体（QQPCMgr）、火绒本体
//!   （Huorong）、安全卫士隔离区一律零命中。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/security-suites.toml";

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}

fn load_scaffold() -> diskpilot_scaffold::Scaffold {
    let path = workspace_root().join(SCAFFOLD_FILE);
    let text = std::fs::read_to_string(&path).expect("read scaffold toml");
    toml::from_str(&text).expect("parse scaffold toml")
}

fn build_set(pattern: &str) -> globset::GlobSet {
    let g = globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .case_insensitive(true)
        .build()
        .unwrap_or_else(|e| panic!("bad glob `{pattern}`: {e}"));
    let mut b = globset::GlobSetBuilder::new();
    b.add(g);
    b.build().unwrap()
}

fn expand(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(end) = bytes[i + 1..].iter().position(|&b| b == b'%') {
                let var = std::str::from_utf8(&bytes[i + 1..i + 1 + end]).unwrap_or("");
                if let Ok(v) = std::env::var(var) {
                    out.push_str(&v.replace('\\', "/"));
                    i += end + 2;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn matching_scopes<'a>(scopes: &'a [(String, globset::GlobSet)], path: &str) -> Vec<&'a str> {
    scopes
        .iter()
        .filter_map(|(id, gs)| {
            if gs.is_match(path) {
                Some(id.as_str())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn security_suites_globs_are_safe() {
    std::env::set_var("USERPROFILE", "C:/Users/test");
    std::env::set_var("APPDATA", "C:/Users/test/AppData/Roaming");
    std::env::set_var("LOCALAPPDATA", "C:/Users/test/AppData/Local");
    std::env::set_var("PROGRAMDATA", "C:/ProgramData");
    std::env::set_var("PROGRAMFILES", "C:/Program Files");
    std::env::set_var("PROGRAMFILES(X86)", "C:/Program Files (x86)");
    std::env::set_var("HOME", "/home/test");

    let scaffold = load_scaffold();
    let scopes: Vec<(String, globset::GlobSet)> = scaffold
        .scopes
        .iter()
        .map(|s| (s.id.clone(), build_set(&expand(&s.glob))))
        .collect();

    // ========================================================================
    // 正向断言
    // ========================================================================
    let positives: &[(&str, &str)] = &[
        // 360ChromeX（新一代）渲染缓存
        (
            "360-browser-cache",
            "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/Cache/data_0",
        ),
        (
            "360-browser-cache",
            "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/Code Cache/js/1/abc.js",
        ),
        (
            "360-browser-cache",
            "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/GPUCache/data_0",
        ),
        (
            "360-browser-cache",
            "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Profile 1/ShaderCache/abc.bin",
        ),
        // 360Chrome（旧版）
        (
            "360-browser-cache",
            "C:/Users/test/AppData/Local/360Chrome/Chrome/User Data/Default/Cache/f_000001",
        ),
        // 360se6（安全浏览器经典版）
        (
            "360se-cache",
            "C:/Users/test/AppData/Roaming/360se6/User Data/Default/Cache/data_1",
        ),
        (
            "360se-cache",
            "C:/Users/test/AppData/Roaming/360se6/User Data/Default/GPUCache/data_2",
        ),
        // 非 Default 的 profile（用户切换场景）
        (
            "360-browser-cache",
            "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Profile 2/Cache/f_000002",
        ),
    ];
    for (expected_id, p) in positives {
        let hits = matching_scopes(&scopes, p);
        assert!(
            hits.contains(expected_id),
            "expected scope `{expected_id}` to match `{p}`, got {hits:?}",
        );
    }

    // ========================================================================
    // 红线断言：必须不被任何 scope 命中
    // ========================================================================
    let red_lines: &[&str] = &[
        // ---- 浏览器登录 / 收藏 / 历史（无 .db 后缀也可能存凭据）----
        "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/Login Data",
        "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/Cookies",
        "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/Bookmarks",
        "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/History",
        "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/Session Storage/SESSIONS",
        "C:/Users/test/AppData/Roaming/360se6/User Data/Default/Login Data",
        "C:/Users/test/AppData/Roaming/360se6/User Data/Default/Cookies",
        "C:/Users/test/AppData/Roaming/360se6/User Data/Default/Bookmarks",
        // ---- User Data 根下的 *.db（即使名字碰巧叫 Cache 也不碰）----
        "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/History.db",
        "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/Web Data",
        // ---- 安全软件本体（只检测不清理）----
        "C:/Program Files (x86)/360/360safe/softmgr/softmgr.dat",
        "C:/Program Files (x86)/360/360safe/update/update.dat",
        "C:/Users/test/AppData/Roaming/360safe/softmgr.db",
        "C:/ProgramData/360safe/update/wbUpdate.dat",
        "C:/ProgramData/Tencent/QQPCMgr/core/virus.db",
        "C:/ProgramData/Tencent/QQPCMgr/update/config.dat",
        "C:/ProgramData/Huorong/Sysdiag/hips.db",
        "C:/ProgramData/Huorong/Sysdiag/UserData.dat",
        // ---- 安全卫士隔离区 / 恢复区（可能牵涉用户被隔离文件）----
        "C:/ProgramData/360safe/Quarantine/quarantine.dat",
        "C:/Program Files (x86)/360/360safe/isolated/isolate.db",
        // ---- 通用红线兜底 ----
        "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/Preferences",
        "C:/Users/test/AppData/Local/360ChromeX/Chrome/User Data/Default/Secure Preferences",
    ];

    let mut violations = Vec::new();
    for p in red_lines {
        let hits = matching_scopes(&scopes, p);
        if !hits.is_empty() {
            violations.push(format!("`{p}` -> {hits:?}"));
        }
    }
    assert!(
        violations.is_empty(),
        "security-suites.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
