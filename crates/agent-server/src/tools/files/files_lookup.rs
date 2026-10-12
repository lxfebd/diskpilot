//! 只读查找域（从 files.rs 拆分，2026-10-12）。
//! DirEntry/list_dir/read_text_file/FindHit/find_files + 上限常量。

use std::path::Path;

use serde::Serialize;

use crate::tools::parallel_walker;

#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: Option<u64>,
}

/// 列出目录直接子项（非递归）。按目录在前、名称排序。
/// 条目收集有 10 万硬上限：超大目录（缓存/解压目录可能有数十万条目）全量
/// 读进内存再排序会瞬间占用数百 MB（2026-10-11 修复）；触顶即截断并标记
/// truncated，排序在已收集子集内进行（截断态本就只承诺前 limit 条）。
pub(crate) const MAX_DIR_ENTRIES: usize = 100_000;
pub fn list_dir(p: &std::path::Path, limit: usize) -> Result<(Vec<DirEntry>, bool), String> {
    let rd = std::fs::read_dir(p).map_err(|e| format!("读取目录失败：{e}"))?;
    let mut items = Vec::new();
    let mut hard_capped = false;
    for e in rd.flatten() {
        if items.len() >= MAX_DIR_ENTRIES {
            hard_capped = true;
            break;
        }
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
    let truncated = hard_capped || items.len() > limit;
    items.truncate(limit);
    Ok((items, truncated))
}

pub(crate) const MAX_READ_BYTES: usize = 256 * 1024;

/// 读文本文件（UTF-8/UTF-16 视 BOM 自动转码；非文本内容按替换字符处理），
/// 超过 256KB 截断并标记。仅限文件，拒绝目录。
pub fn read_text_file(p: &std::path::Path) -> Result<(String, bool), String> {
    let meta = std::fs::metadata(p).map_err(|e| format!("读取元数据失败：{e}"))?;
    if meta.is_dir() {
        return Err("目标是目录，请传文件路径".into());
    }
    // 只读前 256KB+1 字节：原先 std::fs::read 整读再 truncate，读 GB 级文件会
    // 先吃掉等量内存才截断（2026-10-11 修复：截断必须发生在读取时，不是读完后）。
    // 多读 1 字节用于判定 truncated；UTF-16 分支截断后长度为偶数，不受影响。
    let mut bytes = Vec::with_capacity(MAX_READ_BYTES + 1);
    let f = std::fs::File::open(p).map_err(|e| format!("打开文件失败：{e}"))?;
    use std::io::Read;
    f.take(MAX_READ_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("读取文件失败：{e}"))?;
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
    progress: Option<&dyn Fn(usize, usize)>,
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
        // 进度回调：每 4096 项报一次（无命中时最坏要遍历到 MAX_SEEN，
        // 前端进度条据此显示真实推进，而不是干等 45s 超时）。
        if seen.is_multiple_of(4096) {
            if let Some(cb) = progress {
                cb(seen, MAX_SEEN);
            }
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
