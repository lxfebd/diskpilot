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

mod recycle;
pub(crate) use recycle::recycle_one;
pub use recycle::recycle_path;

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
            // dry-run 也实测每项字节（与真删后的 path_bytes_before 同一口径），
            // 让确认面板「预估」与实际可清量对齐，不再恒 None。
            out.push(UndoEntry {
                timestamp: now(),
                action: plan.action,
                source: p.clone(),
                destination: None,
                reason: format!("dry-run: {}", plan.reason),
                bytes_freed: path_bytes_before(p),
            });
        }
        // 只读探测**不落盘**：dry-run 是「预览/确认窗」语义，写 undo.jsonl 会
        // 在「最近清理」台账里留下无法撤销的幽灵条目（destination=None），
        // 且反复预览会让台账无限膨胀。真正执行时每项才单独 write_log。
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

mod protect;
pub use protect::protected_path;

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

mod util;
use util::{copy_then_remove, path_bytes_before, write_log};
