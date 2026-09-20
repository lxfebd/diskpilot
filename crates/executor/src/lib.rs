//! Performs cleanup actions safely. Recycle (default), quarantine, or permanent delete.
//! Every action is appended to `undo.jsonl`.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Recycle,
    Quarantine,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub action: Action,
    pub paths: Vec<PathBuf>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoEntry {
    pub timestamp: String,
    pub action: Action,
    pub source: PathBuf,
    pub destination: Option<PathBuf>,
    pub reason: String,
    /// 实际释放的字节数（回收/隔离/删除后统计）；`None` 表示未能统计
    /// （如 dry-run 或统计失败）。前端据此展示真实「已清理 X」，不再一律标预估。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_freed: Option<u64>,
}

/// 把单个路径移入系统回收站。
///
/// 已知 Windows 误报（实测 Win11 新版 + trash 5.2.5）：`IFileOperation`
/// 明明把文件放进了回收站，`GetAnyOperationsAborted()` 却返回 TRUE，
/// trash crate 据此抛 "Some operations were aborted"。策略：
/// 1. 先走 trash::delete；
/// 2. 若报错但路径已消失 → 实际已回收成功，按成功处理（有 trash 实验佐证）；
/// 3. 若报错且路径还在 → 用旧版 SHFileOperationW（FOF_ALLOWUNDO）兜底，
///    完成后仍以「路径是否消失」为准判定成败。
pub fn recycle_path(p: &Path) -> anyhow::Result<()> {
    recycle_one(p)
}

fn recycle_one(p: &Path) -> anyhow::Result<()> {
    // 路径根本不存在 ≠ 回收成功：不能把「模型编造/已消失的路径」记成成功
    // undo（历史上这正是「建议可清、执行 0 B 且写成功日志」的根因之一）。
    if !p.exists() {
        anyhow::bail!("路径不存在，无法回收：{}", p.display());
    }
    match trash::delete(p) {
        Ok(()) => return Ok(()),
        Err(e) => {
            if !p.exists() {
                tracing::warn!(
                    "trash::delete 误报「{e}」，但路径已消失（实际已进回收站），按成功处理"
                );
                return Ok(());
            }
            tracing::warn!("trash::delete 失败（{e}）且路径仍在，改用 SHFileOperationW 兜底");
        }
    }
    shfileop_recycle(p)
}

/// 旧版 Shell API 回收站删除（trash 老版本同款路径），作为兜底。
#[cfg(windows)]
fn shfileop_recycle(p: &Path) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
        SHFILEOPSTRUCTW,
    };

    // pFrom 必须双 NUL 结尾
    let mut from: Vec<u16> = p.as_os_str().encode_wide().collect();
    from.push(0);
    from.push(0);

    let mut op: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    op.hwnd = std::ptr::null_mut();
    op.wFunc = FO_DELETE;
    op.pFrom = from.as_ptr();
    op.pTo = std::ptr::null();
    op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as u16;
    op.fAnyOperationsAborted = 0;
    op.hNameMappings = std::ptr::null_mut();
    op.lpszProgressTitle = std::ptr::null();

    let rc = unsafe { SHFileOperationW(&mut op) };
    if !p.exists() {
        return Ok(());
    }
    anyhow::bail!(
        "SHFileOperationW 回收失败：rc={rc}, aborted={}（路径 {}）",
        op.fAnyOperationsAborted,
        p.display()
    );
}

#[cfg(not(windows))]
fn shfileop_recycle(p: &Path) -> anyhow::Result<()> {
    anyhow::bail!("非 Windows 平台无 SHFileOperationW 兜底：{}", p.display())
}

pub fn execute(
    plan: &Plan,
    dry_run: bool,
    undo_log: &Path,
    quarantine_root: &Path,
) -> anyhow::Result<Vec<UndoEntry>> {
    // 任何动作（包括 recycle/quarantine）都拒绝盘根、系统保留目录与用户
    // 主目录根——这些不可能是合法的清理目标，误操作代价不可逆。
    for p in &plan.paths {
        if let Some(what) = protected_path(p) {
            anyhow::bail!("refusing to act on {what}: {}", p.display());
        }
    }

    let mut out: Vec<UndoEntry> = Vec::new();
    let now = || chrono::Utc::now().to_rfc3339();

    if dry_run {
        for p in &plan.paths {
            out.push(UndoEntry {
                timestamp: now(),
                action: plan.action,
                source: p.clone(),
                destination: None,
                reason: format!("dry-run: {}", plan.reason),
                bytes_freed: None,
            });
        }
        write_log(undo_log, &out)?;
        return Ok(out);
    }

    // 逐项执行、逐项写 undo 日志：某条失败（被占用/权限不足）时**跳过继续**，
    // 文件留在原地就是安全方向——scaffold disclaimer 承诺「被占用的文件会被
    // 自动跳过」，浏览器/应用开着时其余可清项照常进回收站，而不是整个 scope
    // 因一条锁定文件中断（此前会导致总览页「清理 0 B」）。
    match plan.action {
        Action::Recycle => {
            for p in &plan.paths {
                // 回收前统计真实字节：recycle_one 成功后路径已消失，晚算必为 0。
                let bytes = path_bytes_before(p);
                if let Err(e) = recycle_one(p) {
                    tracing::warn!(
                        "recycle 失败已跳过（可能被占用/无权限）：{} ({e})",
                        p.display()
                    );
                    continue;
                }
                let entry = UndoEntry {
                    timestamp: now(),
                    action: Action::Recycle,
                    source: p.clone(),
                    destination: None,
                    reason: plan.reason.clone(),
                    bytes_freed: bytes,
                };
                write_log(undo_log, std::slice::from_ref(&entry))?;
                out.push(entry);
            }
        }
        Action::Quarantine => {
            std::fs::create_dir_all(quarantine_root)?;
            let batch_stamp = chrono::Utc::now().timestamp_millis();
            for (i, src) in plan.paths.iter().enumerate() {
                let leaf = src
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "item".into());
                // 同一批次内毫秒时间戳会撞车（同名 leaf 各自成槽），加序号保证唯一。
                let dst = quarantine_root.join(format!("{batch_stamp}-{i}-{leaf}"));
                let bytes = path_bytes_before(src);
                if let Err(e) = std::fs::rename(src, &dst) {
                    tracing::warn!("rename failed ({}); falling back to copy+remove", e);
                    copy_then_remove(src, &dst)?;
                }
                let entry = UndoEntry {
                    timestamp: now(),
                    action: Action::Quarantine,
                    source: src.clone(),
                    destination: Some(dst),
                    reason: plan.reason.clone(),
                    bytes_freed: bytes,
                };
                write_log(undo_log, std::slice::from_ref(&entry))?;
                out.push(entry);
            }
        }
        Action::Delete => {
            for p in &plan.paths {
                // TOCTOU 复检：校验通过后、删除前目标可能被换成 symlink 指向
                // 受保护目录。此刻再 canonicalize 一次并重走 protected_path，
                // 把窗口缩小到「canonicalize 与删除之间」（本机用户主动操作场景
                // 威胁极低，但铁律是宁可错放也不误删——Delete 是唯一不可逆动作）。
                if let Some(what) = protected_path(p) {
                    anyhow::bail!(
                        "refusing to delete {what} (TOCTOU recheck): {}",
                        p.display()
                    );
                }
                let bytes = path_bytes_before(p);
                if p.is_dir() {
                    std::fs::remove_dir_all(p)?;
                } else if p.exists() {
                    std::fs::remove_file(p)?;
                }
                let entry = UndoEntry {
                    timestamp: now(),
                    action: Action::Delete,
                    source: p.clone(),
                    destination: None,
                    reason: plan.reason.clone(),
                    bytes_freed: bytes,
                };
                write_log(undo_log, std::slice::from_ref(&entry))?;
                out.push(entry);
            }
        }
    }

    Ok(out)
}

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

fn write_log(undo_log: &Path, entries: &[UndoEntry]) -> anyhow::Result<()> {
    if let Some(parent) = undo_log.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(undo_log)?;
    for e in entries {
        writeln!(f, "{}", serde_json::to_string(e)?)?;
    }
    Ok(())
}

/// Reads every undo entry recorded so far, newest first.
pub fn list_undo(undo_log: &Path) -> anyhow::Result<Vec<UndoEntry>> {
    let mut out: Vec<UndoEntry> = Vec::new();
    if !undo_log.exists() {
        return Ok(out);
    }
    for line in std::fs::read_to_string(undo_log)?.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(e) = serde_json::from_str::<UndoEntry>(line) {
            out.push(e);
        }
    }
    out.reverse();
    Ok(out)
}

/// Restores a single quarantined path back to its original location.
///
/// Safety: only guarantees the move back into `source`; callers are
/// responsible for showing a two-step confirmation before invoking this.
pub fn restore_quarantined(entry: &UndoEntry, quarantine_root: &Path) -> anyhow::Result<PathBuf> {
    let Some(dst) = &entry.destination else {
        anyhow::bail!("entry is not a quarantine record");
    };
    // The destination we recorded lives under quarantine_root; a relative/
    // absolute mismatch (e.g. log copied between machines) is refused.
    let dst_canonical = std::fs::canonicalize(dst).unwrap_or_else(|_| dst.clone());
    let root_canonical =
        std::fs::canonicalize(quarantine_root).unwrap_or_else(|_| quarantine_root.to_path_buf());
    if !dst_canonical.starts_with(&root_canonical) {
        anyhow::bail!(
            "refusing to restore outside quarantine root: {}",
            dst_canonical.display()
        );
    }
    if !dst_canonical.exists() {
        anyhow::bail!("quarantined item no longer exists: {}", dst.display());
    }
    let src = &entry.source;
    if src.exists() {
        anyhow::bail!("original path already exists: {}", src.display());
    }
    if let Some(parent) = src.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Err(e) = std::fs::rename(&dst_canonical, src) {
        tracing::warn!("restore rename failed ({}); falling back to copy+remove", e);
        copy_then_remove(&dst_canonical, src)?;
    }
    Ok(src.clone())
}

/// Removes fully-restored entries from the log so it only tracks pending
/// undo actions.
///
/// 写入必须是原子的：先写同目录临时文件再 rename 覆盖，避免中途崩溃/断电
/// 把 `undo.jsonl` 截断成半截（旧实现 `File::create` 直接截断重写，崩溃即
/// 丢全部历史，违背「一切操作可追溯可撤销」）。rename 同卷内是原子操作。
pub fn remove_restored(undo_log: &Path, restored_sources: &[PathBuf]) -> anyhow::Result<()> {
    if restored_sources.is_empty() {
        return Ok(());
    }
    let remaining: Vec<UndoEntry> = list_undo(undo_log)?
        .into_iter()
        .filter(|e| {
            !restored_sources.iter().any(|src| {
                e.action == Action::Quarantine && &e.source == src && e.destination.is_some()
            })
        })
        .collect();
    let parent = undo_log.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).ok();
    let tmp = parent.join(format!(
        ".undo.{}.{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        for e in remaining.iter().rev() {
            writeln!(f, "{}", serde_json::to_string(e)?)?;
        }
        f.flush()?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, undo_log)?;
    Ok(())
}

fn copy_then_remove(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        copy_dir_recursive(src, dst)?;
        std::fs::remove_dir_all(src)?;
    } else {
        std::fs::copy(src, dst)?;
        std::fs::remove_file(src)?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let p = entry.path();
        let d = dst.join(entry.file_name());
        if p.is_dir() {
            copy_dir_recursive(&p, &d)?;
        } else {
            std::fs::copy(&p, &d)?;
        }
    }
    Ok(())
}

/// 统计路径占用的真实字节（删除/回收前调用）：文件取 len，目录递归求和。
/// 统计失败（无权限/IO 错误）返回 `None`，调用方存 `bytes_freed: None`
/// （前端降级为预估展示，不影响执行）。
fn path_bytes_before(p: &Path) -> Option<u64> {
    if p.is_file() {
        return p.metadata().ok().map(|m| m.len());
    }
    if p.is_dir() {
        fn dir_sum(dir: &Path) -> Option<u64> {
            let mut sum = 0u64;
            for entry in std::fs::read_dir(dir).ok()?.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    sum = sum.saturating_add(dir_sum(&p)?);
                } else if p.is_file() {
                    sum = sum.saturating_add(p.metadata().ok()?.len());
                }
            }
            Some(sum)
        }
        return dir_sum(p);
    }
    None
}
