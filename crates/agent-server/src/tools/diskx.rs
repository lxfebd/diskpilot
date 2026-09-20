//! 磁盘/文件深度只读扫描工具（大文件 / 重复文件 / 磁盘 IO 速率）。
//!
//! **纯工具文件**：不接路由，由后续 `tools/mod.rs` 薄壳对接 MCP tool。
//! **全部只读**：不写注册表、不改文件、不动回收站、不改权限；读文件只做头部哈希。
//!
//! ## 本文件导出函数清单
//!
//! 1. `pub fn collect_biggest_files(path: String, top_n: usize) -> Result<String, String>`
//!    递归扫描某目录下所有文件大小，返回 Top N（默认 20，钳 5..=100）。
//!    降序：同大小按路径字典序。最多看 500,000 个文件，超过标 truncated。
//!
//! 2. `pub fn collect_duplicate_files(path: String, top_n: usize) -> Result<String, String>`
//!    按 `size` 分组（≥2 才是候选）→ 组内做头部 64KB `DefaultHasher` 判定，输出重复组。
//!    最多看 200,000 个文件；同 size 组内文件数 > top_n 时不深比对，只报"未深度比对"。
//!
//! 3. `pub fn collect_disk_io_usage(sample_ms: u64) -> Result<String, String>`
//!    走 CIM `Win32_PerfFormattedData_PerfDisk_PhysicalDisk` 取每盘实时读/写速率与占用率。
//!    过滤：`_Total` = 总计，`0 C:` / `1 D:` 等 = 单盘，`HarddiskN` / `2 C:` 等副本忽略。
//!    `sample_ms` 仅为参考提示（PerfFormattedData 已给当前速率，不做真实双采样）。

use std::collections::BTreeMap;
use std::hash::Hasher;
use std::io::Read;
use std::path::Path;

use crate::ps::{json_array_of, ps_capture};

use super::files::PathGuard;

/// 递归遍历公共 walker（与 `files.rs` / `disk.rs` 同款语义）：
/// skip_hidden(false) 进 httpd dotted 缓存目录 / 不跟随符号链接防环 / max_depth(48)。
///
/// 注：`files::parallel_walker` 是模块内 private，无法跨模块复用，故此处复刻一份。
/// 未来若把 files::parallel_walker 提为 pub，可把本函数删掉并改用。
fn parallel_walker(root: &Path) -> jwalk::WalkDir {
    jwalk::WalkDir::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .max_depth(48)
}

// ── 常量与本地格式化 ──────────────────────────────────────────────────────

/// 大文件扫描的硬上限：单次遍历最多看这么多文件（超过就 break 并标 truncated）。
const MAX_FILES_WALK: usize = 500_000;
/// 重复文件扫描的硬上限：比大文件扫描略松，因为还要留预算给头部哈希。
const MAX_FILES_WALK_DUP: usize = 200_000;
/// 头部哈希读取的字节数：64 KB。够抓绝大多数真实重复，又不会因为扫到几个大文件而拖死。
const HEAD_HASH_BYTES: usize = 64 * 1024;

/// 字节数人类可读（与 `hw.rs::fmt_bytes` 同语义；rustc 1.98 安全写法 `{:.*}`，不用 `{:.1f}`）。
fn fmt_bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.*} GB", 1, n as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if n >= 1024 * 1024 {
        format!("{:.*} MB", 1, n as f64 / 1024.0 / 1024.0)
    } else if n >= 1024 {
        format!("{:.*} KB", 0, n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

// ── 1. 大文件 Top N ──────────────────────────────────────────────────────

/// 递归扫描某目录下所有文件大小，返回 Top N 最大。
///
/// 内存保护：最多看 [`MAX_FILES_WALK`] 个文件（超过就 break 并标 truncated）。
/// 排序：大小降序，同大小按路径字典序（BTreeMap 内部已按路径升序，稳定且确定）。
#[cfg(windows)]
pub fn collect_biggest_files(path: String, top_n: usize) -> Result<String, String> {
    let p = Path::new(&path);
    // PathGuard：拒绝盘根 / 系统目录 / 用户主目录根，防止 AI 误传 `C:\` 或 `C:\Users`。
    PathGuard
        .check(p)
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
    PathGuard
        .check(p)
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

// ── 3. 磁盘 IO 使用率（PowerShell + CIM PerfFormattedData） ───────────────

const DISK_IO_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$rows = @(Get-CimInstance Win32_PerfFormattedData_PerfDisk_PhysicalDisk | Where-Object {
    $_.Name -eq '_Total' -or ($_.Name -match '^\d+ [A-Z]:$')
} | Select-Object Name, DiskReadBytesPerSec, DiskWriteBytesPerSec, PercentDiskTime)
$rows | ConvertTo-Json -Depth 4 -Compress
"#;

/// 磁盘 IO 使用率（只读，实时）。
///
/// 走 CIM `Win32_PerfFormattedData_PerfDisk_PhysicalDisk`，取格式化后的当前速率
/// （单位：字节/秒）。过滤规则：`_Total` = 总计，`<整数> <字母>:` = 真实磁盘（例 `0 C:`），
/// `HarddiskN` / 双前缀副本（例 `2 C:`）等全部丢弃。
///
/// 参数 `sample_ms` 仅作参考提示——PerfFormattedData 提供的是当前速率，不做真实双采样。
#[cfg(windows)]
pub fn collect_disk_io_usage(sample_ms: u64) -> Result<String, String> {
    let raw = ps_capture(DISK_IO_PS)?;
    let arr: Vec<serde_json::Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析磁盘性能计数器失败：{e}"))?;

    if arr.is_empty() {
        return Ok(
            "未读取到磁盘性能计数器（Win32_PerfFormattedData_PerfDisk_PhysicalDisk 无返回）".into(),
        );
    }

    let get_u64 = |v: &serde_json::Value, k: &str| -> u64 {
        v.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0)
    };

    let mut lines: Vec<String> = Vec::with_capacity(arr.len());
    for r in &arr {
        let name = r
            .get("Name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let rb = get_u64(r, "DiskReadBytesPerSec");
        let wb = get_u64(r, "DiskWriteBytesPerSec");
        let pct = get_u64(r, "PercentDiskTime");

        let label = if name == "_Total" {
            "总计".to_string()
        } else {
            // "0 C:" → "C:"（去掉数字前缀）；非标准格式原样返回。
            match name.split_once(' ') {
                Some((_, drive)) => drive.to_string(),
                None => name.clone(),
            }
        };
        lines.push(format!(
            "- {label} · 读 {} · 写 {} · 磁盘占用 {}%",
            fmt_rate(rb),
            fmt_rate(wb),
            pct
        ));
    }

    if lines.is_empty() {
        return Ok("磁盘性能计数器无有效行（过滤规则可能过于严格）".into());
    }

    let mut s = format!("磁盘 IO 实时使用率：\n{}\n", lines.join("\n"));
    s.push_str(&format!(
        "说明：数据来源 Win32_PerfFormattedData_PerfDisk_PhysicalDisk（格式化计数器，当前速率）；\
         sample_ms={sample_ms} 仅作提示，未做真实双采样。"
    ));
    Ok(s)
}

/// 字节/秒 → 人类可读速率（`{v} MB/s` / `{v} KB/s` / `{n} B/s`）。
fn fmt_rate(bps: u64) -> String {
    if bps >= 1_024 * 1_024 {
        format!("{:.*} MB/s", 1, bps as f64 / 1024.0 / 1024.0)
    } else if bps >= 1024 {
        format!("{:.*} KB/s", 0, bps as f64 / 1024.0)
    } else {
        format!("{bps} B/s")
    }
}

#[cfg(not(windows))]
pub fn collect_disk_io_usage(_sample_ms: u64) -> Result<String, String> {
    Ok("当前平台不是 Windows，磁盘 IO 使用率不可用".into())
}

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
    PathGuard
        .check(p)
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
