//! Safety test for dev-caches scaffold — 红线断言保证只清理包管理器缓存。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test dev_caches_safety`
//!
//! glob 严格锚定到用户 AppData/Local（npm/pnpm/Yarn/pip）与 Users 主目录
//! （.npm/_cacache、.cache/pip、.cargo/registry），避免误命中用户在
//! 文档/项目里手动创建的同名目录。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/dev-caches.toml";

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
fn dev_caches_globs_are_safe() {
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
            "npm-cache",
            "C:/Users/test/AppData/Local/npm-cache/_cacache/index-v5/aa/bb",
        ),
        ("npm-cache", "C:/Users/test/AppData/Local/pnpm-cache/_store"),
        (
            "yarn-cache",
            "C:/Users/test/AppData/Local/Yarn/Cache/v6/npm-foo-1.0.0",
        ),
        ("npm-cacache", "C:/Users/test/.npm/_cacache/index-v5/cc/dd"),
        (
            "pip-cache",
            "C:/Users/test/AppData/Local/pip/Cache/http-v2/aa",
        ),
        (
            "cargo-cache",
            "C:/Users/test/.cargo/registry/cache/index.crates.io-6f17d22bba15001f/foo-1.0.0.crate",
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
        // 项目依赖 / 源码绝不碰
        "C:/code/my-app/node_modules/foo/index.js",
        "C:/code/my-app/package.json",
        "C:/code/my-app/pnpm-cache/_store",
        "C:/code/my-app/npm-cache/foo",
        "C:/code/my-app/.cargo/registry/foo",
        // pip 已安装环境
        "C:/Users/test/AppData/Local/Programs/Python/Python312/Lib/site-packages/foo/__init__.py",
        "C:/conda/envs/ml/lib/python3.11/site-packages/bar",
        // 非缓存目录
        "C:/Users/test/AppData/Local/pip/other.txt",
        "C:/Users/test/.cargo/bin/cargo.exe",
        "C:/Users/test/.npmrc",
        "C:/Users/test/.cargo/config.toml",
        "C:/Users/test/.cache/other/foo",
        // 用户文档 / 下载里手动建的"同名目录"（最重要的红线）
        "C:/Users/test/Documents/npm-cache/notes.txt",
        "C:/Users/test/Documents/pnpm-cache/foo",
        "C:/Users/test/Documents/Yarn/Cache/foo",
        "C:/Users/test/Downloads/pip-installer.exe",
        "C:/Users/test/Desktop/.npm/_cacache/manual.txt",
        "C:/Users/test/Pictures/.cache/pip/foo",
        // 通用红线
        "C:/Users/test/.npm/config/account.cfg",
        "C:/Users/test/.cargo/login/auth.dat",
        "C:/Users/test/AppData/Roaming/pip/Cache/foo",
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
        "dev-caches.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
