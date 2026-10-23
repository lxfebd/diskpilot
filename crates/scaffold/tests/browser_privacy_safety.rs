//! Safety test for browser-privacy scaffold（R331 补齐——此前 21 份 TOML 里仅这 3 份没有配套安全测试）。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test browser_privacy_safety`
//!
//! 设计要点（与 scaffolds/browser-privacy.toml 一一对应）：
//! - 正向：Chrome / Edge 的 History / History-journal / History-journal-ms，
//!   Firefox 的 formhistory.sqlite（含 -wal）必须命中对应 scope。
//! - 红线：书签、密码（Login Data）、Cookie、Preferences、Local Storage、
//!   IndexedDB、扩展数据，以及 Firefox 的 places.sqlite（书签+历史同库）、
//!   cookies.sqlite、logins.json、key4.db —— 一律必须零命中。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/browser-privacy.toml";

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
fn browser_privacy_globs_are_safe() {
    std::env::set_var("USERPROFILE", "C:/Users/test");
    std::env::set_var("APPDATA", "C:/Users/test/AppData/Roaming");
    std::env::set_var("LOCALAPPDATA", "C:/Users/test/AppData/Local");
    std::env::set_var("HOME", "/home/test");

    let scaffold = load_scaffold();
    let scopes: Vec<(String, globset::GlobSet)> = scaffold
        .scopes
        .iter()
        .map(|s| (s.id.clone(), build_set(&expand(&s.glob))))
        .collect();

    // 正向：每个 scope 至少一条命中路径
    let positives: &[(&str, &str)] = &[
        (
            "chromium-history",
            "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/History",
        ),
        (
            "chromium-history",
            "C:/Users/test/AppData/Local/Google/Chrome/User Data/Profile 1/History-journal",
        ),
        (
            "chromium-history",
            "C:/Users/test/AppData/Local/Microsoft/Edge/User Data/Default/History-journal-ms",
        ),
        (
            "firefox-formhistory",
            "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc123.default/formhistory.sqlite",
        ),
        (
            "firefox-formhistory",
            "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc123.default/formhistory.sqlite-wal",
        ),
    ];
    for (expected_id, p) in positives {
        let hits = matching_scopes(&scopes, p);
        assert!(
            hits.contains(expected_id),
            "expected scope `{expected_id}` to match `{p}`, got {hits:?}",
        );
    }

    // 红线：书签 / 密码 / Cookie / 站点数据 / 扩展 / Firefox 书签历史库 —— 必须零命中
    let red_lines: &[&str] = &[
        // Chrome / Edge 用户数据（与 History 同目录但绝不能碰）
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Bookmarks",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Login Data",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Cookies",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Preferences",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Secure Preferences",
        "C:/Users/test/AppData/Local/Microsoft/Edge/User Data/Default/Login Data",
        // 站点数据 / 扩展程序数据
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Local Storage/leveldb/000003.log",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/IndexedDB/https_www.example.com",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Extensions/abc/manifest.json",
        // Firefox：书签与历史同库（places.sqlite），表单库只许 formhistory
        "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc123.default/places.sqlite",
        "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc123.default/places.sqlite-wal",
        "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc123.default/cookies.sqlite",
        "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc123.default/logins.json",
        "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc123.default/key4.db",
        // 通用红线（继承 CLAUDE.md）
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/db_storage/MMKV/data.db",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/config/account.cfg",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/login/auth.dat",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Accounts/auth.json",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Favorite/fav1",
        // 伪造同名结构（literal_separator=false 下 `**` 会跨分隔符，需防误命中）
        "C:/Users/test/Desktop/Google/Chrome/User Data/Default/History",
        "C:/Users/test/Documents/Mozilla/Firefox/Profiles/abc/formhistory.sqlite",
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
        "browser-privacy.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
