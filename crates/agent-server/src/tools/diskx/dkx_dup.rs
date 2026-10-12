//! 重复文件扫描域（从 diskx.rs 拆分，2026-10-12）。
//! collect_duplicate_files + read_head_hash（两平台版）。

// 顶层 import 仅供 cfg(windows) 的 collect_duplicate_files 使用（Linux 返回降级文案）。
#[cfg(windows)]
use std::collections::BTreeMap;
#[cfg(windows)]
use std::hash::Hasher;
#[cfg(windows)]
use std::io::Read;
#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
use crate::tools::{fmt_bytes, parallel_walker};

#[cfg(windows)]
use super::super::files::PathGuard;

/// 重复文件扫描的硬上限：比大文件扫描略松，因为还要留预算给头部哈希。
#[cfg(windows)]
const MAX_FILES_WALK_DUP: usize = 200_000;
/// 头部哈希读取的字节数：64 KB。够抓绝大多数真实重复，又不会因为扫到几个大文件而拖死。
#[cfg(windows)]
const HEAD_HASH_BYTES: usize = 64 * 1024;

// ── 2. 重复文件（size 分组 + 头部 64KB 哈希） ────────────────────────────

/// 按 size 分组找重复文件；同 size 组内做头部 64KB `DefaultHasher` 判定。
///
/// 局限性（写入输出末尾）：
/// - 只比对文件头 64KB，未做全文件哈希；
/// - 同 size 组内文件数 > top_n 时不深比对，只报"N 个同大小文件（未深度比对）"；
/// - 跳符号链接；文件数上限 200,000。
#[cfg(windows)]
pub fn collect_duplicate_files(path: String, top_n: usize) -> Result<String, String> {
    let p = Path::new(&path);
    // 只读口径（check_read）：放行盘根枚举（AI 查全盘重复文件的核心入口）。
    PathGuard
        .check_read(p)
        .map_err(|e| format!("路径被安全守卫拒绝：{e}"))?;
    let n = if top_n == 0 { 20 } else { top_n.clamp(5, 100) };

    // 收集 (size, path)，按 size 分组。用 BTreeMap<u64, Vec<String>> 保持 size 升序。
    let mut by_size: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    let mut files_seen = 0usize;
    let mut truncated = false;

    for entry in parallel_walker(p) {
        let Ok(entry) = entry else { continue };
        if files_seen >= MAX_FILES_WALK_DUP {
            truncated = true;
            break;
        }
        if entry.path_is_symlink() {
            continue;
        }
        let ft = entry.file_type();
        if !ft.is_file() {
            continue;
        }
        // 0 字节文件跳过：Windows 上到处都是（.gitkeep / 空 log 等），会淹没真实重复。
        let Some(len) = entry.metadata().map(|m| m.len()).ok().filter(|&x| x > 0) else {
            files_seen += 1;
            continue;
        };
        files_seen += 1;
        by_size
            .entry(len)
            .or_default()
            .push(entry.path().display().to_string());
    }

    // 候选：size 分组内文件数 ≥ 2。
    let dup_size_groups: Vec<(&u64, &Vec<String>)> =
        by_size.iter().filter(|(_, v)| v.len() >= 2).collect();

    let mut total_dup_groups = 0usize;
    let mut lines: Vec<String> = Vec::new();

    for &(size, members) in dup_size_groups.iter().take(n) {
        let sz = *size;
        // 组员数超过 top_n 上限时，不做头部哈希（避免 N+1 大文件读取），直接标注未比对。
        if members.len() > n {
            lines.push(format!(
                "- 重复组 {} · 共 {} 个文件 · {}（同大小，未深度比对）",
                total_dup_groups + 1,
                members.len(),
                fmt_bytes(sz)
            ));
            for m in members.iter().take(6) {
                lines.push(format!("  · {m}"));
            }
            if members.len() > 6 {
                lines.push(format!("  · ……（还有 {} 个未列出）", members.len() - 6));
            }
            total_dup_groups += 1;
            continue;
        }

        // 头部 64KB 哈希：DefaultHasher 对每个候选文件读前 64KB 计算。
        let mut hash_map: BTreeMap<u64, Vec<String>> = BTreeMap::new();
        for m in members {
            let hash = match read_head_hash(m) {
                Some(h) => h,
                None => {
                    // 打开失败（文件被占用 / 无权限 / 已被删）：不计入，静默跳过。
                    continue;
                }
            };
            hash_map.entry(hash).or_default().push(m.clone());
        }
        // 头部哈希相同 → 判为重复组。
        for group in hash_map.values() {
            if group.len() < 2 {
                continue;
            }
            lines.push(format!(
                "- 重复组 {} · 共 {} 个文件 · {}",
                total_dup_groups + 1,
                group.len(),
                fmt_bytes(sz)
            ));
            for m in group.iter() {
                lines.push(format!("  · {m}"));
            }
            total_dup_groups += 1;
        }
    }

    if total_dup_groups == 0 {
        return Ok(format!(
            "未发现重复文件（按大小+头部哈希，共扫描 {files_seen} 个文件）\n\
             说明：只比对文件头部 {0}，未做全文件哈希；同大小大文件组（组员数 > 上限）不深度比对。",
            fmt_bytes(HEAD_HASH_BYTES as u64)
        ));
    }

    let mut s = format!("发现 {total_dup_groups} 个重复组：\n");
    s.push_str(&lines.join("\n"));
    s.push_str(&format!(
        "\n共扫描 {files_seen} 个文件，输出 {total_dup_groups} 个重复组（按大小降序取 Top {n}）\n\
         说明：只比对文件头部 {0}，未做全文件哈希；同大小大文件组（组员数 > 上限）不深度比对；跳符号链接防环；文件数上限 {MAX_FILES_WALK_DUP}，超出标 truncated。",
        fmt_bytes(HEAD_HASH_BYTES as u64)
    ));
    if truncated {
        s.push_str(&format!(
            "\n⚠ 已达遍历上限 {MAX_FILES_WALK_DUP} 个文件，结果不完整"
        ));
    }
    Ok(s)
}

/// 读文件头部 [`HEAD_HASH_BYTES`] 字节并计算 `DefaultHasher` 哈希。
/// 打不开文件（被占用 / 权限 / 已被删）返回 `None`（不计入判定）。
/// 读文件头部 64KB 计算 FNV-1a 哈希；失败返回 None（文件可能被占用/消失）。
#[cfg(windows)]
fn read_head_hash(path: &str) -> Option<u64> {
    let mut f = std::fs::File::open(path).ok()?;
    // 栈上 buffer，避免小文件分配 + 大文件拖死。
    let mut buf = vec![0u8; HEAD_HASH_BYTES];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(&buf);
    Some(hasher.finish())
}

#[cfg(not(windows))]
pub fn collect_duplicate_files(_path: String, _top_n: usize) -> Result<String, String> {
    Ok("当前平台不是 Windows，重复文件扫描不可用".into())
}
