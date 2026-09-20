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

fn child_index(node: &crate::Node, name: &str) -> Option<usize> {
    node.children
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(name))
}

/// 沿相对路径在缓存树里找节点的 size（找不到返回 None）。用于重命名旧路径
/// 删除时——路径已消失无法 stat，回滚聚合得靠缓存节点自己的真实 size。
fn node_size_at(tree: &crate::Node, rel: &Path) -> Option<u64> {
    let mut cur = tree;
    for seg in rel.components() {
        let name = seg.as_os_str().to_string_lossy().to_string();
        match child_index(cur, &name) {
            Some(i) => cur = &cur.children[i],
            None => return None,
        }
    }
    Some(cur.size)
}

/// Apply the resolved changes to a cached tree in journal order. Returns the
/// number of changes applied structurally (add / remove / size update).
pub fn merge_changes(
    tree: &mut crate::Node,
    resolved: &[ResolvedChange],
    root: &Path,
    keep_files: Option<usize>,
) -> usize {
    let mut applied = 0usize;
    for rc in resolved {
        let rel = match rc.path.strip_prefix(root) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue, // outside the scanned subtree
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let parent_rel = rel.parent().map(|p| p.to_path_buf()).unwrap_or_default();

        // RENAME_OLD_NAME：旧路径已不存在（改名/移动走了），但树的旧节点仍
        // 挂在原路径，父目录聚合会把 ghost 算进 size/file_count。走删除分支
        // 把旧节点摘掉——但删除前 stat 会失败（路径已消失），节点还在树上，
        // 直接按缓存节点移除即可。
        let rename_old = rc.reason & USN_REASON_RENAME_OLD_NAME != 0;
        if rename_old {
            // 需要删除但路径已不在：用缓存节点的真实 size/count 回滚聚合。
            let hint = node_size_at(tree, &rel);
            if remove_node(tree, &rel, hint) {
                applied += 1;
            }
            continue;
        }

        let deleted = rc.reason & USN_REASON_FILE_DELETE != 0;
        if deleted {
            // 删除前 stat 一次拿真实大小：节点在缓存里（含 below top-K 未物化的）
            // 都要把父目录聚合回滚掉，否则 size/file_count 永久虚高。
            let size_hint = std::fs::metadata(&rc.path).ok().map(|m| m.len());
            if remove_node(tree, &rel, size_hint) {
                applied += 1;
            }
            continue;
        }
        // Not a delete: the file may still exist (size/data change, create,
        // rename-new-name). Stat it to refresh size; a vanished path is a
        // no-op (the FRN table is a snapshot, the file may be gone by now).
        let md = match std::fs::metadata(&rc.path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        apply_upsert(
            tree,
            &rel,
            &parent_rel,
            md.is_dir(),
            md.len(),
            &rc.path,
            keep_files,
        );
        applied += 1;
    }
    applied
}

/// Navigate (creating missing directories on the way) to `parent_rel`, then
/// upsert the leaf. File size / count deltas propagate up the ancestor chain.
fn apply_upsert(
    tree: &mut crate::Node,
    rel: &Path,
    parent_rel: &Path,
    is_dir: bool,
    size: u64,
    abs_path: &Path,
    keep_files: Option<usize>,
) {
    let leaf_name = rel
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut cur = &mut *tree;
    for seg in parent_rel.components() {
        let name = seg.as_os_str().to_string_lossy().to_string();
        match child_index(cur, &name) {
            Some(i) => {
                // Cached as a file but now holds children (dir promotion).
                if !cur.children[i].is_dir {
                    cur.children[i].is_dir = true;
                    cur.children[i].children = Vec::new();
                }
                cur = &mut cur.children[i];
            }
            None => {
                let path = child_abs_path(cur, &name);
                cur.children.push(crate::Node {
                    name,
                    path,
                    is_dir: true,
                    size: 0,
                    file_count: 0,
                    children: Vec::new(),
                    scaffold_id: None,
                    top_extensions: Vec::new(),
                    children_truncated: None,
                    mtime: None,
                });
                let last = cur.children.len() - 1;
                cur = &mut cur.children[last];
            }
        }
    }

    if is_dir {
        // Directory create event: ensure the node exists (empty dirs carry no
        // aggregate; their size comes from children's delta propagation).
        let path = child_abs_path(cur, &leaf_name);
        if child_index(cur, &leaf_name).is_none() {
            cur.children.push(crate::Node {
                name: leaf_name,
                path,
                is_dir: true,
                size: 0,
                file_count: 0,
                children: Vec::new(),
                scaffold_id: None,
                top_extensions: Vec::new(),
                children_truncated: None,
                mtime: None,
            });
        }
        return;
    }

    // File upsert.
    match child_index(cur, &leaf_name) {
        Some(i) => {
            let old = cur.children[i].size;
            cur.children[i].size = size;
            cur.children[i].path = abs_path.to_string_lossy().to_string();
            propagate_delta(tree, parent_rel, size as i64 - old as i64, 0);
        }
        None => {
            cur.children.push(crate::Node {
                name: leaf_name,
                path: abs_path.to_string_lossy().to_string(),
                is_dir: false,
                size,
                file_count: 1,
                children: Vec::new(),
                scaffold_id: None,
                top_extensions: Vec::new(),
                children_truncated: None,
                mtime: None,
            });
            // Trim to the same "all dirs + top-K files by size" shape the walk
            // uses; the dropped file still counted in the parent aggregate.
            cur.children.sort_by_key(|c| std::cmp::Reverse(c.size));
            if let Some(keep) = keep_files {
                let mut files_kept = 0usize;
                let mut i = cur.children.len();
                while i > 0 && files_kept < keep {
                    i -= 1;
                    if !cur.children[i].is_dir {
                        files_kept += 1;
                    }
                }
                // `i` is the index of the last kept file; drop any *file*
                // children past it (dirs stay).
                let mut j = cur.children.len();
                while j > i {
                    j -= 1;
                    if j >= i && !cur.children[j].is_dir {
                        cur.children.remove(j);
                    }
                }
            }
            propagate_delta(tree, parent_rel, size as i64, 1);
        }
    }
}

fn child_abs_path(node: &crate::Node, name: &str) -> String {
    let base = Path::new(&node.path);
    base.join(name).to_string_lossy().to_string()
}

/// Walk the existing chain from the root to `parent_rel`, adjusting each
/// node's size / file_count by the delta (saturating, never below zero).
fn propagate_delta(tree: &mut crate::Node, parent_rel: &Path, delta_size: i64, delta_count: i64) {
    fn apply(n: &mut crate::Node, ds: i64, dc: i64) {
        n.size = (n.size as i64).saturating_add(ds).max(0) as u64;
        n.file_count = (n.file_count as i64).saturating_add(dc).max(0) as u64;
    }
    let mut cur = &mut *tree;
    apply(cur, delta_size, delta_count);
    for seg in parent_rel.components() {
        let name = seg.as_os_str().to_string_lossy().to_string();
        match child_index(cur, &name) {
            Some(i) => {
                cur = &mut cur.children[i];
                apply(cur, delta_size, delta_count);
            }
            None => break, // chain not present in cache (new outside-subtree dir)
        }
    }
}

/// Remove a node by relative path. Returns true if a node was removed.
///
/// Two tricky cases must still roll back the parent aggregates:
/// - the file was below the parent's top-K (never materialized as a child) —
///   we still know its size from the FRN table stat, so propagate a pure delta;
/// - the node is a directory — `file_count` is a *recursive* count on the
///   cache tree (DirAcc accumulates children), so rolling back must subtract
///   the subtree's own recursive count, not 0.
fn remove_node(tree: &mut crate::Node, rel: &Path, size_hint: Option<u64>) -> bool {
    let parent_rel = rel.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let leaf_name = rel
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    // Navigate to the parent (no creation: deleting from a dir we know about).
    let mut cur = &mut *tree;
    let mut chain_ok = true;
    for seg in parent_rel.components() {
        let name = seg.as_os_str().to_string_lossy().to_string();
        match child_index(cur, &name) {
            Some(i) => cur = &mut cur.children[i],
            None => {
                chain_ok = false;
                break;
            }
        }
    }
    if !chain_ok {
        return false;
    }
    match child_index(cur, &leaf_name) {
        Some(i) => {
            // Node is materialized — remove it and propagate its real size /
            // recursive file_count up the chain.
            let gone = cur.children.remove(i);
            let ds = -(gone.size as i64);
            let dc = -(gone.file_count as i64);
            propagate_delta(tree, &parent_rel, ds, dc);
            true
        }
        None => {
            // Below top-K (never a child) or already gone. The FRN stat gives
            // us a size hint, so the parent aggregates still get corrected;
            // the count delta is -1 only when the hint claims a plain file.
            let Some(hint) = size_hint else {
                return false; // no size info and nothing to remove → no-op
            };
            let is_dir = std::fs::metadata(rel)
                .map(|m| m.is_dir())
                .unwrap_or(false);
            let ds = -(hint as i64);
            let dc = if is_dir { 0 } else { -1 };
            propagate_delta(tree, &parent_rel, ds, dc);
            true
        }
    }
}

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
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Build a synthetic USN_RECORD_V2 byte buffer for `parse_usn_records`.
    /// Layout (Win32 ABI): RecordLength, Major=2, Minor=0, FileRef(8),
    /// ParentRef(8), Usn(8), TimeStamp(8), Reason(4), SourceInfo(4),
    /// SecurityId(4), FileAttributes(4), FileNameLength(2), FileNameOffset(2),
    /// then UTF-16 name.
    fn build_record(
        frn: u64,
        parent: u64,
        usn: i64,
        reason: u32,
        is_dir: bool,
        name: &str,
    ) -> Vec<u8> {
        let mut buf = Vec::new();
        let name_u16: Vec<u16> = name.encode_utf16().collect();
        let name_bytes = name_u16.len() * 2;
        let total = 60 + name_bytes;
        let mut rec = vec![0u8; total];
        rec[0..4].copy_from_slice(&(total as u32).to_le_bytes());
        rec[4..6].copy_from_slice(&2u16.to_le_bytes()); // major version 2
        rec[6..8].copy_from_slice(&0u16.to_le_bytes());
        rec[8..16].copy_from_slice(&frn.to_le_bytes());
        rec[16..24].copy_from_slice(&parent.to_le_bytes());
        rec[24..32].copy_from_slice(&usn.to_le_bytes());
        rec[32..40].copy_from_slice(&0u64.to_le_bytes()); // timestamp
        rec[40..44].copy_from_slice(&reason.to_le_bytes());
        rec[44..48].copy_from_slice(&0u32.to_le_bytes()); // source info
        rec[48..52].copy_from_slice(&0u32.to_le_bytes()); // security id
        rec[52..56].copy_from_slice(&(if is_dir { 0x10u32 } else { 0u32 }).to_le_bytes());
        rec[56..58].copy_from_slice(&(name_bytes as u16).to_le_bytes());
        rec[58..60].copy_from_slice(&60u16.to_le_bytes());
        for (i, u) in name_u16.iter().enumerate() {
            rec[60 + i * 2..62 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        buf.extend_from_slice(&rec);
        buf
    }

    fn dummy_change(frn: u64, parent: u64, name: &str, reason: u32, is_dir: bool) -> UsnChange {
        UsnChange {
            frn,
            parent_frn: parent,
            name: name.to_string(),
            is_dir,
            reason,
            usn: frn as i64,
        }
    }

    #[test]
    fn parse_usn_records_roundtrip() {
        let mut buf = build_record(100, 50, 7, 0x100 | 1, false, "report.pdf");
        buf.extend_from_slice(&build_record(200, 100, 8, 0x200, true, "sub"));
        let recs = parse_usn_records(&buf);
        assert_eq!(recs.len(), 2, "两个记录都应解析出");
        assert_eq!(recs[0].frn, 100);
        assert_eq!(recs[0].parent_frn, 50);
        assert_eq!(recs[0].usn, 7);
        assert_eq!(recs[0].reason, 0x100 | 1);
        assert!(!recs[0].is_dir);
        assert_eq!(recs[0].name, "report.pdf");
        assert_eq!(recs[1].frn, 200);
        assert!(recs[1].is_dir);
        assert_eq!(recs[1].name, "sub");
    }

    #[test]
    fn parse_usn_records_stops_on_zero() {
        // A zero-length record terminates the batch.
        let mut buf = build_record(1, 2, 1, 0x100, false, "a.txt");
        buf.extend_from_slice(&[0u8; 8]);
        let recs = parse_usn_records(&buf);
        assert_eq!(recs.len(), 1);
    }

    #[test]
    fn resolve_paths_walks_parent_chain() {
        let mut table = std::collections::HashMap::new();
        // FRN → (parent, name)
        table.insert(10, (5, "Users".to_string()));
        table.insert(20, (10, "alice".to_string()));
        table.insert(30, (20, "Documents".to_string()));
        table.insert(40, (30, "report.txt".to_string()));
        let root = PathBuf::from("C:\\");
        let ch = dummy_change(40, 30, "report.txt", 1, false);
        let resolved = resolve_paths(&[ch], &table, &root);
        assert_eq!(resolved.len(), 1);
        let p = &resolved[0].path;
        assert_eq!(p, &PathBuf::from("C:\\Users\\alice\\Documents\\report.txt"));
    }

    #[test]
    fn merge_changes_updates_tree_aggregates() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
        let mut tree = crate::Node {
            name: root.file_name().unwrap().to_string_lossy().to_string(),
            path: root.to_string_lossy().to_string(),
            is_dir: true,
            size: 0,
            file_count: 0,
            children: vec![],
            scaffold_id: None,
            top_extensions: vec![],
            children_truncated: None,
            mtime: None,
        };
        // A create + data change under root/dir.
        let d = root.join("dir");
        let _ = std::fs::create_dir_all(&d);
        let f = d.join("new.bin");
        std::fs::write(&f, vec![b'x'; 100]).unwrap();
        let resolved = vec![
            ResolvedChange {
                path: d.clone(),
                is_dir: true,
                reason: USN_REASON_FILE_CREATE,
                frn: 1,
            },
            ResolvedChange {
                path: f.clone(),
                is_dir: false,
                reason: USN_REASON_FILE_CREATE,
                frn: 2,
            },
        ];
        let applied = merge_changes(&mut tree, &resolved, &root, None);
        assert_eq!(applied, 2);
        assert_eq!(tree.file_count, 1);
        assert_eq!(tree.size, 100);
        let dir = tree.children.iter().find(|c| c.name == "dir").unwrap();
        assert!(dir.is_dir);
        assert_eq!(dir.size, 100);
        assert_eq!(dir.file_count, 1);
        // Delete the file — the tree must drop the node and roll back the size.
        let del = ResolvedChange {
            path: f.clone(),
            is_dir: false,
            reason: USN_REASON_FILE_DELETE,
            frn: 2,
        };
        let applied = merge_changes(&mut tree, &[del], &root, None);
        assert_eq!(applied, 1);
        assert_eq!(tree.size, 0);
        assert_eq!(tree.file_count, 0);
        assert_eq!(tree.children[0].children.len(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cursor_still_valid_detects_reset_and_rollover() {
        let state = JournalState {
            journal_id: 42,
            first_usn: 100,
            next_usn: 500,
            lowest_valid_usn: 100,
        };
        // Same journal, cursor inside window → valid.
        let ok = UsnCursor {
            journal_id: 42,
            next_usn: 300,
        };
        assert!(state.cursor_still_valid(&ok));
        // Journal id changed → reset.
        let reset = UsnCursor {
            journal_id: 43,
            next_usn: 300,
        };
        assert!(!state.cursor_still_valid(&reset));
        // Journal rolled over (cursor below lowest_valid) → invalid.
        let rolled = UsnCursor {
            journal_id: 42,
            next_usn: 50,
        };
        assert!(!state.cursor_still_valid(&rolled));
    }

    #[test]
    fn is_size_relevant_filters_noise() {
        // Data change + create → relevant.
        assert!(dummy_change(1, 2, "f", 1, false).is_size_relevant());
        // Metadata change (basic info / security) → not relevant.
        assert!(!dummy_change(1, 2, "f", 0x8000, false).is_size_relevant());
        // Close-only → not relevant.
        assert!(!dummy_change(1, 2, "f", 0x8000_0000, false).is_size_relevant());
    }

    #[test]
    fn remove_dir_rolls_back_recursive_file_count() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
        let mut tree = crate::Node {
            name: root.file_name().unwrap().to_string_lossy().to_string(),
            path: root.to_string_lossy().to_string(),
            is_dir: true,
            size: 0,
            file_count: 0,
            children: vec![],
            scaffold_id: None,
            top_extensions: vec![],
            children_truncated: None,
            mtime: None,
        };
        let d = root.join("dir");
        let _ = std::fs::create_dir_all(&d);
        let f1 = d.join("a.bin");
        let f2 = d.join("b.bin");
        std::fs::write(&f1, vec![b'x'; 10]).unwrap();
        std::fs::write(&f2, vec![b'x'; 20]).unwrap();
        let resolved = vec![
            ResolvedChange {
                path: d.clone(),
                is_dir: true,
                reason: USN_REASON_FILE_CREATE,
                frn: 1,
            },
            ResolvedChange {
                path: f1.clone(),
                is_dir: false,
                reason: USN_REASON_FILE_CREATE,
                frn: 2,
            },
            ResolvedChange {
                path: f2.clone(),
                is_dir: false,
                reason: USN_REASON_FILE_CREATE,
                frn: 3,
            },
        ];
        merge_changes(&mut tree, &resolved, &root, None);
        assert_eq!(tree.file_count, 2);
        assert_eq!(tree.size, 30);

        // 删除目录：file_count 是递归计数，dc 必须减子树自身计数（2），
        // 不是旧实现的 0（那样父目录 file_count 永久虚高）。
        let del = ResolvedChange {
            path: d.clone(),
            is_dir: true,
            reason: USN_REASON_FILE_DELETE,
            frn: 1,
        };
        // 目录删除时路径通常已消失，size_hint=None；remove_node 的目录分支
        // 靠缓存节点自身的 size/count 回滚。
        let applied = remove_node(&mut tree, &d.strip_prefix(&root).unwrap(), None);
        assert!(applied);
        assert_eq!(tree.file_count, 0, "目录删除应回滚递归计数");
        assert_eq!(tree.size, 0);
        assert_eq!(tree.children.len(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn remove_below_top_k_still_rolls_back_aggregate() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
        let mut tree = crate::Node {
            name: root.file_name().unwrap().to_string_lossy().to_string(),
            path: root.to_string_lossy().to_string(),
            is_dir: true,
            size: 0,
            file_count: 0,
            children: vec![],
            scaffold_id: None,
            top_extensions: vec![],
            children_truncated: None,
            mtime: None,
        };
        let d = root.join("dir");
        let _ = std::fs::create_dir_all(&d);
        let big = d.join("big.bin");
        let small = d.join("small.bin");
        std::fs::write(&big, vec![b'x'; 100]).unwrap();
        std::fs::write(&small, vec![b'x'; 5]).unwrap();
        // keep_files=1 → 只有 big.bin 物化为 child，small.bin 未物化但聚合计入。
        merge_changes(
            &mut tree,
            &[
                ResolvedChange {
                    path: d.clone(),
                    is_dir: true,
                    reason: USN_REASON_FILE_CREATE,
                    frn: 1,
                },
                ResolvedChange {
                    path: big.clone(),
                    is_dir: false,
                    reason: USN_REASON_FILE_CREATE,
                    frn: 2,
                },
                ResolvedChange {
                    path: small.clone(),
                    is_dir: false,
                    reason: USN_REASON_FILE_CREATE,
                    frn: 3,
                },
            ],
            &root,
            Some(1),
        );
        assert_eq!(tree.size, 105);
        assert_eq!(tree.file_count, 2);

        // 删除未物化的小文件：stat 给出 size hint（5），聚合必须回滚到 100/1。
        let del = ResolvedChange {
            path: small.clone(),
            is_dir: false,
            reason: USN_REASON_FILE_DELETE,
            frn: 3,
        };
        let applied = merge_changes(&mut tree, &[del], &root, Some(1));
        assert_eq!(applied, 1);
        assert_eq!(tree.size, 100, "below top-K 删除也应回滚聚合 size");
        assert_eq!(tree.file_count, 1, "below top-K 删除也应回滚 file_count");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rename_old_removes_ghost_node() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
        let mut tree = crate::Node {
            name: root.file_name().unwrap().to_string_lossy().to_string(),
            path: root.to_string_lossy().to_string(),
            is_dir: true,
            size: 0,
            file_count: 0,
            children: vec![],
            scaffold_id: None,
            top_extensions: vec![],
            children_truncated: None,
            mtime: None,
        };
        let d = root.join("dir");
        let _ = std::fs::create_dir_all(&d);
        let f = d.join("old.bin");
        std::fs::write(&f, vec![b'x'; 50]).unwrap();
        merge_changes(
            &mut tree,
            &[
                ResolvedChange {
                    path: d.clone(),
                    is_dir: true,
                    reason: USN_REASON_FILE_CREATE,
                    frn: 1,
                },
                ResolvedChange {
                    path: f.clone(),
                    is_dir: false,
                    reason: USN_REASON_FILE_CREATE,
                    frn: 2,
                },
            ],
            &root,
            None,
        );
        assert_eq!(tree.size, 50);
        assert_eq!(tree.file_count, 1);

        // 重命名（RENAME_OLD_NAME）：旧路径已不存在，旧节点应被摘除，聚合回滚。
        std::fs::rename(&f, d.join("new.bin")).unwrap();
        let ren = ResolvedChange {
            path: f.clone(),
            is_dir: false,
            reason: USN_REASON_RENAME_OLD_NAME,
            frn: 2,
        };
        let applied = merge_changes(&mut tree, &[ren], &root, None);
        assert_eq!(applied, 1);
        assert_eq!(tree.size, 0, "重命名旧路径应摘掉 ghost 节点");
        assert_eq!(tree.file_count, 0);
        assert_eq!(tree.children[0].children.len(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }
}
