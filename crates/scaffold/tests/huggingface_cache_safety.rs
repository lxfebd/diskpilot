//! Safety test for huggingface-cache scaffold — 红线断言保证清理只发生在
//! HuggingFace 自己的缓存目录下。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test huggingface_cache_safety`

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/huggingface-cache.toml";

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
fn huggingface_cache_globs_are_safe() {
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
            "hf-hub-models",
            "C:/Users/test/.cache/huggingface/hub/models--gpt2/snapshots/abc",
        ),
        (
            "hf-hub-models",
            "/home/test/.cache/huggingface/hub/models--bert/snapshots/def",
        ),
        (
            "hf-datasets-cache",
            "C:/Users/test/.cache/huggingface/datasets/parquet/q2q.trf",
        ),
        (
            "hf-datasets-cache",
            "/home/test/.cache/huggingface/datasets/squad/abc",
        ),
        (
            "hf-spaces-cache",
            "C:/Users/test/.cache/huggingface/spaces/foo/bar",
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
        // 非 HuggingFace 缓存目录
        "C:/Users/test/.cache/pip/http-v2/abc",
        "C:/Users/test/.cache/huggingface/credentials",
        "/home/test/.cache/git/objects/abc/def",
        "/home/test/.cache/go-build/123",
        // 用户文档 / 项目
        "C:/Users/test/Documents/huggingface/models/foo",
        "C:/Users/test/Desktop/hf-cache/main.py",
        "/home/test/code/huggingface/cache/model.bin",
        // 模型权重在工作目录的情况（用户项目名碰巧含 huggingface）
        "/home/test/projects/huggingface-x/src/model.py",
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
        "huggingface-cache.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
