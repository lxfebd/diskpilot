//! executor 私有工具函数（从 lib.rs 拆分，2026-10-12）。
//! write_log（undo 台账追加）/ copy_then_remove（跨卷兜底复制删除）/
//! copy_dir_recursive / path_bytes_before（动作前真实字节统计）。

use std::io::Write;
use std::path::Path;

use super::UndoEntry;
pub(crate) fn write_log(undo_log: &Path, entries: &[UndoEntry]) -> anyhow::Result<()> {
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

pub(crate) fn copy_then_remove(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        copy_dir_recursive(src, dst)?;
        std::fs::remove_dir_all(src)?;
    } else {
        std::fs::copy(src, dst)?;
        std::fs::remove_file(src)?;
    }
    Ok(())
}

pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
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
pub(crate) fn path_bytes_before(p: &Path) -> Option<u64> {
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
