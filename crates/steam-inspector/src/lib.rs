//! Steam library inspector. Parses Valve KeyValues files (`.acf` app manifests
//! and `libraryfolders.vdf`) into structured metadata so the desktop app can
//! show a "what games do I have, when did I last play, how big" panel without
//! ever touching game files.
//!
//! Hard rule: this crate is **read-only**. Functions only parse text input
//! and read directory listings / metadata. Nothing here writes to disk or
//! shells out to Steam.

use anyhow::Result;
use std::path::{Path, PathBuf};

mod kv;
mod recommend;

pub use kv::{parse, parse_appmanifest, parse_libraryfolders, KvValue, RawAppManifest};
pub(crate) use recommend::{compute_recommendation, now_unix_secs};

// ---------------------------------------------------------------------------
// Steam root discovery (cross-platform)
// ---------------------------------------------------------------------------

/// All paths we try when looking for a Steam install on this OS, in priority
/// order. Returned **without** an `is_dir` filter so the empty-state UI can
/// show the user "we looked at all of these" — required by §6.6 of the
/// design doc.
pub fn steam_root_candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();

    #[cfg(windows)]
    {
        if let Some(p) = read_steam_path_from_registry() {
            out.push(p);
        }
        if let Some(pf86) = std::env::var_os("ProgramFiles(x86)") {
            out.push(PathBuf::from(pf86).join("Steam"));
        }
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            out.push(PathBuf::from(pf).join("Steam"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            out.push(home.join("Library/Application Support/Steam"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            out.push(home.join(".steam/steam"));
            out.push(home.join(".local/share/Steam"));
        }
    }

    out
}

/// Read `HKCU\Software\Valve\Steam\SteamPath` — Steam writes this on every
/// launch (forward-slash form). Skipped on non-Windows builds.
#[cfg(windows)]
fn read_steam_path_from_registry() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
    };

    let subkey: Vec<u16> = "Software\\Valve\\Steam\0".encode_utf16().collect();
    let value_name: Vec<u16> = "SteamPath\0".encode_utf16().collect();

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let res = RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey);
        if res != ERROR_SUCCESS {
            return None;
        }
        let mut buf = [0u16; 512];
        let mut len = (buf.len() * 2) as u32;
        let mut value_type: u32 = 0;
        let res = RegQueryValueExW(
            hkey,
            value_name.as_ptr(),
            std::ptr::null_mut(),
            &mut value_type,
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        );
        let _ = RegCloseKey(hkey);
        if res != ERROR_SUCCESS || value_type != REG_SZ {
            return None;
        }
        // `len` is bytes written, including the trailing NUL.
        let u16_len = (len as usize / 2).saturating_sub(1);
        let s = OsString::from_wide(&buf[..u16_len]);
        Some(PathBuf::from(s))
    }
}

// ---------------------------------------------------------------------------
// Inventory types — frontend-facing schema (mirrored in apps/desktop/src/types.ts).
// ---------------------------------------------------------------------------

/// Forward-slash normalized path string for serialization. Steam stores
/// paths with double-backslash on Windows; we normalize to slashes so the
/// frontend renders them uniformly and so test fixtures don't depend on the
/// host OS path separator.
fn norm_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SteamGame {
    pub appid: u32,
    /// English name from ACF (`name` field).
    pub name_en: String,
    /// Localized name. None until the translation pipeline fills it in.
    pub name_cn: Option<String>,
    /// Folder name only (e.g. "Counter-Strike Global Offensive").
    pub install_dir_name: String,
    /// Full normalized path to the install dir.
    pub install_path: String,
    /// Path to the source ACF file — shown as the "citation" in detail rail.
    pub appmanifest_path: String,
    pub size_bytes: u64,
    pub last_played_ts: Option<u64>,
    /// Unix seconds of the ACF's `LastUpdated` (last time Steam touched the
    /// install). None when ACF reports 0.
    pub last_updated_ts: Option<u64>,
    /// `BytesToDownload` — bytes Steam still needs to fetch for this app.
    pub bytes_to_download: u64,
    /// `BytesDownloaded` — bytes fetched so far for the in-progress update
    /// (0 when fully installed).
    pub bytes_downloaded: u64,
    pub library_root: String,
    pub state_flags: u32,
    pub is_fully_installed: bool,
    /// True when ACF is present but `install_path` doesn't exist on disk.
    pub is_ghost: bool,
    pub default_recommended: bool,
    /// Single-line reason matching §6.5 ("60GB · 8 个月未启动"). None when
    /// not recommended; the UI hides the reason block in that case.
    pub recommendation_reason: Option<String>,
    /// Number of Workshop subdirectories under
    /// `<library>/steamapps/workshop/content/<appid>/`. Cheaply computed
    /// during inspect (one read_dir + count). Detail rail uses this to
    /// decide whether to show the "查看创意工坊" button — the actual size
    /// + per-item mtimes get computed lazily by `list_workshop_items`.
    pub workshop_item_count: u32,
}

/// One Steam Workshop item under a game's `workshop/content/<appid>/`.
/// Returned by `list_workshop_items`, which walks each item dir to compute
/// recursive size + reads folder mtime as a proxy for "last updated by
/// Steam" — Steam doesn't record per-item "last used" anywhere accessible,
/// so the UI must label this as "上次更新", not "上次使用".
#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkshopItem {
    pub id: u64,
    pub size_bytes: u64,
    pub last_modified_ts: u64,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SteamResidueKind {
    /// `steamapps/common/<dir>`：目录在、没有任何 ACF 引用它。
    Install,
    /// `steamapps/workshop/content/<appid>`：订阅内容在、该库没有任何游戏有这个 appid。
    Workshop,
}

/// 卸载残留：Steam 卸载（或第三方删除）后留下的目录——与 ghost 相反的方向。
/// ghost 是「有 ACF 无目录」（不占磁盘，只是记录脏）；residue 是「有目录无
/// ACF」，**真实占用磁盘但游戏列表里完全看不到**，正是磁盘清理工具该抓的那类。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SteamResidue {
    pub kind: SteamResidueKind,
    /// 目录名（Install 是文件夹名，Workshop 是 appid 目录名）。
    pub dir_name: String,
    pub path: String,
    /// 递归目录大小（字节）。
    pub size_bytes: u64,
    /// Workshop 残留时从目录名解析出的 appid；Install 残留为 None。
    pub appid_hint: Option<u32>,
    /// 目录 mtime（Unix 秒）——UI 用它显示「多久没动过」。
    pub last_modified_ts: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SteamLibrary {
    pub root: String,
    pub games: Vec<SteamGame>,
    pub total_size_bytes: u64,
    /// 卸载残留目录（见 [`SteamResidue`]），按大小倒序。
    pub residue: Vec<SteamResidue>,
    /// 残留目录合计占用（字节）——不计入 `total_size_bytes`（那是真实安装）。
    pub residue_size_bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SteamInventory {
    /// Where we found Steam, or None if we couldn't.
    pub steam_root: Option<String>,
    /// Every path we tried — surfaced to the empty-state UI when None.
    pub candidates_checked: Vec<String>,
    pub libraries: Vec<SteamLibrary>,
}

// ---------------------------------------------------------------------------
// inspect_at / inspect — orchestration
// ---------------------------------------------------------------------------

mod inspect;
#[cfg(test)]
pub(crate) use inspect::is_appmanifest_filename;
pub use inspect::{inspect_at, inspect_at_with_clock, list_workshop_items};

/// Top-level: discover Steam, inspect if found. When not found, returns an
/// inventory with `steam_root: None` and `candidates_checked` populated so
/// the empty-state UI can show "we looked at these paths".
pub fn inspect() -> Result<SteamInventory> {
    let candidates = steam_root_candidates();
    let candidates_norm: Vec<String> = candidates.iter().map(|p| norm_path(p)).collect();
    let root = candidates.into_iter().find(|p| p.is_dir());
    match root {
        Some(r) => {
            let mut inv = inspect_at(&r)?;
            inv.candidates_checked = candidates_norm;
            Ok(inv)
        }
        None => Ok(SteamInventory {
            steam_root: None,
            candidates_checked: candidates_norm,
            libraries: Vec::new(),
        }),
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
