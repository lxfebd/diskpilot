//! 文件操作（只读：列目录 / 读文件 / 按文件名查找 / 目录树）—— 全部在 PathGuard 安全边界内。

use std::path::{Path, PathBuf};

use serde::Serialize;

#[cfg(windows)]
mod win_path {
    /// 把路径展开为 Windows 真实长路径：
    /// - GetFullPathNameW：相对路径拼接 + `.` / `..` 段规范化（不要求路径存在）；
    /// - GetLongPathNameW：把 8.3 短名段（`C:\PROGRA~1` → `C:\Program Files`）展开。
    ///
    /// AI / 用户可能以短名形态传路径（`C:\PROGRA~1\...` 或 `C:\SYSTEM~1\...`），
    /// 若守卫只做词法比对，会漏判系统目录（M1 修复）。这里对**实际存在的路径**
    /// 做真展开；无法展开（路径不存在 / 8.3 关闭）时由调用方回退词法
    /// guard_canonical（短名本身不可用，词法兜底足够）。
    pub fn expand_short_name(p: &str) -> Option<String> {
        use std::os::windows::ffi::OsStrExt;

        let wide: Vec<u16> = std::ffi::OsStr::new(p)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // 1) 绝对化 + 展开 . / ..（GetFullPathNameW 不要求路径存在）
        let full = {
            let needed = unsafe {
                GetFullPathNameW(wide.as_ptr(), 0, std::ptr::null_mut(), std::ptr::null_mut())
            };
            if needed == 0 {
                return None;
            }
            let mut buf = vec![0u16; (needed + 1) as usize];
            let n = unsafe {
                GetFullPathNameW(
                    wide.as_ptr(),
                    buf.len() as u32,
                    buf.as_mut_ptr(),
                    std::ptr::null_mut(),
                )
            };
            if n == 0 {
                return None;
            }
            buf.truncate(n as usize);
            buf
        };
        // 2) 8.3 短名 → 长名（GetLongPathNameW 要求路径真实存在）
        let needed = unsafe { GetLongPathNameW(full.as_ptr(), std::ptr::null_mut(), 0) };
        if needed == 0 {
            return None;
        }
        let mut buf = vec![0u16; (needed + 1) as usize];
        let n = unsafe { GetLongPathNameW(full.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if n == 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(String::from_utf16_lossy(&buf))
    }

    #[link(name = "kernel32")]
    extern "system" {
        #[link_name = "GetFullPathNameW"]
        fn GetFullPathNameW(
            lp_file_name: *const u16,
            n_buffer_length: u32,
            lp_buffer: *mut u16,
            lp_file_part: *mut *mut u16,
        ) -> u32;
        #[link_name = "GetLongPathNameW"]
        fn GetLongPathNameW(
            lpsz_short_path: *const u16,
            lpsz_long_path: *mut u16,
            cch_buffer: u32,
        ) -> u32;
    }
}

#[cfg(windows)]
pub fn realpath_or_lexical(p: &Path) -> PathBuf {
    win_path::expand_short_name(&p.to_string_lossy())
        .map(PathBuf::from)
        .unwrap_or_else(|| guard_canonical(p))
}

#[cfg(not(windows))]
pub fn realpath_or_lexical(p: &Path) -> PathBuf {
    guard_canonical(p)
}

/// 路径安全守卫：拒绝盘根、系统目录、用户主目录根等"碰了容易出大事"的位置。
/// 对应主项目「protected_path 守卫」的轻量版：只读工具也需要防 AI 拿错路径
/// 把整个盘/系统目录/主目录列出来刷屏或误读敏感文件。
///
/// 所有系统/主目录都从**环境变量**推导而非硬编码盘符，保证装在任意盘符
/// （C:\Windows 或 D:\Windows）都能正确识别，换机可移植。
#[derive(Debug, Clone, Default)]
pub struct PathGuard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardError {
    Missing,
    Root,
    SystemDir,
    HomeRoot,
}

impl std::fmt::Display for GuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GuardError::Missing => write!(f, "路径不存在"),
            GuardError::Root => write!(f, "盘根目录不可操作"),
            GuardError::SystemDir => write!(f, "系统目录不可操作"),
            GuardError::HomeRoot => write!(f, "用户主目录根不可直接操作"),
        }
    }
}

/// 去掉尾部斜杠/点号的规范化路径（用于环境变量值比较）。
fn normalize(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    let t = s
        .trim_end_matches('\\')
        .trim_end_matches('/')
        .trim_end_matches('.');
    PathBuf::from(t)
}

/// 守卫专用规范化：把 `.` / `..` 段展开后再去掉尾部斜杠，防止
/// `C:/./`、`C:/../`、`C:/WINDOWS/../Windows/...` 这类变体绕过精确比对。
/// 不做磁盘 IO（不 touch 真实路径），纯词法规范化，保证守卫判定可移植。
pub fn guard_canonical(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    let has_drive = s.contains(':');
    let has_leading_sep = s.starts_with('\\') || s.starts_with('/');
    let mut parts: Vec<String> = Vec::new();
    for comp in s.split(['/', '\\']) {
        match comp {
            "" | "." => {}
            ".." => {
                // 盘根上的 .. 视为已在根（不能再退）：只剩盘符段（C:）时不 pop，
                // 其余正常回退一段
                if !parts.is_empty() {
                    let last = parts.last().unwrap();
                    if !(last.len() == 2 && last.ends_with(':')) {
                        parts.pop();
                    }
                }
            }
            c => parts.push(c.to_string()),
        }
    }
    let joined = parts.join("\\");
    let mut out = if has_drive {
        // 含盘符（C:）保持原形态，不加前导斜杠，便于与 normalize 后环境变量比对
        joined
    } else if has_leading_sep {
        format!("\\{joined}")
    } else {
        joined
    };
    // 纯盘符形态（C:）保持原样
    if out == "\\" {
        out.clear();
    }
    PathBuf::from(out)
}

fn os_env_lower(key: &str) -> Option<String> {
    std::env::var_os(key).map(|v| v.to_string_lossy().to_ascii_lowercase())
}

fn is_windows_root(p: &Path) -> bool {
    let g = guard_canonical(p);
    let s = g.to_string_lossy();
    let t = s.trim_end_matches('\\').trim_end_matches('/');
    t.len() == 2 && t.ends_with(':')
}

/// 系统目录判定：优先 `SystemRoot` / `windir` / `ProgramFiles` / `ProgramFiles(x86)` /
/// `ProgramData` 环境变量（跨盘符可移植），并兜底常见固定路径。
/// 匹配语义：仅精确匹配系统根自身（含 `system32` / `syswow64` / `programdata` 等
/// 兜底项），**不**递归下探子目录——否则 `SystemRoot\System32\drivers` 这类
/// 正常可读路径会被误杀。真正的敏感点（系统根本身）已经足够挡住。
fn is_system_dir(p: &Path) -> bool {
    let cand = normalize(&realpath_or_lexical(p))
        .to_string_lossy()
        .to_ascii_lowercase();
    let roots: Vec<String> = [
        os_env_lower("SystemRoot"),
        os_env_lower("windir"),
        os_env_lower("ProgramFiles"),
        os_env_lower("ProgramFiles(x86)"),
        os_env_lower("ProgramData"),
    ]
    .into_iter()
    .flatten()
    .map(|r| r.trim_end_matches('\\').trim_end_matches('/').to_string())
    .collect();

    // 环境变量兜底（C 盘常见固定系统区）
    const FALLBACK: &[&str] = &[
        "c:\\windows",
        "c:\\windows\\system32",
        "c:\\windows\\syswow64",
        "c:\\program files",
        "c:\\program files (x86)",
        "c:\\programdata",
        "c:\\recovery",
        "c:\\system volume information",
        "c:\\$recycle.bin",
    ];

    // 系统区前缀匹配：目录本身或其下任意子路径（如 C:\Windows\System32\hosts）
    // 都算系统区——只读泄漏系统文件内容同样敏感，与写守卫同一口径。
    for r in roots
        .iter()
        .map(|s| s.as_str())
        .chain(FALLBACK.iter().copied())
    {
        if cand == r || cand.starts_with(&format!("{r}\\")) {
            return true;
        }
    }
    false
}

fn is_home_root(p: &Path) -> bool {
    let cand = normalize(&realpath_or_lexical(p))
        .to_string_lossy()
        .to_ascii_lowercase();
    let home = os_env_lower("USERPROFILE").or_else(|| os_env_lower("HOME"));
    match home {
        Some(h) => cand == h.trim_end_matches('\\').trim_end_matches('/'),
        None => false,
    }
}

impl PathGuard {
    /// 校验路径是否可操作：存在 + 非盘根 + 非系统目录 + 非主目录根。
    pub fn check(&self, p: &Path) -> Result<(), GuardError> {
        if !p.exists() {
            return Err(GuardError::Missing);
        }
        if is_windows_root(p) {
            return Err(GuardError::Root);
        }
        if is_system_dir(p) {
            return Err(GuardError::SystemDir);
        }
        if is_home_root(p) {
            return Err(GuardError::HomeRoot);
        }
        Ok(())
    }

    /// 写操作专用校验：除 `check` 的全部规则外，**系统目录下的任何文件/子目录
    /// 也拒绝**（回收站/删除/移动都不能碰 `C:\Windows\...` 里的内容）。
    /// 只读工具仍用 `check`（枚举 Windows 子目录列表本身不危险，防误杀正常路径）。
    pub fn check_for_write(&self, p: &Path) -> Result<(), GuardError> {
        self.check(p)?;
        let g = realpath_or_lexical(p);
        let cand = g.to_string_lossy().to_ascii_lowercase();
        let roots: Vec<String> = [
            os_env_lower("SystemRoot"),
            os_env_lower("windir"),
            os_env_lower("ProgramFiles"),
            os_env_lower("ProgramFiles(x86)"),
            os_env_lower("ProgramData"),
        ]
        .into_iter()
        .flatten()
        .map(|r| r.trim_end_matches('\\').trim_end_matches('/').to_string())
        .collect();
        const FALLBACK: &[&str] = &[
            "c:\\windows",
            "c:\\program files",
            "c:\\program files (x86)",
            "c:\\programdata",
            "c:\\recovery",
            "c:\\system volume information",
            "c:\\$recycle.bin",
        ];
        let root_hits: Vec<String> = roots
            .iter()
            .map(|s| s.as_str())
            .chain(FALLBACK.iter().copied())
            .filter(|r| cand.starts_with(&format!("{r}\\")) || cand == *r)
            .map(|s| s.to_string())
            .collect();
        if !root_hits.is_empty() {
            return Err(GuardError::SystemDir);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: Option<u64>,
}

/// 列出目录直接子项（非递归）。按目录在前、名称排序。
pub fn list_dir(p: &std::path::Path, limit: usize) -> Result<(Vec<DirEntry>, bool), String> {
    let rd = std::fs::read_dir(p).map_err(|e| format!("读取目录失败：{e}"))?;
    let mut items = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        let ft = match e.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let size_bytes = if ft.is_file() {
            e.metadata().map(|m| m.len()).ok()
        } else {
            None
        };
        items.push(DirEntry {
            name,
            path: path.display().to_string(),
            is_dir: ft.is_dir(),
            size_bytes,
        });
    }
    items.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    let truncated = items.len() > limit;
    items.truncate(limit);
    Ok((items, truncated))
}

const MAX_READ_BYTES: usize = 256 * 1024;

/// 读文本文件（UTF-8/UTF-16 视 BOM 自动转码；非文本内容按替换字符处理），
/// 超过 256KB 截断并标记。仅限文件，拒绝目录。
pub fn read_text_file(p: &std::path::Path) -> Result<(String, bool), String> {
    let meta = std::fs::metadata(p).map_err(|e| format!("读取元数据失败：{e}"))?;
    if meta.is_dir() {
        return Err("目标是目录，请传文件路径".into());
    }
    let mut bytes = std::fs::read(p).map_err(|e| format!("读取文件失败：{e}"))?;
    let truncated = bytes.len() > MAX_READ_BYTES;
    bytes.truncate(MAX_READ_BYTES);
    // UTF-16 LE BOM（常见于 .ini/.reg 等 Windows 文本）
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let u16s: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return Ok((String::from_utf16_lossy(&u16s), truncated));
    }
    // UTF-8（含无 BOM）
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

#[derive(Debug, Clone, Serialize)]
pub struct FindHit {
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: Option<u64>,
}

/// 在安全根下按文件名关键字递归查找（大小写不敏感），带文件数硬上限。
/// 用 jwalk 并行遍历（多核提速），跳符号链接防环。
pub fn find_files(
    root: &Path,
    keyword: &str,
    max_hits: usize,
) -> Result<(Vec<FindHit>, bool), String> {
    let kw = keyword.to_ascii_lowercase();
    if kw.is_empty() {
        return Err("查找关键字不能为空".into());
    }
    let mut hits = Vec::new();
    let mut truncated = false;
    let mut seen = 0usize;
    const MAX_SEEN: usize = 500_000; // 防 AI 误传整个盘根名导致遍历到爆

    for entry in parallel_walker(root) {
        let Ok(entry) = entry else { continue };
        seen += 1;
        if hits.len() >= max_hits || seen > MAX_SEEN {
            truncated = true;
            break;
        }
        // 跳符号链接防环
        if entry.path_is_symlink() {
            continue;
        }
        let ft = entry.file_type();
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.contains(&kw) {
            let size = if ft.is_file() {
                entry.metadata().map(|m| m.len()).ok()
            } else {
                None
            };
            hits.push(FindHit {
                path: entry.path().display().to_string(),
                is_dir: ft.is_dir(),
                size_bytes: size,
            });
        }
    }
    hits.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((hits, truncated))
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: Option<u64>,
    pub depth: usize,
}

/// 目录树（深度限制，带大小摘要），供 AI 回答"哪占空间"的结构化补充。
/// 返回浅层目录树而非铺满，避免刷屏。
pub fn file_tree(
    root: &Path,
    max_depth: usize,
    max_nodes: usize,
) -> Result<(Vec<TreeNode>, bool), String> {
    let mut nodes = Vec::new();
    let mut truncated = false;
    for entry in parallel_walker(root) {
        let Ok(entry) = entry else { continue };
        if entry.path_is_symlink() {
            continue; // 防环
        }
        let depth = entry.depth();
        if depth > max_depth.max(1) {
            truncated = true;
            continue;
        }
        if nodes.len() >= max_nodes {
            truncated = true;
            break;
        }
        let ft = entry.file_type();
        let size = if ft.is_file() {
            entry.metadata().map(|m| m.len()).ok()
        } else {
            None
        };
        nodes.push(TreeNode {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path().display().to_string(),
            is_dir: ft.is_dir(),
            size_bytes: size,
            depth,
        });
    }
    Ok((nodes, truncated))
}

/// 递归遍历公共 walker：与主项目 scanner 行为一致
/// （skip_hidden(false) 进 httpd dotted 缓存目录 / 不跟随符号链接防环）。
fn parallel_walker(root: &Path) -> jwalk::WalkDir {
    jwalk::WalkDir::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .max_depth(48)
}

/// undo.jsonl 条目（与主项目 diskpilot-executor 的 `UndoEntry` serde 同构，
/// 让 agent-server 的回收也能写进同一份 `~/.diskpilot/undo.jsonl`，兑现
/// 「一切删除可撤销」承诺）。agent-server 保持零横向依赖，故不 use
/// diskpilot-executor，只镜像字段名——主项目 `list_undo` 反序列化的是
/// JSON 形状，不关心类型来源。
#[derive(serde::Serialize)]
struct UndoEntry {
    timestamp: String,
    #[serde(rename = "action")]
    action: &'static str,
    source: String,
    destination: Option<String>,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes_freed: Option<u64>,
}

/// 把 `DISKPILOT_UNDO_LOG` 指向的文件追加一行 undo 条目。
/// 主进程 agent.rs spawn agent-server 时注入该环境变量；未注入（直接跑
/// agent-server / 测试）时静默跳过——写 undo 是增强，不能阻塞回收本身。
/// 纯追加（OpenOptions append），与主项目 write_log 同语义。
fn append_undo(entries: &[UndoEntry]) {
    let Some(path) = std::env::var_os("DISKPILOT_UNDO_LOG") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    use std::io::Write;
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    for e in entries {
        if let Ok(line) = serde_json::to_string(e) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// 统计路径占用的真实字节：文件取 len；目录递归求和（jwalk，与 scanner
/// 同款并行遍历）。字节用于 undo 条目的 `bytes_freed`（前端「已清理」展示
/// 真实值，不再一律标预估）。
fn path_bytes(p: &std::path::Path) -> Option<u64> {
    if p.is_file() {
        return p.metadata().ok().map(|m| m.len());
    }
    if p.is_dir() {
        let mut sum = 0u64;
        for entry in parallel_walker(p) {
            let Ok(entry) = entry else { continue };
            if entry.file_type().is_file() {
                sum = sum.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
            }
        }
        return Some(sum);
    }
    None
}

/// 写操作确认令牌（纵深防御第二层）：只有经主进程确认门（agent.rs 注入
/// `DISKPILOT_CONFIRMED=1`）的调用才放行。绕过主进程直连 agent-server
/// （任意 MCP 客户端直接发 tools/call）时该环境变量缺失，一律拒绝——
/// 第一层是主进程 `confirmed==Some(true)` 硬门，这层兜住进程级绕过。
pub fn require_confirmed() -> Result<(), String> {
    if std::env::var_os("DISKPILOT_CONFIRMED").is_some() {
        Ok(())
    } else {
        Err("agent-server:confirm: 此写操作必须经 DiskPilot 确认门（confirmed=true）后执行；检测到绕过主进程的直接调用，已拒绝。".into())
    }
}

/// 把单个文件/目录移入系统回收站（写操作，**可逆**）。**由主项目 agent.rs
/// 桥接层做 confirmed 确认门**，本函数只负责「能否安全回收 + 执行」。
/// 守卫（违反即拒绝）：
/// - 路径不存在 → 拒绝；
/// - 盘根 / 系统目录 / 用户主目录根 → 拒绝（复用 PathGuard，回收站也不能碰
///   这些位置）；
/// - 循环调用自身不存在——agent-server 无界面，只提供工具接口。
///
/// 回收成功即写一条 undo 日志（DISKPILOT_UNDO_LOG，与主项目同文件），并
/// 统计实际释放字节（bytes_freed）。
/// 回收失败（被占用/无权限）明确报错，不静默；绝不 delete，只进回收站。
#[cfg(windows)]
pub fn recycle_file(path: &str) -> Result<String, String> {
    require_confirmed()?;
    let p = std::path::Path::new(path.trim());
    if p.as_os_str().is_empty() {
        return Err("路径不能为空".into());
    }
    let guard = PathGuard;
    // 写操作：系统目录下的文件（cmd.exe 等）也拒绝，不只拦系统根自身
    if let Err(e) = guard.check_for_write(p) {
        return Err(format!("拒绝：{e}（安全守卫，回收站也不可操作此位置）"));
    }
    // 回收前统计真实字节（回收成功后路径已消失，必须提前取）。
    let bytes = path_bytes(p).unwrap_or(0);
    // 复用 workspace trash crate（与主项目 executor 同款，含误报兜底语义）
    let (ok, message) = match trash::delete(p) {
        Ok(()) => (true, format!("已将「{}」移入回收站（可还原）", p.display())),
        Err(e) => {
            if !p.exists() {
                // 与主项目 executor 同款：报错但路径已消失 = 实际已回收
                (
                    true,
                    format!(
                        "已将「{}」移入回收站（系统返回警告「{e}」，但路径已消失，判定为已回收，可还原）",
                        p.display()
                    ),
                )
            } else {
                (
                    false,
                    format!(
                        "回收失败：{}（路径仍在，可能被占用或权限不足；文件未被删除）",
                        e
                    ),
                )
            }
        }
    };
    if ok {
        append_undo(std::slice::from_ref(&UndoEntry {
            timestamp: now_rfc3339(),
            action: "recycle",
            source: p.display().to_string(),
            destination: None,
            reason: "AI 回收（已确认）".into(),
            bytes_freed: Some(bytes),
        }));
    }
    Ok(message)
}

#[cfg(not(windows))]
pub fn recycle_file(_path: &str) -> Result<String, String> {
    Ok("当前平台不是 Windows，回收站操作不可用".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn guard_canonical_rewrites_dot_dotdot() {
        // 变体路径 → 展开后的规范形态（纯词法，不 touch 磁盘）
        assert_eq!(guard_canonical(Path::new("C:/./")).to_string_lossy(), "C:");
        assert_eq!(guard_canonical(Path::new("C:/../")).to_string_lossy(), "C:");
        assert_eq!(
            guard_canonical(Path::new("C:/Windows/../Windows/System32")).to_string_lossy(),
            "C:\\Windows\\System32"
        );
        assert_eq!(
            guard_canonical(Path::new("C:/Windows/")).to_string_lossy(),
            "C:\\Windows"
        );
    }

    #[test]
    fn guard_canonical_is_windows_root_variants() {
        // 变体盘根应被识别为盘根
        for p in ["C:/", "C:/./", "C:/../", "C:/.././"] {
            assert!(is_windows_root(Path::new(p)), "{p} 应识别为盘根");
        }
        assert!(
            !is_windows_root(Path::new("C:/Windows")),
            "C:/Windows 不是盘根"
        );
    }

    #[test]
    fn check_for_write_rejects_system_subdir() {
        let g = PathGuard;
        // 系统目录下的文件：写操作必须拒绝（只读 check 同口径前缀匹配后同样拒绝）
        let p = Path::new("C:/Windows/System32/cmd.exe");
        assert!(
            matches!(
                g.check(p),
                Err(GuardError::SystemDir) | Err(GuardError::Missing)
            ),
            "只读 check 也应拒绝系统区子路径"
        );
        // 存在性前置：本机测试时若路径不存在会先报 Missing，属正确守卫路径；
        // 核心断言是 check_for_write 不会因"只是系统子目录"而放行
        match g.check_for_write(p) {
            Err(GuardError::Missing) | Err(GuardError::SystemDir) => {}
            Ok(()) => panic!("check_for_write 放行了系统目录内文件"),
            Err(e) => panic!("unexpected: {e}"),
        }
    }

    #[test]
    fn check_rejects_windows_root_variants() {
        let g = PathGuard;
        for p in ["C:/Windows", "C:/Windows/System32"] {
            // 系统根自身与系统区子路径 → SystemDir（只读守卫同口径前缀匹配，
            // 避免 read_file 把 C:\Windows\System32\hosts 这类系统文件内容泄漏出去）
            let r = g.check(Path::new(p));
            assert!(
                matches!(r, Err(GuardError::SystemDir)) || matches!(r, Err(GuardError::Missing)),
                "{p} check 结果异常: {r:?}"
            );
        }
    }

    /// M1 回归：8.3 短名（C:\PROGRA~1 / C:\SYSTEM~1 等）必须被展开后识别为
    /// 系统目录，不能只靠词法比对漏过去。仅在有该短名的 Windows 上断言
    /// （非 Windows 平台词法展开路径自带 `..` 处理，短名不存在，跳过）。
    #[test]
    fn system_dir_short_name_is_rejected() {
        let g = PathGuard;
        #[cfg(windows)]
        {
            // FALLBACK 名单内系统目录的 8.3 短名（均为本机实测存在的短名）：
            // PROGRA~1→Program Files / PROGRA~2→Program Files (x86) /
            // PROGRA~3→ProgramData / SYSTEM~1→System Volume Information / WIND~1→Windows
            for short in [
                "C:\\PROGRA~1",
                "C:\\PROGRA~2",
                "C:\\PROGRA~3",
                "C:\\SYSTEM~1",
                "C:\\WIND~1",
            ] {
                if !Path::new(short).exists() {
                    continue; // 该机无此短名（8.3 关闭或异名），跳过
                }
                let r = g.check(Path::new(short));
                assert!(
                    matches!(r, Err(GuardError::SystemDir)),
                    "8.3 短名 {short} 应被识别为系统目录，got: {r:?}"
                );
            }
        }
        #[cfg(not(windows))]
        {
            // 非 Windows 只需保证不会崩 + 存在性检查仍生效即可
            let r = g.check(Path::new("/System/Volume"));
            assert!(matches!(r, Ok(()) | Err(GuardError::Missing)));
        }
    }

    /// M1 回归：词法 + 真实路径组合的守卫列表——短名段与 `..` 同时出现也必须拦。
    #[test]
    fn system_dir_mixed_short_and_dotdot() {
        let g = PathGuard;
        #[cfg(windows)]
        {
            // C:\PROGRA~1\..\PROGRA~1 ⇒ C:\Program Files（展开后系统目录）
            for mixed in ["C:\\PROGRA~1\\.", "C:\\PROGRA~1\\..\\PROGRA~1"] {
                if !Path::new(&mixed.replace('\\', "/")).exists() && !Path::new(&mixed).exists() {
                    continue;
                }
                let r = g.check(Path::new(mixed));
                assert!(
                    matches!(r, Err(GuardError::SystemDir) | Err(GuardError::Missing)),
                    "混合路径 {mixed} 应被守卫拦截，got: {r:?}"
                );
            }
        }
    }

    /// M1 回归：check_for_write 对系统目录**子项**（用 8.3 短名前缀）也必须拒绝。
    #[test]
    fn check_for_write_short_name_prefix_rejected() {
        let g = PathGuard;
        #[cfg(windows)]
        {
            for short_root in ["C:\\PROGRA~1", "C:\\PROGRA~3", "C:\\SYSTEM~1"] {
                if !Path::new(short_root).exists() {
                    continue;
                }
                // 子项使用短名前缀（目录本身真实存在；文件可不存在——存在性前置
                // 会先给 Missing，但核心是绝不能放行系统目录前缀的写）
                let p = Path::new(short_root).join("anyfile.tmp");
                match g.check_for_write(&p) {
                    Err(GuardError::Missing) | Err(GuardError::SystemDir) => {}
                    Ok(()) => panic!("check_for_write 放行了系统短名前缀子项: {}", p.display()),
                    Err(e) => panic!("unexpected: {e}"),
                }
            }
        }
    }

    /// 写操作确认层回归：`require_confirmed` 必须挡住「未过主进程确认门」的
    /// 直接调用（DISKPILOT_CONFIRMED 缺失），并在令牌存在时放行。
    /// 这是进程级纵深的第二道防线——第一道是主进程 `confirmed==Some(true)` 硬门，
    /// 这层兜住任意 MCP 客户端直连 agent-server 绕过主项目的路径。
    #[test]
    fn require_confirmed_guards_write_tools() {
        let prev = std::env::var_os("DISKPILOT_CONFIRMED");
        // 缺失令牌 → 拒绝（任意 MCP 客户端直连时的默认状态）
        std::env::remove_var("DISKPILOT_CONFIRMED");
        let e = require_confirmed().expect_err("无确认令牌必须拒绝");
        assert!(e.contains("confirm"), "拒绝文案应点明确认门: {e}");

        // 令牌存在（主进程已校验 confirmed=true 后注入）→ 放行
        std::env::set_var("DISKPILOT_CONFIRMED", "1");
        require_confirmed().expect("确认令牌存在应放行");

        match prev {
            Some(v) => std::env::set_var("DISKPILOT_CONFIRMED", v),
            None => std::env::remove_var("DISKPILOT_CONFIRMED"),
        }
    }

    /// undo 落盘回归：append_undo 必须把条目追加进 `DISKPILOT_UNDO_LOG`
    /// 指向的文件（纯追加，行尾换行），供主项目 list_undo 消费。
    #[test]
    fn append_undo_writes_line_to_env_log() {
        let dir = std::env::temp_dir().join(format!("dp-agent-undo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("undo.jsonl");
        let prev = std::env::var_os("DISKPILOT_UNDO_LOG");
        std::env::set_var("DISKPILOT_UNDO_LOG", &log);
        append_undo(&[UndoEntry {
            timestamp: "2026-09-20T00:00:00Z".into(),
            action: "recycle",
            source: "C:/tmp/victim.dat".into(),
            destination: None,
            reason: "AI 回收（已确认）".into(),
            bytes_freed: Some(4096),
        }]);
        append_undo(&[UndoEntry {
            timestamp: "2026-09-20T00:00:01Z".into(),
            action: "recycle",
            source: "C:/tmp/other.dat".into(),
            destination: None,
            reason: "AI 回收（已确认）".into(),
            bytes_freed: None,
        }]);
        match prev {
            Some(v) => std::env::set_var("DISKPILOT_UNDO_LOG", v),
            None => std::env::remove_var("DISKPILOT_UNDO_LOG"),
        }
        let raw = std::fs::read_to_string(&log).expect("undo 日志应存在");
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2, "应追加两行: {raw}");
        let first: serde_json::Value = serde_json::from_str(lines[0]).expect("首行应为合法 JSON");
        assert_eq!(first["action"], "recycle");
        assert_eq!(first["source"], "C:/tmp/victim.dat");
        assert_eq!(first["bytes_freed"], 4096);
        let second: serde_json::Value = serde_json::from_str(lines[1]).expect("次行应为合法 JSON");
        assert!(second.get("bytes_freed").is_none(), "None 字节应省略键");
    }

    /// path_bytes 统计回归：单文件返回 len，目录递归求和。
    #[test]
    fn path_bytes_counts_file_and_dir() {
        let dir = std::env::temp_dir().join(format!("dp-agent-bytes-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.dat"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("sub/b.dat"), vec![0u8; 250]).unwrap();
        assert_eq!(path_bytes(&dir.join("a.dat")), Some(100));
        assert_eq!(path_bytes(&dir), Some(350));
        assert_eq!(path_bytes(&dir.join("nope.dat")), None);
    }
}
