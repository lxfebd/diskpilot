//! 磁盘枚举与体积估算命令（list_drives / volume_info / estimate_size）。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：盘符枚举、单盘空间查询（Win32
//! GetDiskFreeSpaceExW）、进度分母用的快速体积估算。无 AppState 依赖。

#[derive(serde::Serialize, Clone)]
pub(crate) struct VolumeInfo {
    /// 盘符，如 "C:\"。仅 list_drives 填充；volume_info 单查时不带（历史兼容）。
    #[serde(default)]
    pub(crate) path: String,
    pub(crate) total_bytes: u64,
    pub(crate) used_bytes: u64,
    pub(crate) free_bytes: u64,
}

/// 枚举本机所有可用磁盘，返回盘符与空间信息。供"全盘扫描"首页直出，
/// 长辈不必再手动选文件夹。
#[tauri::command]
pub(crate) fn list_drives() -> Vec<VolumeInfo> {
    let mut out = Vec::new();
    for letter in b'C'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if let Ok(mut info) = volume_info(root.clone()) {
            info.path = root;
            // used_bytes == 0 且经 GetDiskFreeSpaceExW 成功 -> 普通可读盘
            out.push(info);
        }
    }
    out
}

#[tauri::command]
pub(crate) fn volume_info(path: String) -> Result<VolumeInfo, String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut wide: Vec<u16> = std::ffi::OsStr::new(&path).encode_wide().collect();
        // Trim to drive root for the Win32 API.
        let root: Vec<u16> = if wide.len() >= 2 && wide[1] == b':' as u16 {
            vec![wide[0], wide[1], b'\\' as u16, 0]
        } else {
            wide.push(0);
            wide
        };
        let mut free_to_caller: u64 = 0;
        let mut total: u64 = 0;
        let mut total_free: u64 = 0;
        // SAFETY: calling documented Win32 API with valid wide-char buffers.
        let ok = unsafe {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetDiskFreeSpaceExW(
                    lpDirectoryName: *const u16,
                    lpFreeBytesAvailable: *mut u64,
                    lpTotalNumberOfBytes: *mut u64,
                    lpTotalNumberOfFreeBytes: *mut u64,
                ) -> i32;
            }
            GetDiskFreeSpaceExW(
                root.as_ptr(),
                &mut free_to_caller,
                &mut total,
                &mut total_free,
            )
        };
        if ok == 0 {
            return Err("GetDiskFreeSpaceExW failed".into());
        }
        Ok(VolumeInfo {
            path,
            total_bytes: total,
            used_bytes: total.saturating_sub(total_free),
            free_bytes: total_free,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("volume_info only implemented on Windows".into())
    }
}

/// Fast size-only walk used to seed the progress-bar denominator before the
/// real scan starts. Single jwalk pass, sums file sizes, no other state.
#[tauri::command]
pub(crate) async fn estimate_size(path: String) -> Result<u64, String> {
    let p = std::path::PathBuf::from(&path);
    tokio::task::spawn_blocking(move || -> u64 {
        let mut total: u64 = 0;
        for entry in diskpilot_scanner::diskpilot_walker(&p)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_file() {
                if let Ok(md) = entry.metadata() {
                    total = total.saturating_add(md.len());
                }
            }
        }
        total
    })
    .await
    .map_err(|e| e.to_string())
}
