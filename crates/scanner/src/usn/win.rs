//! USN Journal 的 Windows DeviceIoControl 层：卷设备打开 + FSCTL 查询/
//! 枚举/回放（query_journal / build_frn_table / read_changes）。
//! 从 usn.rs 拆分（2026-10-12），保持 crate::usn::win:: 旧调用路径。
//! 仅在 windows target 编译（上层模块 #[cfg(windows)] pub mod win;）。
use super::{JournalState, UsnChange, UsnCursor};
use anyhow::anyhow;
use std::collections::HashMap;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{
    ERROR_HANDLE_EOF, ERROR_JOURNAL_NOT_ACTIVE, GENERIC_READ, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::Ioctl::{
    FSCTL_ENUM_USN_DATA, FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_USN_JOURNAL, MFT_ENUM_DATA_V0,
    READ_USN_JOURNAL_DATA_V0, USN_JOURNAL_DATA_V0,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

const OPEN_EXISTING: u32 = 3;

fn device_path(volume_letter: char) -> String {
    format!(r"\\.\{}:", volume_letter.to_ascii_uppercase())
}

/// Open the volume with backup semantics so directory enumeration works
/// without admin rights (raw MFT reads need admin, journal reads don't).
fn open_volume(volume_letter: char) -> anyhow::Result<std::fs::File> {
    let path = device_path(volume_letter);
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(anyhow!(
            "CreateFileW({path}) failed (last_error={})",
            unsafe { windows_sys::Win32::Foundation::GetLastError() }
        ));
    }
    // Wrap in std::fs::File via FromRawHandle so it closes on drop.
    Ok(unsafe { std::fs::File::from_raw_handle(handle) })
}

/// FSCTL_QUERY_USN_JOURNAL — the volume's journal identity + high-water
/// marks. Fails on non-NTFS volumes (no change journal).
pub fn query_journal(volume_letter: char) -> anyhow::Result<JournalState> {
    let file = open_volume(volume_letter)?;
    let handle = file.as_raw_handle();
    let mut data: USN_JOURNAL_DATA_V0 = unsafe { std::mem::zeroed() };
    let mut bytes = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_QUERY_USN_JOURNAL,
            std::ptr::null(),
            0,
            &mut data as *mut _ as *mut _,
            std::mem::size_of::<USN_JOURNAL_DATA_V0>() as u32,
            &mut bytes,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        let e = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if e == ERROR_JOURNAL_NOT_ACTIVE {
            return Err(anyhow!(
                "USN journal is not active on {}",
                device_path(volume_letter)
            ));
        }
        return Err(anyhow!("FSCTL_QUERY_USN_JOURNAL failed (last_error={e})"));
    }
    Ok(JournalState {
        journal_id: data.UsnJournalID,
        first_usn: data.FirstUsn,
        next_usn: data.NextUsn,
        lowest_valid_usn: data.LowestValidUsn,
    })
}

/// FSCTL_READ_USN_JOURNAL — replay journal records since `cursor`.
/// Returns changes plus the new high-water mark to persist.
pub fn read_changes(
    volume_letter: char,
    cursor: &UsnCursor,
) -> anyhow::Result<(Vec<UsnChange>, i64)> {
    let file = open_volume(volume_letter)?;
    let handle = file.as_raw_handle();
    let mut read = READ_USN_JOURNAL_DATA_V0 {
        StartUsn: cursor.next_usn,
        ReasonMask: 0, // all reasons
        ReturnOnlyOnClose: 0,
        Timeout: 0,
        BytesToWaitFor: 0,
        UsnJournalID: cursor.journal_id,
    };
    let mut buf = vec![0u8; 1 << 20]; // 1 MiB batches
    let mut changes = Vec::new();
    let mut next_usn = cursor.next_usn;
    let mut iterations = 0u32;
    loop {
        let mut bytes = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                FSCTL_READ_USN_JOURNAL,
                &mut read as *mut _ as *mut _,
                std::mem::size_of::<READ_USN_JOURNAL_DATA_V0>() as u32,
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                &mut bytes,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            let e = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            if e == ERROR_HANDLE_EOF {
                break; // caught up
            }
            return Err(anyhow!("FSCTL_READ_USN_JOURNAL failed (last_error={e})"));
        }
        let recs = super::parse_usn_records(&buf[..bytes as usize]);
        for r in &recs {
            if r.usn > next_usn {
                next_usn = r.usn;
            }
        }
        changes.extend(recs);
        iterations += 1;
        if iterations > 10_000 {
            return Err(anyhow!("USN journal replay exceeded 10k batches"));
        }
        if bytes == 0 {
            // No records but not EOF — bump the cursor to avoid spinning.
            next_usn = next_usn.saturating_add(1);
        }
        read.StartUsn = next_usn;
    }
    Ok((changes, next_usn))
}

/// FSCTL_ENUM_USN_DATA — paginated full-volume enumeration. Returns an
/// FRN → (parent FRN, name) table for path resolution. `on_progress`
/// receives (records_seen, 0).
pub fn build_frn_table<F>(
    volume_letter: char,
    cancel: Option<&AtomicBool>,
    mut on_progress: F,
) -> anyhow::Result<HashMap<u64, (u64, String)>>
where
    F: FnMut(u64, u64),
{
    let file = open_volume(volume_letter)?;
    let handle = file.as_raw_handle();
    let mut enum_data = MFT_ENUM_DATA_V0 {
        StartFileReferenceNumber: 0,
        LowUsn: 0,
        HighUsn: i64::MAX,
    };
    let mut buf = vec![0u8; 1 << 20];
    let mut map = HashMap::new();
    let mut seen = 0u64;
    let mut last_frn = 0u64;
    let mut guard = 0u32;
    loop {
        let mut bytes = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                FSCTL_ENUM_USN_DATA,
                &mut enum_data as *mut _ as *mut _,
                std::mem::size_of::<MFT_ENUM_DATA_V0>() as u32,
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                &mut bytes,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            let e = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            if e == ERROR_HANDLE_EOF {
                break; // enumerated everything
            }
            return Err(anyhow!("FSCTL_ENUM_USN_DATA failed (last_error={e})"));
        }
        let recs = super::parse_usn_records(&buf[..bytes as usize]);
        for r in &recs {
            map.insert(r.frn, (r.parent_frn, r.name.clone()));
            seen += 1;
            last_frn = r.frn;
        }
        on_progress(seen, 0);
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(anyhow!("scan:cancelled: usn frn table"));
        }
        if recs.is_empty() {
            return Err(anyhow!("FSCTL_ENUM_USN_DATA stalled (no records in batch)"));
        }
        enum_data.StartFileReferenceNumber = last_frn + 1;
        guard += 1;
        if guard > 1_000_000 {
            return Err(anyhow!("FSCTL_ENUM_USN_DATA exceeded iteration guard"));
        }
    }
    Ok(map)
}
