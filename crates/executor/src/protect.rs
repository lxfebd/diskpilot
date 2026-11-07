//! 受保护路径判定域：protected_path / protected_path_folded / BLOCKED。
//! 从 lib.rs 拆分（2026-10-12）。executor 的 execute 与 Tauri 命令层共用，
//! 确保任何入口都拦得住盘根/系统目录。先 canonicalize 再字符串折叠兜底。

use std::path::Path;

/// 判断路径是否属于「绝不清理」的受保护目标。executor 的 `execute` 与
/// Tauri 命令层共用这份校验，确保任何入口都拦得住盘根/系统目录。
///
/// 先 `fs::canonicalize` 解析符号链接/接缝（junction）：`C:\Users\<me>\link` 若
/// 真实指向 `C:\Windows`，必须按解析后的目标判定，否则清理会误触系统目录。
/// 注意：**校验用 canonical 路径，动作仍用调用方原路径**——这样拒绝的是
/// 「目标指向受保护目录」的链接本身，不会误删真实 Windows 文件。
/// canonicalize 失败（路径不存在/无权限/含 `..`）时**fail-closed**：退回原有
/// 字符串折叠逻辑（同样能拦文本绕过），绝不因解析失败而放行。
pub fn protected_path(p: &Path) -> Option<&'static str> {
    let canonical = std::fs::canonicalize(p).ok();
    if let Some(real) = canonical.as_deref() {
        if let Some(what) = protected_path_folded(real) {
            return Some(what);
        }
        // 若原路径本身不是 canonical 形式（含链接），且解析后未命中黑名单，
        // 说明它指向一个「正常用户目录」——但仍需继续用原路径的折叠形式
        // 走一遍（防止链接解析路径与黑名单首段差异导致的漏网，见下）。
    }
    protected_path_folded(p)
}

/// `protected_path` 的纯字符串实现：折叠 `\\?\` / `..` / `.` 后按段匹配
/// 盘根 / 系统目录 / 用户主目录根。canonicalize 失败时的 fail-closed 兜底。
fn protected_path_folded(p: &Path) -> Option<&'static str> {
    let raw = p.to_string_lossy();
    // R505：UNC 路径（`\\nas\c$\Windows`、`\\?\UNC\nas\...`）一律拒绝——
    // 网络共享上的「系统目录」无法靠盘符后首段命中 BLOCKED，
    // 且清理动作不该触碰任何网络路径。先判前缀再剥扩展路径前缀，
    // 否则 `\\?\UNC\` 被剥成盘符形态后漏判。
    let low = raw.to_ascii_lowercase();
    // 裸 `\\server\share` 是 UNC；`\\?\UNC\server\share` 也是 UNC；
    // 但 `\\?\C:\` 是本地盘的扩展路径，不算 UNC。
    let is_unc =
        (low.starts_with(r"\\") && !low.starts_with(r"\\?\")) || low.starts_with(r"\\?\unc\");
    if is_unc {
        return Some("a UNC/network share");
    }
    // Windows 扩展路径前缀 `\\?\` 与盘符一样不做区分，先剥掉。
    let s = raw.trim_start_matches(r"\\?\").replace('\\', "/");
    let mut parts: Vec<&str> = s.split('/').collect();
    // 折叠 `..`（`C:/Users/../Windows` → `C:/Windows`），防简单绕过。
    let mut folded: Vec<&str> = Vec::with_capacity(parts.len());
    for seg in parts.drain(..) {
        match seg {
            ".." => {
                folded.pop();
            }
            "" | "." => {}
            seg => folded.push(seg),
        }
    }
    if folded.is_empty() {
        return Some("a filesystem root");
    }
    // Windows 绝对路径首段是盘符（`C:`）。系统目录 / 主目录红线要对**盘符之后**
    // 的段做匹配——直接拿首段比会把 `C:\Windows` 拿去和盘符比，红线整体失效。
    let segs: &[&str] = if folded[0].len() == 2 && folded[0].ends_with(':') {
        if folded.len() == 1 {
            return Some("a drive root");
        }
        &folded[1..]
    } else {
        &folded[..]
    };
    // macOS 的 /etc、/tmp、/var 是合成 symlink，canonicalize 后变成
    // /private/etc、/private/tmp、/private/var——剥掉 /private 前缀让黑名单
    // 照常命中。不会误伤临时区：/private/var/folders 剥掉后首段是 var，
    // 而 var 按设计不在黑名单（与 BLOCKED 注同理）。
    #[cfg(target_os = "macos")]
    let segs: &[&str] = if segs.len() >= 2 && segs[0].eq_ignore_ascii_case("private") {
        &segs[1..]
    } else {
        segs
    };
    let first = segs[0];
    let first_lower = first.to_lowercase();
    const BLOCKED: &[&str] = &[
        // Windows
        "windows",
        "program files",
        "program files (x86)",
        "programdata",
        "$recycle.bin",
        "system volume information",
        "$windows.~bt",
        "$windows.~ws",
        "perflogs",
        "recovery",
        // Unix（Linux/macOS）：只拦「永远不该清、且无用户数据在下面」的系统根。
        // var/srv/media/mnt/run/tmp 首段下混着用户数据（/var/folders 是 mac 的
        // 默认 temp 区），整体列入会把正常临时目录扫描误判为系统目录，故排除。
        "etc",
        "usr",
        "bin",
        "sbin",
        "lib",
        "lib64",
        "boot",
        "dev",
        "proc",
        "sys",
        "root",
        // macOS
        "system",
        "library",
        "applications",
    ];
    if BLOCKED.contains(&first_lower.as_str()) {
        return Some("a system directory");
    }
    // 用户主目录根：盘符后仅 `Users/<名字>` 两层，第三层不存在
    if segs.len() >= 2 && first_lower == "users" {
        let user = segs[1].to_lowercase();
        if segs.len() == 2
            && user != "public"
            && user != "default"
            && user != "default user"
            && user != "all users"
        {
            return Some("a user home directory");
        }
    }
    // Unix 用户主目录：`/home` 父级（len 1）与 `/home/<名字>`（len 2）都不可清理；
    // `/home/<名字>/…` 子目录（len ≥ 3）允许（与 Windows Users 规则对齐）。
    if segs.len() <= 2 && first_lower == "home" {
        return Some("a user home directory");
    }
    None
}
