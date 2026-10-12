//! 目录占用 Top N 域（从 diskx.rs 拆分，2026-10-12）。
//! collect_top_directories（两平台版）。

use std::collections::BTreeMap;
use std::path::Path;

use crate::tools::{fmt_bytes, parallel_walker};

use super::super::files::PathGuard;

/// 大文件扫描的硬上限：单次遍历最多看这么多文件（超过就 break 并标 truncated）。
const MAX_FILES_WALK: usize = 500_000;

// ── 4. 目录占用 Top N（jwalk 单遍聚合子目录大小） ─────────────────────────

/// 递归统计某目录下**直接子目录**的占用大小（含子目录内所有文件），
/// 返回 Top N 最大子目录（默认 20，钳 5..=100）。
///
/// 语义：只看 `root` 的**第一层**子目录（不嵌套展开——用户问「哪个文件夹
/// 占了大头」要的是同级对比）；单遍 jwalk 并行遍历，每遇到 `root/xxx/...`
/// 的路径就把文件大小累加到第一层子目录名下。文件上限 [`MAX_FILES_WALK`]。
#[cfg(windows)]
pub fn collect_top_directories(path: String, top_n: usize) -> Result<String, String> {
    let p = Path::new(&path);
    // 只读口径（check_read）：放行盘根枚举——统计盘根直接子目录占用是 AI
    // 分析空间分布的核心入口，不修改任何东西。系统目录/主目录根仍拒绝。
    PathGuard
        .check_read(p)
        .map_err(|e| format!("路径被安全守卫拒绝：{e}"))?;
    let n = if top_n == 0 { 20 } else { top_n.clamp(5, 100) };

    // 目录名 -> (字节, 文件数)。root 直接子项名即第一层目录名。
    let mut dirs: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut total_bytes: u64 = 0;
    let mut total_files = 0usize;
    let mut truncated = false;

    for entry in parallel_walker(p) {
        let Ok(entry) = entry else { continue };
        if total_files >= MAX_FILES_WALK {
            truncated = true;
            break;
        }
        if entry.path_is_symlink() {
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(len) = entry.metadata().map(|m| m.len()).ok().filter(|&x| x > 0) else {
            total_files += 1;
            continue;
        };
        total_files += 1;
        total_bytes = total_bytes.saturating_add(len);
        // 取 root 之后的第一个路径段作为「第一层子目录」名。
        let entry_path = entry.path();
        let rel = entry_path.strip_prefix(p).unwrap_or(entry_path.as_path());
        let top = rel
            .iter()
            .next()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "(根内文件)".to_string());
        let e = dirs.entry(top).or_default();
        e.0 = e.0.saturating_add(len);
        e.1 += 1;
    }

    if dirs.is_empty() {
        return Ok(format!("{} 下没有可统计的子目录（或目录为空）", path));
    }

    let mut items: Vec<(String, u64, u64)> = dirs
        .into_iter()
        .map(|(k, (bytes, files))| (k, bytes, files))
        .collect();
    items.sort_by_key(|x| std::cmp::Reverse(x.1));
    let truncated_items = items.len() > n;
    items.truncate(n);

    let mut s = format!(
        "{} 按直接子目录占用（共 {total_files} 个文件 / {}）：\n",
        path,
        fmt_bytes(total_bytes)
    );
    for (name, bytes, files) in &items {
        let pct = if total_bytes > 0 {
            (*bytes as f64 / total_bytes as f64 * 100.0) as u64
        } else {
            0
        };
        s.push_str(&format!(
            "- {name}：{0}（{files} 个文件，占 {pct}%）\n",
            fmt_bytes(*bytes)
        ));
    }
    s.push_str(&format!(
        "\n说明：只统计第一层子目录（同级对比），每个子目录大小含其全部子级；\
         根目录下直接散落的文件计入「(根内文件)」。文件数上限 {MAX_FILES_WALK}。"
    ));
    if truncated || truncated_items {
        s.push_str(&format!(
            "\n⚠ 已达上限（子目录数 > {n} 或文件数上限），结果不完整"
        ));
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_top_directories(_path: String, _top_n: usize) -> Result<String, String> {
    Ok("当前平台不是 Windows，目录占用统计不可用".into())
}
