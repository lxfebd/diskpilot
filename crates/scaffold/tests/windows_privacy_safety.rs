//! Safety test for windows-privacy scaffold（R331 补齐）。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test windows_privacy_safety`
//!
//! 设计要点（与 scaffolds/windows-privacy.toml 一一对应）：
//! - 正向：Recent 快捷方式、thumbcache_*.db、INetCache 下的临时文件必须命中。
//! - 红线：凭据库、JumpList（Recent Custom）、thumbcache.idx、
//!   以及无关目录下的同名结构 —— 一律必须零命中。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/windows-privacy.toml";

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
fn windows_privacy_globs_are_safe() {
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
            "recent-items",
            "C:/Users/test/AppData/Roaming/Microsoft/Windows/Recent/报告.lnk",
        ),
        (
            "recent-items",
            "C:/Users/test/AppData/Roaming/Microsoft/Windows/Recent/bar.txt",
        ),
        // JumpList 就在 Recent 里（scope label 明写「Recent / JumpList」），属于本脚本范围
        (
            "recent-items",
            "C:/Users/test/AppData/Roaming/Microsoft/Windows/Recent/AutomaticDestinations/abc.automaticDestinations-ms",
        ),
        (
            "thumbcache",
            "C:/Users/test/AppData/Local/Microsoft/Windows/Explorer/thumbcache_32.db",
        ),
        (
            "thumbcache",
            "C:/Users/test/AppData/Local/Microsoft/Windows/Explorer/thumbcache_256.db",
        ),
        (
            "inet-cache",
            "C:/Users/test/AppData/Local/Microsoft/Windows/INetCache/IE/ABC123/index.dat",
        ),
    ];
    for (expected_id, p) in positives {
        let hits = matching_scopes(&scopes, p);
        assert!(
            hits.contains(expected_id),
            "expected scope `{expected_id}` to match `{p}`, got {hits:?}",
        );
    }

    // 红线：凭据 / JumpList / 缩略图索引 / 无关目录同名结构 —— 必须零命中
    let red_lines: &[&str] = &[
        // 凭据库：动了就是账号安全事故
        "C:/Users/test/AppData/Roaming/Microsoft/Credentials/DFBE70A7E5CC19A398EBF1B96859CE5D",
        "C:/Users/test/AppData/Local/Microsoft/Credentials/DFBE70A7E5CC19A398EBF1B96859CE5D",
        "C:/Users/test/AppData/Roaming/Microsoft/Protect/S-1-5-21/abc",
        // 「Recent Custom」是 Recent 的兄弟目录，不属于 Recent/**
        "C:/Users/test/AppData/Roaming/Microsoft/Windows/Recent Custom/AutomaticDestinations-ms/abc",
        // 缩略图索引只清 *.db，idx 不动（pattern 只写 thumbcache_*.db）
        "C:/Users/test/AppData/Local/Microsoft/Windows/Explorer/thumbcache.idx",
        // 无关目录下的同名结构
        "C:/Users/test/Documents/Microsoft/Windows/Explorer/thumbcache_32.db",
        "C:/Users/test/Desktop/Microsoft/Windows/INetCache/IE/x/index.dat",
        "C:/Users/test/Documents/Microsoft/Windows/INetCache/IE/x/index.dat",
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
        "windows-privacy.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
