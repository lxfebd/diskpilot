//! 覆盖守卫（R331）：scaffolds/*.toml 与 crates/scaffold/tests/*_safety.rs 必须一一对应。
//!
//! docs/scaffold-authoring.md 写明「一份脚本必须同时交付 TOML + 安全测试，缺一不可合入」，
//! 但此前 window 三份新脚本漏了安全测试，靠人肉审查才发现。这条测试把规则变成可执行检查：
//! - 每份 TOML 都要有 <id 下划线化>_safety.rs；
//! - 每个 *_safety.rs 都要对得上一份 TOML（防止改名后留下孤儿测试）。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test scaffold_coverage`

use std::collections::BTreeSet;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}

/// 扫描目录里符合后缀的文件主名（不含扩展名）。
fn stems_in(dir: &std::path::Path, ext: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            out.insert(stem.to_string());
        }
    }
    out
}

#[test]
fn every_scaffold_has_a_safety_test() {
    let root = workspace_root();
    let scaffold_dir = root.join("scaffolds");
    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");

    // scaffolds/*.toml 的 id（文件名即 id，与 TOML 的 id 字段一致）
    let toml_ids = stems_in(&scaffold_dir, "toml");

    // tests/*_safety.rs 对应的 id（下划线还原成连字符）
    let mut test_ids = BTreeSet::new();
    let mut orphan_files = Vec::new();
    for stem in stems_in(&test_dir, "rs") {
        let Some(base) = stem.strip_suffix("_safety") else {
            continue;
        };
        test_ids.insert(base.replace('_', "-"));
        orphan_files.push(format!("`{stem}.rs`"));
    }

    // TOML 的 id 字段必须与文件名一致，否则测试命名对不上
    let mut mismatched = Vec::new();
    for id in &toml_ids {
        let path = scaffold_dir.join(format!("{id}.toml"));
        let text = std::fs::read_to_string(&path).expect("read scaffold toml");
        let parsed: diskpilot_scaffold::Scaffold =
            toml::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        if parsed.id != *id {
            mismatched.push(format!(
                "scaffolds/{id}.toml 的 id 字段是 `{}`，与文件名不一致",
                parsed.id
            ));
        }
    }

    let missing: Vec<String> = toml_ids
        .difference(&test_ids)
        .map(|id| format!("crates/scaffold/tests/{}_safety.rs（对应 scaffolds/{id}.toml）", id.replace('-', "_")))
        .collect();
    let extra: Vec<String> = test_ids
        .difference(&toml_ids)
        .map(|id| format!("crates/scaffold/tests/{}_safety.rs 找不到对应的 scaffolds/{id}.toml", id.replace('-', "_")))
        .collect();

    let mut problems = Vec::new();
    if !missing.is_empty() {
        problems.push(format!("缺安全测试：\n  {}", missing.join("\n  ")));
    }
    if !extra.is_empty() {
        problems.push(format!("孤儿安全测试：\n  {}", extra.join("\n  ")));
    }
    if !mismatched.is_empty() {
        problems.push(format!("文件名与 id 不一致：\n  {}", mismatched.join("\n  ")));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));

    // 空转保护：目录扫不到东西时上面的断言也会「通过」
    assert!(
        toml_ids.len() >= 20,
        "只扫到 {} 份 scaffold TOML，路径疑似不对：{}",
        toml_ids.len(),
        scaffold_dir.display()
    );
}
