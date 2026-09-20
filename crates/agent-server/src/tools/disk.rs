//! 磁盘健康/用量/文件类型统计（纯逻辑，Windows 原生 API；非 Windows 降级）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DiskInfo {
    pub drive: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub used_percent: f64,
}

/// 枚举所有存在盘符，返回各自用量（GetLogicalDrives + GetDiskFreeSpaceExW）。
#[cfg(windows)]
pub fn collect_disks() -> Vec<DiskInfo> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetLogicalDrives};

    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let wide: Vec<u16> = OsStr::new(&root)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut total = 0u64;
        let mut free = 0u64;
        let ok = unsafe {
            GetDiskFreeSpaceExW(wide.as_ptr(), std::ptr::null_mut(), &mut total, &mut free)
        };
        if ok == 0 {
            continue;
        }
        let used = total.saturating_sub(free);
        let percent = if total > 0 {
            (used as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        out.push(DiskInfo {
            drive: format!("{letter}:"),
            total_bytes: total,
            used_bytes: used,
            free_bytes: free,
            used_percent: percent,
        });
    }
    out
}

#[cfg(not(windows))]
pub fn collect_disks() -> Vec<DiskInfo> {
    Vec::new()
}

/// 某路径所在卷的用量（GetDiskFreeSpaceExW 对任意路径生效）。
#[cfg(windows)]
pub fn collect_volume_usage(path: &std::path::Path) -> Option<DiskInfo> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut total = 0u64;
    let mut free = 0u64;
    let ok =
        unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), std::ptr::null_mut(), &mut total, &mut free) };
    if ok == 0 {
        return None;
    }
    let used = total.saturating_sub(free);
    let percent = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    Some(DiskInfo {
        drive: path.display().to_string(),
        total_bytes: total,
        used_bytes: used,
        free_bytes: free,
        used_percent: percent,
    })
}

#[cfg(not(windows))]
pub fn collect_volume_usage(_path: &std::path::Path) -> Option<DiskInfo> {
    None
}

#[derive(Debug, Clone, Serialize)]
pub struct TypeStat {
    pub ext: String,
    pub count: u64,
    pub bytes: u64,
}

const MAX_WALK_FILES: usize = 200_000;

#[derive(Default)]
struct WalkState {
    count: usize,
    map: std::collections::HashMap<String, (u64, u64)>,
    total_bytes: u64,
    truncated: bool,
}

/// 递归统计目录下按扩展名聚合的文件数与占用，按占用降序。
/// 带硬上限（20 万文件 / 20 层深度），超限标记 truncated。
/// jwalk 并行遍历，多核机器上比串行快数倍。
pub fn file_type_stats(root: &std::path::Path, top_n: usize) -> (Vec<TypeStat>, u64, bool) {
    let mut state = WalkState::default();
    let mut seen = 0usize;
    for entry in parallel_walker(root) {
        let Ok(entry) = entry else { continue };
        seen += 1;
        if state.count >= MAX_WALK_FILES || seen > MAX_WALK_FILES + 100_000 {
            state.truncated = true;
            break;
        }
        // 跳符号链接防环
        if entry.path_is_symlink() {
            continue;
        }
        let ft = entry.file_type();
        if ft.is_file() {
            state.count += 1;
            let ext = entry
                .file_name()
                .to_string_lossy()
                .rsplit('.')
                .next()
                .map(|x| x.to_ascii_lowercase())
                .unwrap_or_else(|| "(无)".into());
            let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let ent = state.map.entry(ext).or_insert((0, 0));
            ent.0 += 1;
            ent.1 += len;
            state.total_bytes += len;
        }
    }
    let mut list: Vec<TypeStat> = state
        .map
        .into_iter()
        .map(|(ext, (count, bytes))| TypeStat { ext, count, bytes })
        .collect();
    list.sort_by_key(|t| std::cmp::Reverse(t.bytes));
    list.truncate(top_n.max(1));
    (list, state.total_bytes, state.truncated)
}

/// 递归遍历公共 walker（与 files.rs 同款语义）。
fn parallel_walker(root: &std::path::Path) -> jwalk::WalkDir {
    jwalk::WalkDir::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .max_depth(20)
}

/// 单个分区的类型元数据（卷类型 / 文件系统 / 卷标）。
#[derive(Debug, Clone, Serialize)]
pub struct VolumeMeta {
    pub drive: String,
    pub drive_type: String,
    pub fs: String,
    pub label: String,
}

#[cfg(windows)]
fn drive_type_name(t: u32) -> String {
    // DRIVE_* 常量在 windows-sys 0.59 未导出，按 Win32 枚举值内联：
    // 0=unknown 1=no root 2=removable 3=fixed 4=remote 5=cdrom 6=ramdisk
    match t {
        2 => "可移动磁盘".into(),
        3 => "本地磁盘".into(),
        4 => "网络驱动器".into(),
        5 => "光盘驱动器".into(),
        6 => "内存磁盘".into(),
        _ => "未知".into(),
    }
}

#[cfg(windows)]
fn trim_wide(s: &[u16]) -> &[u16] {
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    &s[..end]
}

/// 每个分区的卷类型元数据（只读无副作用）。跨盘符（含装在 D 盘的系统）均可识别。
#[cfg(windows)]
pub fn collect_volume_meta() -> Vec<VolumeMeta> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };

    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let wide: Vec<u16> = OsStr::new(&root)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let dt = unsafe { GetDriveTypeW(wide.as_ptr()) };
        if dt == 0 {
            continue;
        }
        let mut vol_name = [0u16; 64];
        let mut fs_name = [0u16; 64];
        let mut serial = 0u32;
        let mut max_comp = 0u32;
        let mut flags = 0u32;
        let ok = unsafe {
            GetVolumeInformationW(
                wide.as_ptr(),
                vol_name.as_mut_ptr(),
                vol_name.len() as u32,
                &mut serial,
                &mut max_comp,
                &mut flags,
                fs_name.as_mut_ptr(),
                fs_name.len() as u32,
            )
        };
        let (label, fs) = if ok != 0 {
            (
                String::from_utf16_lossy(trim_wide(&vol_name)),
                String::from_utf16_lossy(trim_wide(&fs_name)),
            )
        } else {
            (String::new(), String::new())
        };
        out.push(VolumeMeta {
            drive: format!("{letter}:"),
            drive_type: drive_type_name(dt),
            fs,
            label,
        });
    }
    out
}

#[cfg(not(windows))]
pub fn collect_volume_meta() -> Vec<VolumeMeta> {
    Vec::new()
}
