//! 路径安全守卫（从 files.rs 拆分，2026-10-12）。
//! PathGuard + GuardError + guard_canonical + normalize + 系统/主目录判定。
//!
//! `realpath_or_lexical` 与 `win_path` 保留在父模块 `tools::files`。

use std::path::{Path, PathBuf};

use super::realpath_or_lexical;

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

pub(crate) fn is_windows_root(p: &Path) -> bool {
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

    /// 只读工具专用校验：盘根**放行**（`list_dir("D:\")` 这类只读枚举盘根本身
    /// 不修改任何东西，是 AI 分析空间分布的必要入口；且 list_dir 有 limit
    /// 截断，不会刷屏）。系统目录 / 用户主目录根 / 不存在路径仍照常拦截——
    /// 那些位置列出内容同样可能泄漏敏感信息，只读也不放行。
    pub fn check_read(&self, p: &Path) -> Result<(), GuardError> {
        if !p.exists() {
            return Err(GuardError::Missing);
        }
        if is_system_dir(p) {
            return Err(GuardError::SystemDir);
        }
        if is_home_root(p) {
            return Err(GuardError::HomeRoot);
        }
        Ok(())
    }
}
