//! USN Journal incremental scan (Windows).
//!
//! The Change Journal (USN Journal) is an NTFS on-disk log of every file
//! system change (create / delete / rename / data change / ...). A full scan
//! touches every file; an incremental scan replays the journal from the last
//! cursor and only re-stats the files that actually changed. For a mostly-idle
//! volume the diff is a few hundred records, so re-scanning a multi-GB tree
//! drops from minutes to well under a second.
//!
//! Pipeline:
//!   1. `query_journal` — FSCTL_QUERY_USN_JOURNAL → journal id + current
//!      `NextUsn`. The caller persists `UsnCursor { journal_id, next_usn }`
//!      as the "last seen" position.
//!   2. `build_frn_table` — FSCTL_ENUM_USN_DATA builds a full-volume
//!      FRN→(parent FRN, name) table. USN records carry only `parent_frn` +
//!      file name, so resolving a change back to a path needs this table.
//!      Enumeration is MFT-speed (directory entries only, no file content),
//!      so it stays well under a full walk.
//!   3. `read_changes` — FSCTL_READ_USN_JOURNAL replays records since the
//!      cursor (batch by batch, `USN_RECORD_V2`).
//!   4. `resolve_paths` folds FRN→path onto the changes; `scan_with_usn` in
//!      `lib.rs` re-stats only the touched files (USN records carry no size)
//!      and merges the diff into the cached `Node` tree.
//!
//! Known boundaries (documented on purpose):
//!   - USN records have no file size — the incremental path always re-stats
//!     changed files to keep `Node.size` accurate.
//!   - A volume whose journal was disabled, reset, or rolled over
//!     (`LowestValidUsn > cursor.next_usn`) needs a full re-scan; callers
//!     detect this from the returned `JournalState` and fall back to
//!     `scan_with_stats_cancellable`.
//!   - Non-NTFS volumes (exFAT / FAT32 / ReFS) have no change journal; the
//!     DeviceIoControl calls fail and the caller falls back to a full scan.
//!
//! The record-parsing / path-resolution half of this module is pure and
//! unit-tested off-line; the DeviceIoControl half is `#[cfg(windows)]` and
//! exercised manually on a real NTFS volume.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A single change journal event. `is_dir` comes from `FileAttributes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsnChange {
    pub frn: u64,
    pub parent_frn: u64,
    pub name: String,
    pub is_dir: bool,
    pub reason: u32,
    pub usn: i64,
}

impl UsnChange {
    /// Data-affecting reasons: DATA_OVERWRITE | DATA_EXTEND | DATA_TRUNCATION,
    /// plus create / delete (the only reasons that move a node's size).
    pub fn is_size_relevant(&self) -> bool {
        const DATA_CHANGE: u32 = 1 | 2 | 4;
        self.reason & (DATA_CHANGE | USN_REASON_FILE_CREATE | USN_REASON_FILE_DELETE) != 0
    }

    pub fn is_delete(&self) -> bool {
        self.reason & USN_REASON_FILE_DELETE != 0
    }

    pub fn is_rename_old(&self) -> bool {
        self.reason & USN_REASON_RENAME_OLD_NAME != 0
    }
}

/// A change resolved to an absolute path via the FRN table.
#[derive(Debug, Clone)]
pub struct ResolvedChange {
    pub path: PathBuf,
    pub is_dir: bool,
    pub reason: u32,
    pub frn: u64,
}

/// The persisted cursor used for the next incremental scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsnCursor {
    pub journal_id: u64,
    pub next_usn: i64,
}

/// Result of querying the volume's change journal state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalState {
    pub journal_id: u64,
    pub first_usn: i64,
    pub next_usn: i64,
    pub lowest_valid_usn: i64,
}

impl JournalState {
    /// True when the persisted cursor is still inside the journal's retained
    /// window; otherwise the caller must do a full re-scan.
    pub fn cursor_still_valid(&self, cursor: &UsnCursor) -> bool {
        cursor.journal_id == self.journal_id
            && cursor.next_usn >= self.lowest_valid_usn
            && cursor.next_usn <= self.next_usn
    }
}

// Reason flags — duplicated from the Win32 API so the pure parser stays
// platform-independent and the numeric literals are named.
pub(crate) const USN_REASON_FILE_CREATE: u32 = 0x100;
pub(crate) const USN_REASON_FILE_DELETE: u32 = 0x200;
pub(crate) const USN_REASON_RENAME_OLD_NAME: u32 = 0x1000;
pub(crate) const USN_REASON_RENAME_NEW_NAME: u32 = 0x2000;

/// Parse a raw `USN_RECORD_V2` byte buffer into `UsnChange` records.
///
/// The layout is `#[repr(C)]`-stable (Win32 ABI). Records are packed with
/// 8-byte alignment after the 8-byte header, so each record starts at an
/// offset that is a multiple of 8.
pub fn parse_usn_records(buf: &[u8]) -> Vec<UsnChange> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 8 <= buf.len() {
        let rec_len = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap_or([0; 4])) as usize;
        if rec_len == 0 {
            break;
        }
        if rec_len < 60 || off + rec_len > buf.len() {
            break;
        }
        let r = &buf[off..off + rec_len];
        let major = u16::from_le_bytes(r[4..6].try_into().unwrap_or([0; 2]));
        // USN_RECORD_V2 (major 2) is the current stable format. v3/v4 records
        // have a different header; skip rather than mis-parse.
        if major != 2 {
            off += rec_len;
            continue;
        }
        let frn = u64::from_le_bytes(r[8..16].try_into().unwrap_or([0; 8]));
        let parent_frn = u64::from_le_bytes(r[16..24].try_into().unwrap_or([0; 8]));
        let usn = i64::from_le_bytes(r[24..32].try_into().unwrap_or([0; 8]));
        let reason = u32::from_le_bytes(r[40..44].try_into().unwrap_or([0; 4]));
        let attrs = u32::from_le_bytes(r[52..56].try_into().unwrap_or([0; 4]));
        let name_len = u16::from_le_bytes(r[56..58].try_into().unwrap_or([0; 2])) as usize;
        let name_off = u16::from_le_bytes(r[58..60].try_into().unwrap_or([0; 2])) as usize;
        if name_off + name_len > rec_len || !name_len.is_multiple_of(2) {
            off += rec_len;
            continue;
        }
        let name = decode_utf16(&r[name_off..name_off + name_len]);
        out.push(UsnChange {
            frn,
            parent_frn,
            name,
            is_dir: attrs & 0x10 != 0, // FILE_ATTRIBUTE_DIRECTORY
            reason,
            usn,
        });
        off += rec_len;
    }
    out
}

fn decode_utf16(bytes: &[u8]) -> String {
    let mut u16s = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i + 2 <= bytes.len() {
        u16s.push(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
        i += 2;
    }
    String::from_utf16_lossy(&u16s)
}

/// Resolve changes to absolute paths using a full-volume FRN table built by
/// `build_frn_table` (FRN → (parent FRN, name)).
///
/// Walks each change's parent chain up to the volume root, joining names with
/// `volume_root` as the anchor. A cycle guard stops pathological chains (the
/// same parent visited twice) rather than looping forever.
pub fn resolve_paths(
    changes: &[UsnChange],
    table: &std::collections::HashMap<u64, (u64, String)>,
    volume_root: &Path,
) -> Vec<ResolvedChange> {
    let mut out = Vec::with_capacity(changes.len());
    for c in changes {
        // Skip the volume root itself (parent chain terminates there).
        if c.parent_frn == c.frn {
            continue;
        }
        let mut chain = vec![c.name.clone()];
        let mut cur = c.parent_frn;
        let mut guard = 0u32;
        let mut ok = true;
        while cur != 0 {
            if guard > 4096 {
                ok = false;
                break;
            }
            match table.get(&cur) {
                Some((parent, name)) if *parent != cur => {
                    chain.push(name.clone());
                    cur = *parent;
                }
                _ => {
                    // Reached the volume root (its parent is itself) or the
                    // table has no entry for this FRN — stop climbing.
                    break;
                }
            }
            guard += 1;
        }
        if !ok {
            continue;
        }
        // chain is file → ... → top; reverse to root → ... → file.
        let mut path = volume_root.to_path_buf();
        for seg in chain.iter().rev() {
            path.push(seg);
        }
        out.push(ResolvedChange {
            path,
            is_dir: c.is_dir,
            reason: c.reason,
            frn: c.frn,
        });
    }
    out
}

// ---- cached-tree merge ------------------------------------------------------
//
// The merge adjusts aggregates by *delta propagation* over the ancestor chain
// instead of re-summing children. That stays exact even though each directory
// only keeps its top-K largest files as visible children (the cache tree and
// a fresh walk share this truncation).

pub use crate::usn_tree::merge_changes;
// ---- windows-only DeviceIoControl plumbing --------------------------------

#[cfg(windows)]
pub mod win {
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
}

#[cfg(test)]
#[path = "usn_tests.rs"]
mod tests;
