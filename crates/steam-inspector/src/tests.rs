//! steam-inspector 单元测试（从 lib.rs 拆分，2026-10-12）。
//! KV 解析 / appmanifest / libraryfolders / 推荐逻辑 / 文件名匹配。

use super::*;
use crate::recommend::SECONDS_PER_MONTH;
use std::path::{Path, PathBuf};

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

// -----------------------------------------------------------------
// 卸载残留检测（有目录无 ACF）
// -----------------------------------------------------------------

fn residue_tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("steam-residue-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn write_files(dir: &Path, count: usize, size: usize) {
    std::fs::create_dir_all(dir).unwrap();
    for i in 0..count {
        std::fs::write(dir.join(format!("f{i}.bin")), vec![0u8; size]).unwrap();
    }
}

fn acf(appid: u32, installdir: &str) -> String {
    format!(
        "\n\"AppState\"\n{{\n\"appid\"        \"{appid}\"\n\"name\"         \"Game {appid}\"\n\"installdir\"   \"{installdir}\"\n\"StateFlags\"   \"4\"\n\"SizeOnDisk\"   \"1000\"\n}}\n"
    )
}

#[test]
fn residue_detects_leftover_install_dirs_and_orphan_workshop() {
    let dir = residue_tmp("basic");
    let steamapps = dir.join("steamapps");
    let common = steamapps.join("common");

    // 已安装（ACF 引用）——不应报残留
    write_files(&common.join("Counter-Strike Global Offensive"), 3, 4096);
    // 卸载残留：目录在、没有任何 ACF 引用 → 真实占用、游戏列表隐形
    write_files(&common.join("Old Game Leftover"), 2, 1024);
    // 白名单：多游戏共享运行库，删了会连带坏游戏 → 不报
    write_files(&common.join("Steamworks Shared"), 1, 8192);
    // workshop：440 有游戏；999 没有任何游戏 → 订阅残留
    write_files(&steamapps.join("workshop/content/440/12345"), 1, 512);
    write_files(&steamapps.join("workshop/content/999/6789"), 1, 700);
    // 非数字目录不是订阅内容
    write_files(&steamapps.join("workshop/content/downloads"), 1, 128);

    std::fs::write(
        steamapps.join("appmanifest_440.acf"),
        acf(440, "Counter-Strike Global Offensive"),
    )
    .unwrap();

    let inv = inspect_at_with_clock(&dir, TEST_NOW).unwrap();
    let lib = &inv.libraries[0];
    assert_eq!(lib.games.len(), 1);
    assert!(!lib.games[0].is_ghost);

    let names: Vec<&str> = lib.residue.iter().map(|r| r.dir_name.as_str()).collect();
    assert_eq!(names, vec!["Old Game Leftover", "999"], "残留识别不符");
    assert_eq!(lib.residue[0].kind, SteamResidueKind::Install);
    assert_eq!(lib.residue[0].appid_hint, None);
    assert_eq!(lib.residue[1].kind, SteamResidueKind::Workshop);
    assert_eq!(lib.residue[1].appid_hint, Some(999));
    // 递归大小：2×1024 与 1×700
    assert_eq!(lib.residue[0].size_bytes, 2048);
    assert_eq!(lib.residue[1].size_bytes, 700);
    assert_eq!(lib.residue_size_bytes, 2748);
    // 残留不计入「真实安装」总量
    assert_eq!(lib.total_size_bytes, 1000);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn residue_case_insensitive_and_ghost_appid_not_double_reported() {
    let dir = residue_tmp("case");
    let steamapps = dir.join("steamapps");
    let common = steamapps.join("common");

    // ACF 写 "Portal 2"，磁盘目录实际是 "PORTAL 2"（Windows 大小写不敏感）
    write_files(&common.join("PORTAL 2"), 1, 512);
    std::fs::write(steamapps.join("appmanifest_620.acf"), acf(620, "Portal 2")).unwrap();
    // ghost 游戏（ACF 在、目录不在）的 workshop 内容：游戏条目仍在（appid 630），
    // 不重复报成残留——避免与 ghost 条目双重计数。
    write_files(&steamapps.join("workshop/content/630/111"), 1, 256);
    std::fs::write(
        steamapps.join("appmanifest_630.acf"),
        acf(630, "Ghost Game"),
    )
    .unwrap();

    let inv = inspect_at_with_clock(&dir, TEST_NOW).unwrap();
    let lib = &inv.libraries[0];
    assert!(lib.games.iter().any(|g| g.is_ghost));
    assert!(
        lib.residue.is_empty(),
        "大小写不同的已安装目录与 ghost appid 都不应报残留，got {:?}",
        lib.residue.iter().map(|r| &r.dir_name).collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
