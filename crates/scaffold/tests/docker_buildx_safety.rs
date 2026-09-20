//! Safety test for docker-buildx scaffold — 红线断言保证清理只发生在
//! BuildKit / Buildx 自己的缓存目录下。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test docker_buildx_safety`
//!
//! 设计要点：
//! - 正向：~/.cache/buildkit 与 ~/.docker/buildx 下的任意层级必须命中。
//! - 红线：/var/lib/docker（镜像/容器/卷所在）、用户文档、项目源码、
//!   其他 ~/.cache 子目录一律零命中。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/docker-buildx.toml";

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
fn docker_buildx_globs_are_safe() {
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
            "buildkit-cache",
            "C:/Users/test/.cache/buildkit/sha256/abc/blob",
        ),
        ("buildkit-cache", "/home/test/.cache/buildkit/cache.db"),
        (
            "docker-buildx-metadata",
            "C:/Users/test/.docker/buildx/instances/default",
        ),
        (
            "docker-buildx-metadata",
            "/home/test/.docker/buildx/instances/default",
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
        // Docker 引擎数据根（镜像/容器/卷）
        "/var/lib/docker/buildkit/sha256/abc/blob",
        "/var/lib/docker/containers/xyz/config.json",
        "C:/ProgramData/Docker/buildkit/sha256/abc",
        // 用户文档 / 项目里的 Docker 目录
        "C:/Users/test/Documents/docker-buildx/notes.md",
        "C:/Users/test/Desktop/buildkit-tools/main.rs",
        "/home/test/projects/docker/cache/foo",
        "/home/test/code/buildx/cache.bin",
        // 其他 ~/.cache 子目录（不得误伤）
        "C:/Users/test/.cache/pip/http-v2/abc",
        "/home/test/.cache/pip/http-v2/abc",
        "/home/test/.cache/go-build/123/abc",
        // 镜像/容器数据
        "/home/test/.docker/config.json",
        "C:/Users/test/.docker/config.json",
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
        "docker-buildx.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
