//! steam-inspector 单元测试（从 lib.rs 拆分，2026-10-12）。
//! KV 解析 / appmanifest / libraryfolders / 推荐逻辑 / 文件名匹配。

use super::*;
use crate::recommend::SECONDS_PER_MONTH;

#[test]
fn parses_simple_kv() {
    let kv = parse(r#""k" "v""#).unwrap();
    assert_eq!(kv.len(), 1);
    assert_eq!(kv[0].0, "k");
    assert_eq!(kv[0].1.as_str(), Some("v"));
}

#[test]
fn parses_nested_block() {
    let kv = parse(r#""outer" { "k" "v" "n" { "x" "y" } }"#).unwrap();
    let outer = kv[0].1.as_block().unwrap();
    assert_eq!(outer[0].0, "k");
    assert_eq!(outer[0].1.as_str(), Some("v"));
    assert_eq!(outer[1].0, "n");
    let inner = outer[1].1.as_block().unwrap();
    assert_eq!(inner[0].1.as_str(), Some("y"));
}

#[test]
fn handles_escape_sequences() {
    let kv = parse(r#""k" "a\nb\tc\\d\"e""#).unwrap();
    assert_eq!(kv[0].1.as_str(), Some("a\nb\tc\\d\"e"));
}

#[test]
fn handles_line_comments() {
    let kv = parse("// header comment\n\"k\" \"v\"\n// trailing comment\n").unwrap();
    assert_eq!(kv.len(), 1);
    assert_eq!(kv[0].1.as_str(), Some("v"));
}

#[test]
fn unterminated_string_errors() {
    assert!(parse(r#""k" "unterminated"#).is_err());
}

#[test]
fn last_played_zero_maps_to_none() {
    // Carved-down minimal AppState.
    let acf = r#"
"AppState"
{
"appid"        "730"
"name"         "Counter-Strike 2"
"installdir"   "Counter-Strike Global Offensive"
"StateFlags"   "4"
"LastPlayed"   "0"
"LastUpdated"  "1700000000"
"SizeOnDisk"   "35000000000"
}
"#;
    let m = parse_appmanifest(acf).unwrap();
    assert_eq!(m.appid, 730);
    assert_eq!(m.last_played, None);
    assert_eq!(m.last_updated, Some(1_700_000_000));
    assert!(m.is_fully_installed());
}

#[test]
fn missing_optional_fields_are_zero_not_error() {
    let acf = r#"
"AppState"
{
"appid"      "1234"
"name"       "minimal game"
"installdir" "minimal"
}
"#;
    let m = parse_appmanifest(acf).unwrap();
    assert_eq!(m.size_on_disk, 0);
    assert_eq!(m.state_flags, 0);
    assert!(!m.is_fully_installed());
    assert_eq!(m.last_played, None);
}

#[test]
fn missing_appid_is_an_error() {
    let acf = r#"
"AppState"
{
"name" "no appid here"
}
"#;
    assert!(parse_appmanifest(acf).is_err());
}

#[test]
fn libraryfolders_extracts_all_paths() {
    let vdf = r#"
"libraryfolders"
{
"0"
{
    "path"        "C:\\Program Files (x86)\\Steam"
    "label"       ""
    "contentid"   "1"
    "totalsize"   "0"
    "apps"
    {
        "730"  "35000000000"
    }
}
"1"
{
    "path"  "D:\\SteamLibrary"
    "apps"
    {
        "1091500" "70000000000"
    }
}
}
"#;
    let roots = parse_libraryfolders(vdf).unwrap();
    assert_eq!(roots.len(), 2);
    assert!(roots[0].to_string_lossy().contains("Program Files"));
    assert!(roots[1].to_string_lossy().contains("SteamLibrary"));
}

#[test]
fn case_insensitive_top_level_key() {
    // Some VDF dialects use lowercase "libraryfolders", some title case.
    let vdf = r#"
"LibraryFolders"
{
"0" { "path" "C:/Steam" }
}
"#;
    let roots = parse_libraryfolders(vdf).unwrap();
    assert_eq!(roots.len(), 1);
}

// -----------------------------------------------------------------
// compute_recommendation
// -----------------------------------------------------------------

/// Pick a fixed `now` so reasons don't drift with the real clock.
/// Roughly 2026-05-01.
const TEST_NOW: u64 = 1_777_000_000;

fn months_ago(now: u64, months: u32) -> u64 {
    let s = SECONDS_PER_MONTH as u64 * months as u64;
    now.saturating_sub(s)
}

#[test]
fn ghost_always_recommended_with_diagnostic_reason() {
    let (rec, reason) =
        compute_recommendation(50_000_000_000, Some(TEST_NOW), true, TEST_NOW, 1001);
    assert!(rec);
    assert_eq!(reason.as_deref(), Some("ACF 存在但安装目录缺失"));
}

#[test]
fn small_game_never_recommended() {
    // 2GB never played — below the 5GB floor.
    let (rec, reason) = compute_recommendation(2_000_000_000, None, false, TEST_NOW, 1002);
    assert!(!rec);
    assert!(reason.is_none());
}

#[test]
fn large_old_game_recommended_with_size_and_age_reason() {
    let played_8mo_ago = months_ago(TEST_NOW, 8);
    let (rec, reason) =
        compute_recommendation(60_000_000_000, Some(played_8mo_ago), false, TEST_NOW, 1003);
    assert!(rec);
    let r = reason.unwrap();
    assert!(r.starts_with("60GB"), "got: {}", r);
    assert!(r.contains("8 个月未启动"), "got: {}", r);
}

#[test]
fn never_played_large_game_recommended_with_explicit_never_string() {
    // §6.5: never-played counts as 12-month-stale for the recommendation
    // threshold, but for *display* we say "从未启动" — the more honest
    // statement, since we don't actually know the user has owned the
    // game for 12 months. The truthful copy beats the formula-derived one.
    let (rec, reason) = compute_recommendation(60_000_000_000, None, false, TEST_NOW, 1004);
    assert!(rec);
    assert_eq!(reason.as_deref(), Some("60GB · 从未启动"));
}

#[test]
fn medium_recently_played_not_recommended() {
    let played_2mo_ago = months_ago(TEST_NOW, 2);
    let (rec, _) =
        compute_recommendation(40_000_000_000, Some(played_2mo_ago), false, TEST_NOW, 1005);
    assert!(!rec, "40GB played 2 months ago shouldn't be recommended");
}

#[test]
fn very_large_recently_played_recommended() {
    // 60GB played 3 months ago → triggers the "size_gb >= 50 AND >= 3mo" rule.
    let played_3mo_ago = months_ago(TEST_NOW, 3);
    let (rec, reason) =
        compute_recommendation(60_000_000_000, Some(played_3mo_ago), false, TEST_NOW, 1006);
    assert!(rec);
    assert!(reason.unwrap().contains("个月未启动"));
}

#[test]
fn very_old_game_says_n_years() {
    let played_3y_ago = months_ago(TEST_NOW, 36);
    let (rec, reason) =
        compute_recommendation(70_000_000_000, Some(played_3y_ago), false, TEST_NOW, 1234);
    assert!(rec);
    assert!(reason.unwrap().contains("3 年未启动"));
}

#[test]
fn system_component_never_recommended_even_if_ghost() {
    // SteamVR runtime (250820) is a system component: even a huge,
    // never-played, ghost ACF must never surface as "建议处理" — removing
    // it breaks every VR game.
    let (rec, reason) = compute_recommendation(80_000_000_000, None, true, TEST_NOW, 250820);
    assert!(!rec);
    assert!(reason.is_none());
}

#[test]
fn steam_client_itself_never_recommended() {
    // Steam client (appid 480) also carries the "installdir = steam"
    // marker; the recommendation must not suggest uninstalling Steam.
    let (rec, _) = compute_recommendation(30_000_000_000, None, false, TEST_NOW, 480);
    assert!(!rec);
}

// -----------------------------------------------------------------
// is_appmanifest_filename
// -----------------------------------------------------------------

#[test]
fn appmanifest_filename_matcher() {
    assert!(is_appmanifest_filename("appmanifest_730.acf"));
    assert!(is_appmanifest_filename("appmanifest_1091500.acf"));
    // Wrong extension.
    assert!(!is_appmanifest_filename("appmanifest_730.txt"));
    // Wrong prefix.
    assert!(!is_appmanifest_filename("manifest_730.acf"));
    // Non-numeric "appid" — would crash parse_appmanifest, skip early.
    assert!(!is_appmanifest_filename("appmanifest_abc.acf"));
    // Empty appid.
    assert!(!is_appmanifest_filename("appmanifest_.acf"));
}
