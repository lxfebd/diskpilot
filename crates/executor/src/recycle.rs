//! 回收站删除域：recycle_path / recycle_one / shfileop_recycle。
//! 从 lib.rs 拆分（2026-10-12）。先走 trash::delete，误报或失败时用
//! SHFileOperationW 兜底，成败一律以「路径是否消失」为准。

use std::path::Path;

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

pub(crate) fn recycle_one(p: &Path) -> anyhow::Result<()> {
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
