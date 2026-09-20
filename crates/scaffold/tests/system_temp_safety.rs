//! Safety test for system-temp scaffold — 红线断言保证清理只发生在 %TEMP% 下。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test system_temp_safety`
//!
//! 设计要点：
//! - 正向：%TEMP% 下的任意层级临时文件必须命中。
//! - 红线：用户文档、Downloads、Windows 系统目录、AppData 非 Temp 目录（
//!   Roaming / 其他 Local 子目录）、项目源码一律零命中。glob 锚定在
//!   `**/AppData/Local/Temp/` 与 %TEMP% 展开后的绝对路径上。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/system-temp.toml";

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
fn system_temp_globs_are_safe() {
    std::env::set_var("USERPROFILE", "C:/Users/test");
    std::env::set_var("APPDATA", "C:/Users/test/AppData/Roaming");
    std::env::set_var("LOCALAPPDATA", "C:/Users/test/AppData/Local");
    std::env::set_var("TMP", "C:/Users/test/AppData/Local/Temp");
    std::env::set_var("TEMP", "C:/Users/test/AppData/Local/Temp");
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
            "temp-files",
            "C:/Users/test/AppData/Local/Temp/folder_internal/f_old.tmp",
        ),
        (
            "temp-files",
            "C:/Users/test/AppData/Local/Temp/sub/installer_cache.exe",
        ),
        (
            "temp-files",
            "C:/Users/test/AppData/Local/Temp/root_file.log",
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
        // 系统目录
        "C:/Windows/Temp/foo.tmp",
        "C:/Windows/System32/config/temp.db",
        // 非 Temp 的 AppData 目录
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Cache/f_000001",
        "C:/Users/test/AppData/Roaming/Temp/important.txt",
        "C:/Users/test/AppData/Local/Packages/foo/temp/settings.json",
        // 用户文档 / 下载 / 桌面
        "C:/Users/test/Documents/temp/report.docx",
        "C:/Users/test/Downloads/installer_cache.exe",
        "C:/Users/test/Desktop/Temp/notes.txt",
        // 项目 / 源码中的 "temp" 目录（过长但仍不得误伤）
        "C:/code/my-project/temp/output.bin",
        // 通用红线
        "C:/Users/test/AppData/Roaming/config/account.cfg",
        "C:/Users/test/.temp/login/auth.dat",
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
        "system-temp.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
