//! Discord scaffold safety test — glob 正向断言 + 红线断言。
//!
//! 需求依据：docs/scaffold-requirements/messaging.md §7.8（Discord 实测附录）。
//! 红线断言失败 = glob 写宽了 → 收紧 glob，不要放宽测试。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/discord.toml";

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // out of crates/scaffold
    p.pop(); // out of crates
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

/// Mirror scaffold::expand_env for `%VAR%`-style env substitution.
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
fn discord_globs_are_safe() {
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

    // ========================================================================
    // 正向断言：每个 scope id 至少一条命中路径
    // ========================================================================
    let positives: &[(&str, &str)] = &[
        (
            "cache",
            "C:/Users/test/AppData/Roaming/discord/Cache/Cache_Data/f_000001",
        ),
        (
            "code-cache",
            "C:/Users/test/AppData/Roaming/discord/Code Cache/js/index.js",
        ),
        (
            "gpu-cache",
            "C:/Users/test/AppData/Roaming/discord/GPUCache/data_0",
        ),
        (
            "gpu-cache",
            "C:/Users/test/AppData/Roaming/discord/DawnGraphiteCache/data_0",
        ),
        (
            "gpu-cache",
            "C:/Users/test/AppData/Roaming/discord/DawnWebGPUCache/data_0",
        ),
        (
            "blob-storage",
            "C:/Users/test/AppData/Roaming/discord/blob_storage/0000000000000001/1a2b3c",
        ),
        (
            "crashpad",
            "C:/Users/test/AppData/Roaming/discord/Crashpad/reports/2026-05-01.dmp",
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
    // 红线断言：以下路径必须不被任何 scope 命中
    // ========================================================================
    let red_lines: &[&str] = &[
        // 登录态 / 数据存储（删了要重新登录 / 丢设置）
        "C:/Users/test/AppData/Roaming/discord/Local Storage/leveldb/000003.log",
        "C:/Users/test/AppData/Roaming/discord/Local Storage/leveldb/LOG",
        "C:/Users/test/AppData/Roaming/discord/Session Storage/000003.log",
        "C:/Users/test/AppData/Roaming/discord/IndexedDB/https_discord.com_0.indexeddb.leveldb/000003.log",
        "C:/Users/test/AppData/Roaming/discord/Network/Cookies",
        "C:/Users/test/AppData/Roaming/discord/Network/Trust Tokens/index",
        // Chromium 凭据 / 顶层状态
        "C:/Users/test/AppData/Roaming/discord/Preferences",
        "C:/Users/test/AppData/Roaming/discord/First Run",
        "C:/Users/test/AppData/Roaming/discord/Local State",
        "C:/Users/test/AppData/Roaming/discord/Cookies",
        "C:/Users/test/AppData/Roaming/discord/Login Data",
        // 更新器 / 安装目录（Local 无用户数据，也不碰）
        "C:/Users/test/AppData/Local/discord/Update.exe",
        "C:/Users/test/AppData/Local/discord/app-1.0.9000/discord.exe",
        // 与 Cache 同名的其它 app（大小写不敏感匹配下不能误伤）
        "C:/Users/test/AppData/Roaming/SomeApp/discord/Cache/f_001",
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
        "discord.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
