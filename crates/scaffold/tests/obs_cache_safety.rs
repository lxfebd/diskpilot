//! Safety test for obs-cache scaffold — 红线断言保证清理只发生在 OBS 的
//! crash-dumps / logs 目录下，绝不触碰场景/配置。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test obs_cache_safety`

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/obs-cache.toml";

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
        } else if bytes[i] == b'$' {
            // ${VAR} (Unix style) — test helper mirror of expand_env
            let rest = &bytes[i + 1..];
            if rest.first() == Some(&b'{') {
                if let Some(end_rel) = rest[1..].iter().position(|&b| b == b'}') {
                    let var = std::str::from_utf8(&rest[1..1 + end_rel]).unwrap_or("");
                    if let Ok(v) = std::env::var(var) {
                        out.push_str(&v.replace('\\', "/"));
                        i += end_rel + 3;
                        continue;
                    }
                }
            } else {
                let end = rest
                    .iter()
                    .position(|&b| b == b'/' || b == b'\\' || b == b'}' || b == b' ')
                    .unwrap_or(rest.len());
                let var = std::str::from_utf8(&rest[..end]).unwrap_or("");
                if let Ok(v) = std::env::var(var) {
                    out.push_str(&v.replace('\\', "/"));
                    i += end + 1;
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
fn obs_cache_globs_are_safe() {
    std::env::set_var("APPDATA", "C:/Users/test/AppData/Roaming");
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
        (
            "obs-crash-dumps",
            "C:/Users/test/AppData/Roaming/obs-studio/crash-dumps/abc.dmp",
        ),
        (
            "obs-crash-dumps",
            "/home/test/.config/obs-studio/crash-dumps/def.dmp",
        ),
        (
            "obs-logs",
            "C:/Users/test/AppData/Roaming/obs-studio/logs/2026-09-01.log",
        ),
        (
            "obs-logs",
            "/home/test/.config/obs-studio/logs/2026-09-01.log",
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
    // 红线断言：OBS 的配置 / 场景 / 录制文件绝不命中
    // ========================================================================
    let red_lines: &[&str] = &[
        // OBS 配置与场景
        "C:/Users/test/AppData/Roaming/obs-studio/basic/scenes/Scene 1.json",
        "C:/Users/test/AppData/Roaming/obs-studio/basic/profiles/Default.ini",
        "C:/Users/test/AppData/Roaming/obs-studio/plugin_config/obs-browser/browser.json",
        "C:/Users/test/AppData/Roaming/obs-studio/global.ini",
        "/home/test/.config/obs-studio/basic/scenes/Scene 2.json",
        // 用户自己的录制文件
        "C:/Users/test/Videos/obs/2026-09-01 12-00-00.mkv",
        "/home/test/Videos/obs-recording.mp4",
        // 非 OBS 的 AppData 目录
        "C:/Users/test/AppData/Roaming/obs-suite/crash-dumps/abc.dmp",
        "C:/Users/test/AppData/Local/obs-studio/crash-dumps/abc.dmp",
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
        "obs-cache.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
