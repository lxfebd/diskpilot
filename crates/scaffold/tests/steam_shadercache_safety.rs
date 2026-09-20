//! Safety test for steam-shadercache scaffold — 红线断言保证只清理 shadercache。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test steam_shadercache_safety`

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/steam-shadercache.toml";

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
fn steam_shadercache_globs_are_safe() {
    std::env::set_var("USERPROFILE", "C:/Users/test");
    std::env::set_var("ProgramFiles", "C:/Program Files");
    std::env::set_var("ProgramFiles(x86)", "C:/Program Files (x86)");
    std::env::set_var("LOCALAPPDATA", "C:/Users/test/AppData/Local");
    std::env::set_var("HOME", "/home/test");

    let scaffold = load_scaffold();
    let scopes: Vec<(String, globset::GlobSet)> = scaffold
        .scopes
        .iter()
        .map(|s| (s.id.clone(), build_set(&expand(&s.glob))))
        .collect();

    let positives: &[(&str, &str)] = &[
        (
            "shader-cache",
            "D:/Steam/steamapps/shadercache/730/foobar/D3DSCache/0",
        ),
        (
            "shader-cache",
            "C:/Program Files (x86)/Steam/steamapps/shadercache/570/f_1",
        ),
        (
            "shader-cache",
            "G:/SteamLibrary/steamapps/shadercache/440/cache",
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
        // 游戏本体
        "C:/Program Files (x86)/Steam/steamapps/common/CS2/game/bin/win64/cs2.exe",
        "D:/Steam/steamapps/common/Dota 2/game/dota/bin/win64/dota2.exe",
        // 创意工坊 / 存档 / 安装信息
        "C:/Program Files (x86)/Steam/steamapps/workshop/content/730/234567",
        "C:/Program Files (x86)/Steam/userdata/76561198000000000/730/remote",
        "C:/Program Files (x86)/Steam/steamapps/appmanifest_730.acf",
        // 非 shadercache 的其他 cache
        "C:/Program Files (x86)/Steam/config/htmlcache/index.json",
        "C:/Program Files (x86)/Steam/logs/connection_log.txt",
        // 用户文件
        "C:/Users/test/Desktop/steam/steamapps/shadercache/myfile.txt",
        "C:/Users/test/Documents/steamapps/shadercache/important.bak",
        // 通用红线
        "C:/Program Files (x86)/Steam/config/account.cfg",
        "C:/Program Files (x86)/Steam/userdata/login/auth.dat",
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
        "steam-shadercache.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
