//! Safety test for firefox-cache scaffold — 红线断言保证只清理 Firefox 缓存目录。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test firefox_cache_safety`

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/firefox-cache.toml";

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
        if bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
            if let Some(end) = bytes[i + 2..].iter().position(|&b| b == b'}') {
                let var = std::str::from_utf8(&bytes[i + 2..i + 2 + end]).unwrap_or("");
                if let Ok(v) = std::env::var(var) {
                    out.push_str(&v.replace('\\', "/"));
                    i += end + 3;
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
fn firefox_cache_globs_are_safe() {
    std::env::set_var("USERPROFILE", "C:/Users/test");
    std::env::set_var("LOCALAPPDATA", "C:/Users/test/AppData/Local");
    std::env::set_var("APPDATA", "C:/Users/test/AppData/Roaming");
    std::env::set_var("HOME", "/home/test");

    let scaffold = load_scaffold();
    let scopes: Vec<(String, globset::GlobSet)> = scaffold
        .scopes
        .iter()
        .map(|s| (s.id.clone(), build_set(&expand(&s.glob))))
        .collect();

    let positives: &[(&str, &str)] = &[
        (
            "firefox-network-cache",
            "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/cache2/entries/xyz",
        ),
        (
            "firefox-startup-cache",
            "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/startupCache/script-cache.bin",
        ),
        (
            "firefox-shader-cache",
            "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/shader-cache/f_1",
        ),
        (
            "firefox-network-cache",
            "/home/test/.mozilla/firefox/abc.default/Cache/foo",
        ),
    ];
    for (expected_id, p) in positives {
        let hits = matching_scopes(&scopes, p);
        assert!(
            hits.contains(expected_id),
            "expected scope `{expected_id}` to match `{p}`, got {hits:?}",
        );
    }

    let red_lines: &[&str] = &[
        // 书签 / 历史 / 密码 / Cookie（.sqlite 文件）
        "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/places.sqlite",
        "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/cookies.sqlite",
        "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/logins.json",
        "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/key4.db",
        "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/formhistory.sqlite",
        // 扩展数据 / 网站数据
        "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/extensions/abc",
        "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/storage/default/http_www.example.com",
        "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/containers.json",
        // 用户下载 / 项目
        "C:/Users/test/Downloads/Firefox/cache_pack.zip",
        "C:/code/firefox/cache2/tmp.bin",
        // 伪造同名结构（裸 `**/` 前缀 + literal_separator=false 会误命中）
        "C:/Users/test/Desktop/.mozilla/firefox/abc/cache2/entries/x",
        "C:/Users/test/Documents/Mozilla/Firefox/Profiles/abc.default/Cache/foo",
        "/opt/data/.mozilla/firefox/abc/cache2/entries/x",
        // 通用红线
        "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/db_storage/MMKV/data.db",
        "C:/Users/test/AppData/Local/Mozilla/Firefox/Profiles/abc.default/config/account.cfg",
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
        "firefox-cache.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
