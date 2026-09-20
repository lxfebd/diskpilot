//! QQ (PC) scaffold safety test — glob 正向断言 + 红线断言。
//!
//! 需求依据：docs/scaffold-requirements/messaging.md §7.7（QQ 实测附录）。
//! 红线断言失败 = glob 写宽了 → 收紧 glob，不要放宽测试。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/qq-pc.toml";

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
fn qq_pc_globs_are_safe() {
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
            "temp-cache",
            "C:/Users/test/AppData/Roaming/Tencent/QQ/STemp/pic/2026-05/123.dat",
        ),
        (
            "temp-cache",
            "C:/Users/test/AppData/Roaming/Tencent/QQ/Temp/base/audio.dat",
        ),
        (
            "received-files",
            "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/FileReceive/2026-05/note.pdf",
        ),
        (
            "received-files",
            "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/FileRecv/2026-05/note.pdf",
        ),
        (
            "received-images",
            "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/ImageRecv/2026-05/img.jpg",
        ),
        (
            "received-videos",
            "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/VideoRecv/2026-05/clip.mp4",
        ),
        (
            "received-stickers",
            "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/CustomFaceRecv/2026-05/sticker.gif",
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
        // QQNT 数据根（聊天库 / 账号状态 / QtWebView cookie）——红线大仓库
        "C:/Users/test/AppData/Roaming/Tencent/QQ/nt_data/QQ/9.x/global/account.cfg",
        "C:/Users/test/AppData/Roaming/Tencent/QQ/nt_data/QQ/9.x/QtWebView/Default/Cookies",
        "C:/Users/test/AppData/Roaming/Tencent/QQ/nt_data/QQ/9.x/global/msg/chat.db",
        // 消息记录
        "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/Messageres/2026-05/msg.dat",
        // 账号状态 / 配置
        "C:/Users/test/AppData/Roaming/Tencent/QQ/Accounts/123456/config.dat",
        "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/config/config.ini",
        "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/login/auth.dat",
        // 收藏
        "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/Favorite/2026-05/item.dat",
        "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/Fav/item.dat",
        // 加密物料
        "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/key/secret.key",
        "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/crypto/secret.bin",
        // db 文件（任意位置）
        "C:/Users/test/AppData/Roaming/Tencent/QQ/123456/ImageRecv/2026-05/index.db",
        // 其它 Tencent app 目录（QQMusic / Wemeet / WeGame 等）绝不被碰
        "C:/Users/test/AppData/Roaming/Tencent/QQMusic/123456/ImageRecv/2026-05/img.jpg",
        "C:/Users/test/AppData/Roaming/Tencent/WeMeet/user/ImageRecv/2026-05/img.jpg",
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
        "qq-pc.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
