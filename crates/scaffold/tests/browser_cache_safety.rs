//! Safety test for browser-cache scaffold — 红线断言保证 glob 只命中浏览器缓存目录。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test browser_cache_safety`
//!
//! 设计要点（与 scaffolds/browser-cache.toml 一一对应）：
//! - 正向：Chrome / Edge 的 Cache、Code Cache、GPUCache、GrShaderCache、
//!   Dawn 系列、ShaderCache、Service Worker CacheStorage 必须能命中。
//! - 红线：书签、密码、登录态、历史、Local Storage、IndexedDB、Cookie、
//!   Preferences、扩展数据、用户下载文件——一律必须零命中。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/browser-cache.toml";

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
        if bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
            if let Some(end) = bytes[i + 2..].iter().position(|&b| b == b'}') {
                let var = std::str::from_utf8(&bytes[i + 2..i + 2 + end]).unwrap_or("");
                if let Ok(v) = std::env::var(var) {
                    out.push_str(&v.replace('\\', "/"));
                    i += end + 3;
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
fn browser_cache_globs_are_safe() {
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
    // 正向断言：每个 scope 至少一条命中路径
    // 注意：browser-cache 的 scope 是 directory granularity，glob 以缓存目录名
    // 结尾（无尾部 /**），所以正向路径必须是缓存目录本身，而非其内部文件。
    // ========================================================================
    let positives: &[(&str, &str)] = &[
        (
            "chrome-cache",
            "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Cache",
        ),
        (
            "chrome-cache",
            "C:/Users/test/AppData/Local/Google/Chrome/User Data/Profile 1/Code Cache",
        ),
        (
            "chrome-cache",
            "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/GPUCache",
        ),
        (
            "chrome-serviceworker-cache",
            "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Service Worker/CacheStorage",
        ),
        (
            "edge-cache",
            "C:/Users/test/AppData/Local/Microsoft/Edge/User Data/Default/Cache",
        ),
        (
            "edge-cache",
            "C:/Users/test/AppData/Local/Microsoft/Edge/User Data/Default/ShaderCache",
        ),
        (
            "edge-serviceworker-cache",
            "C:/Users/test/AppData/Local/Microsoft/Edge/User Data/Default/Service Worker/CacheStorage",
        ),
        // Linux 安装根（~/.config/google-chrome 与 ~/.config/microsoft-edge）
        (
            "chrome-cache",
            "/home/test/.config/google-chrome/Default/Cache",
        ),
        (
            "edge-cache",
            "/home/test/.config/microsoft-edge/Default/GPUCache",
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
    // 红线断言：这些路径必须不被任何 scope 命中
    // ========================================================================
    let red_lines: &[&str] = &[
        // 书签 / 密码 / 登录态 / 历史（多为 .db 或 .json 但位于 User Data 根）
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Bookmarks",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Login Data",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/History",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Preferences",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Cookies",
        // 站点数据（Local Storage / IndexedDB / CacheStorage 之外的用户态）
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Local Storage/leveldb/000003.log",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/IndexedDB/https_www.example.com",
        // 扩展程序数据
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Extensions/abc/manifest.json",
        // 通用红线（继承 CLAUDE.md）：任何 .db 都不允许
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/db_storage/MMKV/data.db",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/config/account.cfg",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/login/auth.dat",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Accounts/auth.json",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Favorite/fav1",
        // 用户下载 / 桌面资料绝不碰
        "C:/Users/test/Downloads/Chrome/cache_pack.zip",
        "C:/Users/test/Desktop/Cache/notes.txt",
        // 伪造同名结构（裸 `**/` 前缀 + literal_separator=false 会误命中）
        "C:/Users/test/Desktop/Google/Chrome/User Data/Default/Cache",
        "C:/Users/test/Documents/Microsoft/Edge/User Data/Default/ShaderCache",
        "/home/test/Documents/google-chrome/Default/Code Cache",
        "/home/test/Downloads/microsoft-edge/Default/Cache",
        // 更深的嵌套：确保不是 "含 Cache 字样就匹配"
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/IndexedDB/Cache/api.example.com",
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
        "browser-cache.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
