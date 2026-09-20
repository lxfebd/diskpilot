//! Safety test for ide-caches scaffold — 红线断言保证清理只发生在 IDE 的
//! 缓存目录下，绝不触碰扩展 / 设置 / 项目源码。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test ide_caches_safety`

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/ide-caches.toml";

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
fn ide_cache_globs_are_safe() {
    std::env::set_var("APPDATA", "C:/Users/test/AppData/Roaming");
    std::env::set_var("USERPROFILE", "C:/Users/test");
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
            "vscode-cache",
            "C:/Users/test/AppData/Roaming/Code/Cache/Cache_Data/data_0",
        ),
        (
            "vscode-cache",
            "C:/Users/test/AppData/Roaming/Code/CachedData/1.90.0/index.js",
        ),
        (
            "vscode-cache",
            "C:/Users/test/AppData/Roaming/Code/Code Cache/js/abc",
        ),
        (
            "vscode-cache",
            "C:/Users/test/AppData/Roaming/Code/GPUCache/data_0",
        ),
        (
            "cursor-cache",
            "C:/Users/test/AppData/Roaming/Cursor/Cache/Cache_Data/data_0",
        ),
        (
            "cursor-cache",
            "C:/Users/test/AppData/Roaming/Cursor/CachedData/0.40.0/index.js",
        ),
        (
            "intellij-caches",
            "C:/Users/test/AppData/Local/JetBrains/IntelliJIdea2024.1/caches/foo",
        ),
        (
            "intellij-caches",
            "C:/Users/test/AppData/Local/JetBrains/PyCharm2024.1/caches/bar",
        ),
        (
            "intellij-index",
            "C:/Users/test/AppData/Local/JetBrains/IntelliJIdea2024.1/index/abc",
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
    // 红线断言
    // ========================================================================
    let red_lines: &[&str] = &[
        // IDE 扩展 / 设置 / 用户数据绝不命中
        "C:/Users/test/AppData/Roaming/Code/extensions/ms-python.python/extension.js",
        "C:/Users/test/AppData/Roaming/Code/User/settings.json",
        "C:/Users/test/AppData/Roaming/Code/User/workspaceStorage/foo/state.vscdb",
        "C:/Users/test/AppData/Roaming/Cursor/extensions/foo/bar.js",
        "C:/Users/test/AppData/Roaming/Cursor/User/settings.json",
        // IntelliJ 项目级 .idea 与配置（不是 JetBrains 全局缓存）
        "C:/Users/test/projects/my-app/.idea/workspace.xml",
        "C:/Users/test/AppData/Roaming/JetBrains/IntelliJIdea2024.1/options/editor.xml",
        // 项目源码 / 文档 / 非 IDE 目录
        "C:/Users/test/Documents/Code/project/main.ts",
        "C:/Users/test/Desktop/Cache/notes.txt",
        "/home/test/code/Cursor/cache/main.py",
        "/home/test/projects/vscode/src/index.ts",
        // IntelliJ 全局配置（非缓存）
        "C:/Users/test/AppData/Local/JetBrains/Toolbox/apps/IDEA-U/idea.exe",
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
        "ide-caches.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
