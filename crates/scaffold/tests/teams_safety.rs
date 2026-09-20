//! Teams scaffold safety test — glob 正向断言 + 红线断言。
//!
//! 需求依据：docs/scaffold-requirements/appendix-teams.md（Teams 实测附录）。
//! 红线断言失败 = glob 写宽了 → 收紧 glob，不要放宽测试。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/teams.toml";

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
fn teams_globs_are_safe() {
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
        // 1.x 经典版（Electron）缓存区
        ("cache", "C:/Users/test/AppData/Roaming/Microsoft/Teams/Cache/Cache_Data/f_000001"),
        ("code-cache", "C:/Users/test/AppData/Roaming/Microsoft/Teams/Code Cache/js/index.js"),
        ("gpu-cache", "C:/Users/test/AppData/Roaming/Microsoft/Teams/GPUCache/data_0"),
        ("gpu-cache", "C:/Users/test/AppData/Roaming/Microsoft/Teams/DawnGraphiteCache/data_0"),
        ("gpu-cache", "C:/Users/test/AppData/Roaming/Microsoft/Teams/DawnWebGPUCache/data_0"),
        ("blob-storage", "C:/Users/test/AppData/Roaming/Microsoft/Teams/blob_storage/1a2b3c/00000001"),
        ("crashpad", "C:/Users/test/AppData/Roaming/Microsoft/Teams/Crashpad/reports/2026-05-01.dmp"),
        // 会议音视频临时 + 日志
        (
            "media-stack",
            "C:/Users/test/AppData/Roaming/Microsoft/Teams/media-stack/video/20260501_103042.m4v",
        ),
        (
            "app-logs",
            "C:/Users/test/AppData/Roaming/Microsoft/Teams/logs/main.log",
        ),
        (
            "app-logs",
            "C:/Users/test/AppData/Roaming/Microsoft/Teams/log/main.log",
        ),
        (
            "app-logs",
            "C:/Users/test/AppData/Local/Microsoft/Teams/logs/renderer.log",
        ),
        (
            "app-logs",
            "C:/Users/test/AppData/Local/Microsoft/Teams/log/20260501.log",
        ),
        // 2.x 新版（MSTeams_8wekyb3d8bbwe 包）
        (
            "new-app-cache",
            "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AC/8a8f/000001.tmp",
        ),
        (
            "new-local-cache",
            "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/LocalCache/cached/video.mp4",
        ),
        (
            "new-temp-state",
            "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/TempState/scratch.dat",
        ),
        (
            "new-chromium-cache",
            "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/Cache/Cache_Data/f_000002",
        ),
        (
            "new-chromium-cache",
            "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Roaming/GPUCache/data_0",
        ),
        (
            "new-chromium-cache",
            "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/Code Cache/js/app.js",
        ),
        (
            "new-chromium-cache",
            "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/blob_storage/1a2b3c/000001",
        ),
        (
            "new-chromium-cache",
            "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/DawnWebGPUCache/data_0",
        ),
        (
            "new-chromium-cache",
            "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/Crashpad/reports/2026-05-01.dmp",
        ),
        (
            "new-chromium-cache",
            "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/ShaderCache/data_0",
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
        // --- 1.x 经典版：登录态 / 数据存储（删了要重新登录 / 丢聊天历史）---
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Local Storage/leveldb/000003.log",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Local Storage/leveldb/LOG",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Session Storage/000003.log",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/IndexedDB/https_teams.microsoft.com_0.indexeddb.leveldb/000003.log",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/IndexedDB/https_teams.microsoft.com_0.indexeddb.leveldb/LOCK",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Network/Cookies",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Network/Trust Tokens/index",
        // --- 1.x 经典版：Chromium 凭据 / 顶层状态 ---
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Preferences",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/First Run",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Local State",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Cookies",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Login Data",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Widevine CDM/decryption_keys.json",
        // --- 账号配置 / 设置（显式列入红线，任务要求）---
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/desktop-config/10582a0a-0000-0000-0000-000000000000/config.dat",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/app_settings.json",
        // --- 外接程序安装（不碰）---
        "C:/Users/test/AppData/Roaming/Microsoft/Teams Meeting Add-in/install/1.0.0.1/TeamsMeetingAddin.dll",
        // --- 1.x 通用红线族（数据库 / key / config / Favorite）---
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/IndexedDB/https_teams.microsoft.com_0.indexeddb.leveldb/000003.db",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Local Storage/leveldb/000003.log.db",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/key/secret.key",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/config/account.cfg",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Accounts/12345/config.dat",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Favorite/1/file.dat",
        "C:/Users/test/AppData/Roaming/Microsoft/Teams/Fav/item.dat",
        // --- 2.x 新版：状态区（红线，绝不 clean）---
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/LocalState/state.bin",
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/RoamingState/settings.json",
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/Settings/current/settings.dat",
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/SystemAppData/user.dat",
        // --- 2.x 新版：AppData 树里与缓存同级的登录/会话目录 ---
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/Local Storage/leveldb/000003.log",
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/IndexedDB/https_teams.microsoft.com_0.indexeddb.leveldb/000003.log",
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/Network/Cookies",
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/Session Storage/000003.log",
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/Preferences",
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/Local State",
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Local/Login Data",
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Roaming/Local Storage/leveldb/LOG",
        "C:/Users/test/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/AppData/Roaming/Network/Cookies",
        // --- 与 Cache 同名的其它 app（大小写不敏感匹配下不能误伤）---
        "C:/Users/test/AppData/Roaming/SomeApp/Microsoft/Teams/Cache/f_001",
        "C:/Users/test/AppData/Roaming/Microsoft/TeamsCache/f_001",
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
        "teams.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
