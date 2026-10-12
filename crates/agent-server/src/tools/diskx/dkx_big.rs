//! 大文件 Top N 扫描域（从 diskx.rs 拆分，2026-10-12）。
//! collect_biggest_files（两平台版）。

// 顶层 import 仅供 cfg(windows) 的 collect_biggest_files 使用（Linux 返回降级文案）。
#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
use crate::tools::{fmt_bytes, parallel_walker};

#[cfg(windows)]
use super::super::files::PathGuard;

/// 大文件扫描的硬上限：单次遍历最多看这么多文件（超过就 break 并标 truncated）。
#[cfg(windows)]
const MAX_FILES_WALK: usize = 500_000;

// ── 1. 大文件 Top N ──────────────────────────────────────────────────────

/// 递归扫描某目录下所有文件大小，返回 Top N 最大。
///
/// 内存保护：最多看 [`MAX_FILES_WALK`] 个文件（超过就 break 并标 truncated）。
/// 排序：大小降序，同大小按路径字典序（BTreeMap 内部已按路径升序，稳定且确定）。
#[cfg(windows)]
pub fn collect_biggest_files(path: String, top_n: usize) -> Result<String, String> {
    let p = Path::new(&path);
    // 只读口径（check_read）：放行盘根枚举（AI 分析全盘大文件分布的核心入口）。
    // 系统目录 / 用户主目录根仍拒绝。
    PathGuard
        .check_read(p)
        .map_err(|e| format!("路径被安全守卫拒绝：{e}"))?;
    let n = if top_n == 0 { 20 } else { top_n.clamp(5, 100) };

    let mut files: Vec<(u64, String)> = Vec::with_capacity(4096);
    let mut truncated = false;
    let mut files_seen = 0usize;

    // parallel_walker 已配 skip_hidden(false) / follow_links(false) / max_depth(48)。
    for entry in parallel_walker(p) {
        let Ok(entry) = entry else { continue };
        // 内存保护：先看文件数，超过上限就 break（jwalk 是并行遍历，超限时静默退）。
        if files_seen >= MAX_FILES_WALK {
            truncated = true;
            break;
        }
        // 跳符号链接防环（parallel_walker 已 follow_links(false)，此处再保险）。
        if entry.path_is_symlink() {
            continue;
        }
        // 只认普通文件，跳过目录 / 特殊类型。
        let ft = entry.file_type();
        if !ft.is_file() {
            continue;
        }
        let Some(len) = entry.metadata().map(|m| m.len()).ok().filter(|&x| x > 0) else {
            // 元数据读不到或 0 字节：直接跳过，不进候选池。
            files_seen += 1;
            continue;
        };
        files_seen += 1;
        files.push((len, entry.path().display().to_string()));
    }

    if files.is_empty() {
        return Ok("该目录下没有文件".into());
    }

    // 大小降序，同大小按路径字典序（files 内部已按路径稳定，二次排序稳定即可）。
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    files.truncate(n);

    let total = files_seen;
    let shown = files.len();
    let mut s = String::new();
    for (size, fp) in &files {
        s.push_str(&format!("- {fp} · {0}\n", fmt_bytes(*size)));
    }
    s.push_str(&format!(
        "共扫描 {total} 个文件，取最大 {shown} 个\n\
         （并行遍历；跳符号链接防环；文件数上限 {MAX_FILES_WALK}，超出标 truncated）"
    ));
    if truncated {
        s.push_str(&format!(
            "\n⚠ 已达遍历上限 {MAX_FILES_WALK} 个文件，结果不完整"
        ));
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_biggest_files(_path: String, _top_n: usize) -> Result<String, String> {
    Ok("当前平台不是 Windows，大文件扫描不可用".into())
}
