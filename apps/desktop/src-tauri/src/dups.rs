//! 重复文件建议（R2）：结构化扫描 + 保留哪份启发式 + 运行中文件跳过。
//!
//! 与 agent-server `diskx::collect_duplicate_files` 的关系：那是 AI 对话侧
//! 只读**文本**工具（头部 64KB 哈希）；这里是桌面侧**结构化**建议，直接喂给
//! 前端确认清单（CleanupProposalDialog 风格），清理走既有 `execute_ai_plan`
//! 确认门（user_confirmed + cleanup.execute + 强制 Recycle + protected_path）。
//!
//! 安全铁律（对齐 diskx.rs / executor.rs）：
//! - 本模块**只读**：只打开文件做头部哈希 / 独占探测，绝不写、绝不删；
//! - 路径守卫：拒绝盘根 / 系统目录 / 用户主目录根（复用 executor 同款语义，
//!   这里本地实现 PathGuard 的拷贝，避免跨 crate 依赖）；
//! - 运行中文件（独占打开失败）标记 `is_running` 并从可删候选剔除——清理
//!   正在写的文件即使进程持有也会失败，白白制造失败任务；
//! - 头部哈希只读 64KB，不做全文件哈希（与 diskx 一致，注明局限）；
//! - 文件数硬上限，防内存爆炸。
//!
//! ## 「保留哪份」启发式
//! 每组重复文件推荐保留一份：
//! 1. **path 最短**保留（越深越可能是某进程随手复制的副本；根附近的越可能是
//!    原件/用户手放的重要文件）；
//! 2. 同长时 **mtime 最旧**保留（历史原件，新复制件可删）；
//! 3. 仍并列（路径短且 mtime 都相同）→ 任意取第一个（字典序）——纯展示建议，
//!    用户确认窗可改选保留哪份、勾选删哪些。

use std::collections::BTreeMap;
use std::hash::Hasher;
use std::io::Read;
use std::path::{Path, PathBuf};

use diskpilot_scanner::diskpilot_walker;

// 递归遍历直接用 scanner 的 diskpilot_walker（同款语义：符号链接不跟随防环、
// 深度受限），在调用处内联 `diskpilot_walker(root).into_iter().flatten()`，
// 与 cleanup.rs / executor.rs 一致，避免在 desktop 引入 jwalk 直接依赖。

/// 扫描硬上限：单次遍历最多看这么多文件（超过就 break 并标 truncated）。
const MAX_FILES_SCAN: usize = 200_000;
/// 头部哈希读取字节数：64 KB（与 diskx.rs 一致）。
const HEAD_HASH_BYTES: usize = 64 * 1024;

/// 单个重复候选文件。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DupFile {
    pub path: String,
    /// 文件 mtime（UNIX 秒）。读不到（MFT 等）为 None。
    pub mtime: Option<u64>,
    /// 独占打开失败（被进程占用 / 无权限）→ 判定为「运行中」，不进可删候选。
    pub is_running: bool,
}

/// 一组重复文件（size 分组 + 头部哈希聚类后 ≥2 个的文件）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DupGroup {
    /// 组内文件大小（字节，>0）。
    pub size: u64,
    /// 组内全部文件（含运行中文件——展示给用户看，但回收候选会剔除）。
    pub files: Vec<DupFile>,
    /// 建议保留的文件 path（启发式），其余为可回收候选。
    pub keep: String,
    /// 建议回收的文件 path（= files 中非 keep 且非运行中）。
    pub recycle_candidates: Vec<String>,
    /// 组内运行中文件 path（展示「跳过原因」）。
    pub running: Vec<String>,
}

impl DupGroup {
    /// 计算 keep / recycle_candidates / running。`keep` 选中后从文件里排除，
    /// `running` 不计入候选。
    fn finalize(mut self) -> Self {
        // 启发式选 keep：path 短优先（越浅越像原件），同长比 mtime 旧。
        self.files.sort_by(|a, b| {
            a.path
                .len()
                .cmp(&b.path.len())
                .then_with(|| a.mtime.unwrap_or(0).cmp(&b.mtime.unwrap_or(0)))
                .then_with(|| a.path.cmp(&b.path))
        });
        let keep = self.files[0].clone();
        self.keep = keep.path.clone();
        self.running = self
            .files
            .iter()
            .filter(|f| f.is_running)
            .map(|f| f.path.clone())
            .collect();
        self.recycle_candidates = self
            .files
            .iter()
            .skip(1)
            .filter(|f| !f.is_running)
            .map(|f| f.path.clone())
            .collect();
        self
    }
}

/// 字节数人类可读（rustc 1.98 安全写法 `{:.*}`）。
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

/// 运行中文件探测：用「带写共享的读打开」试探。Windows 上若别的进程持有
/// **拒绝写共享**（`FILE_SHARE_WRITE` 未开）的句柄，我们的 `CreateFileW` 以
/// GENERIC_READ + 请求 FILE_SHARE_WRITE 打开就会失败（ERROR_SHARING_VIOLATION）
/// —— 恰好暴露「有进程正在写它」。只读备份语义（BACKUP_SEMANTICS）不需要，
/// 文件就是普通文件。普通只读文件共享打开必然成功。
#[cfg(windows)]
fn is_file_busy(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    let opened = handle != INVALID_HANDLE_VALUE;
    if opened {
        unsafe {
            let _ = CloseHandle(handle);
        }
    }
    !opened
}

/// 非 Windows 无「拒绝写共享」语义，统一视为非占用（只读展示字段，不参与判定）。
#[cfg(not(windows))]
fn is_file_busy(_path: &Path) -> bool {
    false
}

/// 读文件头部 [`HEAD_HASH_BYTES`] 字节并计算 `DefaultHasher` 哈希。
/// 打不开文件（被占用 / 权限 / 已被删）返回 `None`（不计入判定）。
fn read_head_hash(path: &Path) -> Option<u64> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; HEAD_HASH_BYTES];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(&buf);
    Some(hasher.finish())
}

/// 路径守卫：直接复用 `diskpilot_executor::protected_path`——**与执行侧同一把锁**。
/// 提案建议的路径集合必须 ⊆ 执行敢删的集合，否则会出现「建议能删、执行却拒绝」
/// 的脱节。返回 None = 通过。
fn guarded_path(p: &Path) -> Option<String> {
    diskpilot_executor::protected_path(p).map(|what| format!("受保护路径：{what}"))
}

/// 结构化重复文件扫描（只读）。
///
/// 按 size 分组（≥2 才候选）→ 组内做头部 64KB 哈希聚类（≥2 才输出）。
/// 每组计算 keep / recycle_candidates / running。文件数硬上限 [`MAX_FILES_SCAN`]。
pub fn scan_duplicate_files(root: &Path, min_size: u64) -> Result<Vec<DupGroup>, String> {
    if let Some(reason) = guarded_path(root) {
        return Err(format!("路径被安全守卫拒绝：{reason}"));
    }
    if !root.is_dir() {
        return Err(format!("目录不存在：{}", root.display()));
    }

    let mut by_size: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
    let mut files_seen = 0usize;
    let mut truncated = false;

    for entry in diskpilot_walker(root).into_iter().flatten() {
        if files_seen >= MAX_FILES_SCAN {
            truncated = true;
            break;
        }
        if entry.path_is_symlink() {
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            files_seen += 1;
            continue;
        };
        let len = meta.len();
        if len == 0 || len < min_size {
            files_seen += 1;
            continue;
        }
        files_seen += 1;
        by_size
            .entry(len)
            .or_default()
            .push(entry.path().to_path_buf());
    }

    let mut groups: Vec<DupGroup> = Vec::new();
    for (size, members) in by_size {
        if members.len() < 2 {
            continue;
        }
        // 组内做头部 64KB 哈希：同 hash 聚成一组；读不到（占用/权限/已删）不计。
        let mut hash_map: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
        for m in &members {
            let Some(h) = read_head_hash(m) else { continue };
            hash_map.entry(h).or_default().push(m.clone());
        }
        for paths in hash_map.values() {
            if paths.len() < 2 {
                continue;
            }
            let mut files: Vec<DupFile> = Vec::with_capacity(paths.len());
            for p in paths {
                let mtime = std::fs::metadata(p)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
                files.push(DupFile {
                    path: p.display().to_string(),
                    mtime,
                    is_running: is_file_busy(p),
                });
            }
            groups.push(
                DupGroup {
                    size,
                    files,
                    keep: String::new(),
                    recycle_candidates: Vec::new(),
                    running: Vec::new(),
                }
                .finalize(),
            );
        }
    }

    // 输出完成度提示（截断 / 只比对头部哈希）。
    if groups.is_empty() {
        let warn = if truncated {
            "；⚠ 已达遍历上限，结果不完整。"
        } else {
            "。"
        };
        return Err(format!(
            "未发现重复文件（按大小+头部哈希，共扫描 {files_seen} 个文件）{warn}说明：只比对文件头部 {}，未做全文件哈希。",
            fmt_bytes(HEAD_HASH_BYTES as u64)
        ));
    }
    Ok(groups)
}

/// Tauri 命令：结构化重复文件建议（只读 L0）。
#[tauri::command]
pub(crate) fn scan_duplicate_files_cmd(
    root_path: String,
    min_size: Option<u64>,
) -> Result<Vec<DupGroup>, String> {
    let root = PathBuf::from(&root_path);
    scan_duplicate_files(&root, min_size.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tmp_sub(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("dp-dups-test-{}-{}", std::process::id(), seq));
        let d = base.join(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(p: &Path, bytes: &[u8]) {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, bytes).unwrap();
    }

    #[test]
    fn clusters_by_hash_within_size_group() {
        let root = tmp_sub("hash");
        // 三份 200B 头部全同 → 一组；两份 200B 另一头部（CD）→ 另一组；没有杂物。
        write(&root.join("a.bin"), &vec![0xAB; 200]);
        write(&root.join("b.bin"), &vec![0xAB; 200]);
        write(&root.join("c.bin"), &vec![0xAB; 200]);
        write(&root.join("d.bin"), &vec![0xCD; 200]);
        write(&root.join("e.bin"), &vec![0xCD; 200]);

        let groups = scan_duplicate_files(&root, 0).unwrap();
        assert_eq!(groups.len(), 2, "两个头部各成一组");
        // 找 AB 组（a/b/c 同 0xAB）
        let ab: Vec<&DupGroup> = groups
            .iter()
            .filter(|g| g.files.iter().any(|f| f.path.ends_with("a.bin")))
            .collect();
        assert_eq!(ab.len(), 1);
        assert_eq!(ab[0].files.len(), 3, "AB 全同哈希 3 份成组");
        assert_eq!(ab[0].recycle_candidates.len(), 2, "保留 1 份，2 份可回收");
        assert!(
            ab[0].recycle_candidates.iter().all(|s| s != &ab[0].keep),
            "回收候选不含保留份"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn keep_shortest_path_then_oldest() {
        let root = tmp_sub("keep");
        // 深层副本（更短路径的"原件"保留；mtime 无法控，靠路径长度）
        write(&root.join("orig.bin"), &vec![0x11; 100]);
        write(&root.join("sub/dup.bin"), &vec![0x11; 100]);
        write(&root.join("sub/deep/dup2.bin"), &vec![0x11; 100]);

        let groups = scan_duplicate_files(&root, 0).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].keep,
            root.join("orig.bin").display().to_string(),
            "路径最短保留"
        );
        assert_eq!(groups[0].recycle_candidates.len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn guards_root_and_missing_dir() {
        // 盘根拒绝
        let root = if cfg!(windows) {
            PathBuf::from("C:\\")
        } else {
            PathBuf::from("/")
        };
        let err = scan_duplicate_files(&root, 0).unwrap_err();
        assert!(err.contains("安全守卫"), "盘根被拒: {err}");
        // 不存在目录拒绝
        let missing = std::env::temp_dir().join("dp-dups-nonexistent-zzz");
        let err = scan_duplicate_files(&missing, 0).unwrap_err();
        assert!(err.contains("不存在"), "目录不存在被拒: {err}");
    }

    #[test]
    fn min_size_filters_tiny_files() {
        let root = tmp_sub("minsize");
        write(&root.join("a.bin"), &vec![0x01; 50]);
        write(&root.join("b.bin"), &vec![0x01; 50]);
        // min_size=100 → 都不进候选
        let groups = scan_duplicate_files(&root, 100).unwrap_err();
        assert!(
            groups.contains("未发现重复文件"),
            "min_size 过滤后无组: {groups}"
        );
        // min_size=0 → 50B 成组
        let groups = scan_duplicate_files(&root, 0).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].size, 50);
        let _ = std::fs::remove_dir_all(&root);
    }
}
