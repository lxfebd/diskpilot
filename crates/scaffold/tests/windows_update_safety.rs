//! Safety test for windows-update scaffold（R331 补齐）。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test windows_update_safety`
//!
//! 设计要点（与 scaffolds/windows-update.toml 一一对应）：
//! - 正向：SoftwareDistribution/Download 下的更新包缓存、
//!   DeliveryOptimization/Cache 下的分片缓存必须命中。
//! - 红线：SoftwareDistribution 根下的 DataStore.edb（更新历史与回滚依赖）、
//!   AuthCabs、Windows 系统目录、用户目录下的同名 DeliveryOptimization 目录。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/windows-update.toml";

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
fn windows_update_globs_are_safe() {
    std::env::set_var("USERPROFILE", "C:/Users/test");
    std::env::set_var("APPDATA", "C:/Users/test/AppData/Roaming");
    std::env::set_var("LOCALAPPDATA", "C:/Users/test/AppData/Local");
    std::env::set_var("WINDIR", "C:/Windows");
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
            "windows-update-download",
            "C:/Windows/SoftwareDistribution/Download/Install/package.cab",
        ),
        (
            "windows-update-download",
            "C:/Windows/SoftwareDistribution/Download/update1.cab",
        ),
        (
            "delivery-optimization-cache",
            "C:/Windows/ServiceProfiles/NetworkService/AppData/Local/Microsoft/Windows/DeliveryOptimization/Cache/1/2/file",
        ),
    ];
    for (expected_id, p) in positives {
        let hits = matching_scopes(&scopes, p);
        assert!(
            hits.contains(expected_id),
            "expected scope `{expected_id}` to match `{p}`, got {hits:?}",
        );
    }

    // 红线：更新历史库 / 回滚依赖 / 系统目录 / 无关目录 —— 必须零命中
    let red_lines: &[&str] = &[
        // 删了 DataStore.edb 会丢更新历史、破坏回滚
        "C:/Windows/SoftwareDistribution/DataStore/DataStore.edb",
        "C:/Windows/SoftwareDistribution/DataStore/Logs/edb.log",
        // 回滚用的 cab 备份
        "C:/Windows/SoftwareDistribution/AuthCabs/abc.cab",
        "C:/Windows/SoftwareDistribution/SelfUpdate/abc.cab",
        // 系统目录绝不碰
        "C:/Windows/System32/kernel32.dll",
        "C:/Windows/WinSxS/x86_microsoft-windows-shell32/abc",
        "C:/Windows/assembly/GAC_MSIL/abc/def.dll",
        // 用户目录下的同名结构（glob 锚在 %WINDIR%）
        "C:/Users/test/AppData/Local/Microsoft/Windows/DeliveryOptimization/Cache/1/2/file",
        "C:/Users/test/Desktop/SoftwareDistribution/Download/x.cab",
        // 推送服务自身的状态目录（不在 Download/** 下，删了会破坏 WU 服务状态）
        "C:/Windows/SoftwareDistribution/PostRebootEventCache.V2/abc.dat",
        "C:/Windows/SoftwareDistribution/reporting/EventCache/abc.bin",
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
        "windows-update.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
