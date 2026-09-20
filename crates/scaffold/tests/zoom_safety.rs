//! Zoom scaffold safety test — glob 正向断言 + 红线断言。
//!
//! 需求依据：docs/scaffold-requirements/appendix-zoom.md。
//! 红线断言失败 = glob 写宽了 → 收紧 glob，不要放宽测试。
//!
//! Zoom 最重要的红线是「用户录制」：%USERPROFILE%/Documents/Zoom 与
//! ZoomRecordings 是用户内容，误删无法从云端找回。scaffold 里这两个根
//! 只出现在 detect（让卡片可见），绝不进 scope；这里用红线断言锁死。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/zoom.toml";

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
fn zoom_globs_are_safe() {
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
            "data-cache",
            "C:/Users/test/AppData/Roaming/zoom.us/data/WebRTC/video/frame.bin",
        ),
        (
            "data-cache",
            "C:/Users/test/AppData/Roaming/zoom.us/data/media-cache/image/thumbs/1.jpg",
        ),
        (
            "telemetry",
            "C:/Users/test/AppData/Roaming/zoom.us/data/Telemetry/Usage/2026-05-01.json",
        ),
        (
            "logs",
            "C:/Users/test/AppData/Roaming/zoom.us/logs/zoom.exe.log.2026-05-01",
        ),
        (
            "installer-downloads",
            "C:/Users/test/AppData/Roaming/zoom.us/downloads/ZoomInstaller-6.0.0.301.exe",
        ),
        ("electron-cache", "C:/Users/test/AppData/Local/Zoom/Cache"),
        (
            "electron-cache",
            "C:/Users/test/AppData/Local/Zoom/Code Cache",
        ),
        (
            "electron-cache",
            "C:/Users/test/AppData/Local/Zoom/GPUCache",
        ),
        (
            "electron-logs",
            "C:/Users/test/AppData/Local/Zoom/app/logs/zoom.log",
        ),
        (
            "electron-logs",
            "C:/Users/test/AppData/Local/Zoom/app/Crashpad/reports/2026-05-01.dmp",
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
    // 通用红线（crate 内置样本）+ Zoom 特有的「用户录制 / 聊天 DB / 账号配置 /
    // Chromium 凭据 / 加密 key」+ 锚定陷阱（同名的 ZoomCache、zoom.us-extra 等）
    // ========================================================================
    let zoom_red_lines: &[&str] = &[
        // —— Zoom 第一红线：用户录制（%USERPROFILE%/Documents 下，绝不可碰）
        "C:/Users/test/Documents/Zoom/MyMeeting-20260501_090000/MP4/MyMeeting-20260501_090000.mp4",
        "C:/Users/test/Documents/Zoom/MyMeeting-20260501_090000/MP4/MyMeeting-20260501_090000.aitxt",
        "C:/Users/test/Documents/Zoom/Recordings/WeeklyStandup/WeeklyStandup.mp4",
        "C:/Users/test/Documents/ZoomRecordings/Meeting_20260501/Meeting_20260501.mp4",
        "C:/Users/test/Documents/ZoomRecordings/Meeting_20260501/Chat.txt",
        "C:/Users/test/Documents/ZoomRecordings/Meeting_20260501/Thumbnails/1.jpg",
        // —— 账号配置（config 类红线，删了要重新登录 / 丢设置）
        "C:/Users/test/AppData/Roaming/zoom.us/data/zoom.us.conf",
        "C:/Users/test/AppData/Roaming/zoom.us/data/zoom.exe.conf",
        "C:/Users/test/AppData/Roaming/zoom.us/data/config/settings.ini",
        "C:/Users/test/AppData/Roaming/zoom.us/data/config/preferences.json",
        // —— 聊天 / 会议消息数据库（im 目录含 DB，删了历史消息不可恢复）
        "C:/Users/test/AppData/Roaming/zoom.us/data/im/Message.db",
        "C:/Users/test/AppData/Roaming/zoom.us/data/im/Message.db-wal",
        "C:/Users/test/AppData/Roaming/zoom.us/data/im/Message.db-shm",
        "C:/Users/test/AppData/Roaming/zoom.us/data/im/Meetings/Meeting.db",
        // —— 登录态（login / Accounts / 会话）
        "C:/Users/test/AppData/Roaming/zoom.us/data/login/session.dat",
        "C:/Users/test/AppData/Roaming/zoom.us/data/Accounts/2470000000/settings.dat",
        // —— Chromium 登录态与凭据（%LOCALAPPDATA%/Zoom 是 Electron UserData 根）
        "C:/Users/test/AppData/Local/Zoom/Login Data",
        "C:/Users/test/AppData/Local/Zoom/Cookies",
        "C:/Users/test/AppData/Local/Zoom/Local State",
        "C:/Users/test/AppData/Local/Zoom/Preferences",
        "C:/Users/test/AppData/Local/Zoom/First Run",
        "C:/Users/test/AppData/Local/Zoom/Session Storage/000003.log",
        "C:/Users/test/AppData/Local/Zoom/Local Storage/leveldb/000003.log",
        "C:/Users/test/AppData/Local/Zoom/IndexedDB/https_zoom.us_0.indexeddb.leveldb/000003.log",
        "C:/Users/test/AppData/Local/Zoom/Network/Cookies",
        "C:/Users/test/AppData/Local/Zoom/Trust Tokens/index",
        // —— app/ 下的 Chromium 状态与加密 key 材料
        "C:/Users/test/AppData/Local/Zoom/app/Network/Cookies",
        "C:/Users/test/AppData/Local/Zoom/app/Local Storage/leveldb/LOG",
        "C:/Users/test/AppData/Local/Zoom/app/Session Storage/000003.log",
        "C:/Users/test/AppData/Local/Zoom/app/IndexedDB/https_web.zoom.us_0.indexeddb.leveldb/000003.log",
        "C:/Users/test/AppData/Local/Zoom/app/key/secret.key",
        "C:/Users/test/AppData/Local/Zoom/app/crypto/secret.bin",
        // 大小写不敏感匹配下，key / Key / Crypto 都不能放行
        "C:/Users/test/AppData/Local/Zoom/app/Key/secret.key",
        "C:/Users/test/AppData/Local/Zoom/app/Crypto/secret.bin",
        // —— 锚定陷阱：app/ 下只取 logs 与 Crashpad，Cache 类不动
        "C:/Users/test/AppData/Local/Zoom/app/Cache/Cache_Data/f_000001",
        "C:/Users/test/AppData/Local/Zoom/app/GPUCache/data_0",
        // —— 锚定陷阱：WebRTC 只在 data/ 下才认为是缓存；data/ 外的同名目录不碰
        "C:/Users/test/AppData/Roaming/zoom.us/WebRTC/video/frame.bin",
        // —— 锚定陷阱：日志只在 Roaming/zoom.us/logs；data 里的 log 目录不碰
        "C:/Users/test/AppData/Roaming/zoom.us/data/log/zoom.log",
        // —— 锚定陷阱：遥测只在 data/Telemetry；同级的 telemetry-cache 不碰
        "C:/Users/test/AppData/Roaming/zoom.us/data/telemetry-cache/Usage/x.json",
        // —— 锚定陷阱：安装包归档只在 downloads/ 下
        "C:/Users/test/AppData/Roaming/zoom.us/downloads2/ZoomInstaller.exe",
        // —— 同名应用 / 用户自建目录（大小写不敏感匹配下不能误伤）
        "C:/Users/test/AppData/Roaming/zoom.us-extra/data/WebRTC/video/frame.bin",
        "C:/Users/test/AppData/Roaming/ZoomCache/Cache_Data/f_001",
        "C:/Users/test/AppData/Local/ZoomInfo/Cache/Cache_Data/f_001",
        "C:/Users/test/Desktop/Zoom/notes.txt",
        "C:/Users/test/Documents/ZoomCache/export.mp4",
    ];

    let mut violations = Vec::new();
    for p in zoom_red_lines {
        let hits = matching_scopes(&scopes, p);
        if !hits.is_empty() {
            violations.push(format!("`{p}` -> {hits:?}"));
        }
    }
    // 通用红线样本（CLAUDE.md Hard rule #1，crate 内置清单）也必须零命中。
    for p in diskpilot_scaffold::red_line_samples() {
        let hits = matching_scopes(&scopes, p);
        if !hits.is_empty() {
            violations.push(format!("`{p}` -> {hits:?}"));
        }
    }
    assert!(
        violations.is_empty(),
        "zoom.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
