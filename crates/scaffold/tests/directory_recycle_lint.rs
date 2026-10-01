//! Regression test for the directory-granularity Recycle lint rule:
//! `recycle_granularity = "directory"` scopes have their mode silently
//! overridden to Recycle at runtime (desktop/executor.rs locks directory
//! granularity to Recycle), so a TOML writing `mode = "quarantine"/"delete"`
//! is a trap that never takes effect. The shared check lives in
//! `diskpilot_scaffold::directory_scope_recycle_violations` so scaffold-lint
//! and this test use the same ruler.

use diskpilot_scaffold::{directory_scope_recycle_violations, Mode, RecycleGranularity, Scaffold};

fn parse(toml: &str) -> Scaffold {
    toml::from_str(toml).expect("parse scaffold toml")
}

#[test]
fn directory_granularity_with_recycle_mode_is_clean() {
    let s = parse(
        r#"
id = "t"
name = "t"
risk = "low"
disclaimer = "test"
detect = ["**/t/**"]
match = { name_contains = ["t"] }
[[scopes]]
id = "dir-cache"
label = "dir"
glob = "**/cache/**"
mode = "recycle"
recycle_granularity = "directory"
"#,
    );
    assert!(directory_scope_recycle_violations(&s).is_empty());
}

#[test]
fn directory_granularity_with_quarantine_is_caught() {
    let s = parse(
        r#"
id = "t"
name = "t"
risk = "low"
disclaimer = "test"
detect = ["**/t/**"]
match = { name_contains = ["t"] }
[[scopes]]
id = "dir-q"
label = "dir"
glob = "**/cache/**"
mode = "quarantine"
recycle_granularity = "directory"
"#,
    );
    let v = directory_scope_recycle_violations(&s);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].0, "dir-q");
    assert_eq!(v[0].1, Mode::Quarantine);
}

#[test]
fn directory_granularity_with_delete_is_caught() {
    let s = parse(
        r#"
id = "t"
name = "t"
risk = "low"
disclaimer = "test"
detect = ["**/t/**"]
match = { name_contains = ["t"] }
[[scopes]]
id = "dir-d"
label = "dir"
glob = "**/cache/**"
mode = "delete"
recycle_granularity = "directory"
"#,
    );
    let v = directory_scope_recycle_violations(&s);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].0, "dir-d");
    assert_eq!(v[0].1, Mode::Delete);
}

#[test]
fn file_granularity_with_non_recycle_mode_is_not_caught() {
    // 文件粒度 scope 尊重 mode（quarantine/delete 照常生效），不在 lint 范围。
    let s = parse(
        r#"
id = "t"
name = "t"
risk = "low"
disclaimer = "test"
detect = ["**/t/**"]
match = { name_contains = ["t"] }
[[scopes]]
id = "file-d"
label = "file"
glob = "**/tmp/**"
mode = "delete"
recycle_granularity = "file"
"#,
    );
    assert!(directory_scope_recycle_violations(&s).is_empty());
}

#[test]
fn enum_equality_sanity() {
    assert_eq!(RecycleGranularity::default(), RecycleGranularity::File);
    assert_ne!(Mode::Recycle, Mode::Delete);
}
