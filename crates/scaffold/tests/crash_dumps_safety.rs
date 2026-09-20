//! Safety test for crash-dumps scaffold — 红线断言保证只清理崩溃转储与 WER 报告。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test crash_dumps_safety`
//!
//! 设计要点：
//! - 正向：%LOCALAPPDATA%/CrashDumps、%LOCALAPPDATA%/.../WER/{ReportQueue,ReportArchive}、
//!   C:/Windows/Minidump 下的 .dmp / 报告文件命中。
//! - 红线：Windows 系统目录、用户的 dump 分析资料、AppData 非 WER/CrashDumps 目录
//!   一律零命中。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/crash-dumps.toml";

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
fn crash_dumps_globs_are_safe() {
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
    // 正向断言
    // ========================================================================
    let positives: &[(&str, &str)] = &[
        (
            "crash-dumps-local",
            "C:/Users/test/AppData/Local/CrashDumps/FOO.exe.12345.dmp",
        ),
        (
            "wer-reports",
            "C:/Users/test/AppData/Local/Microsoft/Windows/WER/ReportQueue/Report.exe/abc/dump.mdmp",
        ),
        (
            "wer-reports",
            "C:/Users/test/AppData/Local/Microsoft/Windows/WER/ReportArchive/Report.exe/cba/WERInternalMetadata.xml",
        ),
        (
            "crash-dumps-system",
            "C:/Windows/Minidump/12345-67890-01.dmp",
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
        // Windows 自身目录（除 Minidump 的白名单外）
        "C:/Windows/System32/config/systemprofile/AppData/Local/CrashDumps/systemfile.dmp",
        "C:/Windows/System32/sysdata.dmp",
        "C:/Windows/Logs/wer.log",
        // WER 目录之外的 Windows 报告
        "C:/ProgramData/Microsoft/Windows/WER/ReportQueue/foo.mdmp",
        // 用户的 dump 分析资料（不是程序自动生成的）
        "C:/Users/test/Documents/dumps/analysis.dmp",
        "C:/Users/test/CrashDumps/manual_save.dmp",
        // AppData 其他目录
        "C:/Users/test/AppData/Roaming/Microsoft/Windows/WER/ReportQueue/foo.mdmp",
        "C:/Users/test/AppData/Local/Microsoft/Edge/User Data/Crashpad/reports/foo.mdmp",
        // 通用红线
        "C:/Users/test/AppData/Local/Temp/config/account.cfg",
        "C:/Users/test/.wer/private/log.txt",
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
        "crash-dumps.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
