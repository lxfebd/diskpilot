//! Steam library inspector. Parses Valve KeyValues files (`.acf` app manifests
//! and `libraryfolders.vdf`) into structured metadata so the desktop app can
//! show a "what games do I have, when did I last play, how big" panel without
//! ever touching game files.
//!
//! Hard rule: this crate is **read-only**. Functions only parse text input
//! and read directory listings / metadata. Nothing here writes to disk or
//! shells out to Steam.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

mod kv;
mod recommend;

pub use kv::{parse, parse_appmanifest, parse_libraryfolders, KvValue, RawAppManifest};
use recommend::{compute_recommendation, now_unix_secs};

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

pub fn discover_steam_root() -> Option<PathBuf> {
    steam_root_candidates().into_iter().find(|p| p.is_dir())
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct SteamLibrary {
    pub root: String,
    pub games: Vec<SteamGame>,
    pub total_size_bytes: u64,
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

/// Inspect a Steam install at `steam_root`. Pure read-only: parses
/// `.acf` / `.vdf` and checks directory existence for ghost detection.
pub fn inspect_at(steam_root: &Path) -> Result<SteamInventory> {
    inspect_at_with_clock(steam_root, now_unix_secs())
}

/// Test-friendly variant — accept an explicit clock so recommendation
/// strings are deterministic.
pub fn inspect_at_with_clock(steam_root: &Path, now_secs: u64) -> Result<SteamInventory> {
    let library_roots = read_library_roots(steam_root)?;
    let mut libraries = Vec::with_capacity(library_roots.len());
    for lib_root in &library_roots {
        match read_library(lib_root, now_secs) {
            Ok(l) => libraries.push(l),
            Err(e) => {
                tracing::warn!("steam-inspector: skipping library {:?}: {:#}", lib_root, e);
            }
        }
    }
    Ok(SteamInventory {
        steam_root: Some(norm_path(steam_root)),
        candidates_checked: vec![norm_path(steam_root)],
        libraries,
    })
}

fn read_library_roots(steam_root: &Path) -> Result<Vec<PathBuf>> {
    let lf = steam_root.join("config").join("libraryfolders.vdf");
    if lf.is_file() {
        let text =
            std::fs::read_to_string(&lf).with_context(|| format!("read {}", lf.display()))?;
        let parsed = parse_libraryfolders(&text)?;
        if !parsed.is_empty() {
            return Ok(parsed);
        }
    }
    // No libraryfolders.vdf, or it parsed empty — fall back to steam_root
    // itself. Common for fresh installs that haven't added a second library.
    Ok(vec![steam_root.to_path_buf()])
}

fn read_library(lib_root: &Path, now_secs: u64) -> Result<SteamLibrary> {
    let steamapps = lib_root.join("steamapps");
    let mut games = Vec::new();

    if !steamapps.is_dir() {
        return Ok(SteamLibrary {
            root: norm_path(lib_root),
            games,
            total_size_bytes: 0,
        });
    }

    for entry in std::fs::read_dir(&steamapps)
        .with_context(|| format!("read_dir {}", steamapps.display()))?
    {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("read_dir entry error: {:#}", e);
                continue;
            }
        };
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !is_appmanifest_filename(name) {
            continue;
        }

        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("read {} failed: {:#}", path.display(), e);
                continue;
            }
        };
        let raw = match parse_appmanifest(&text) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("parse {} failed: {:#}", path.display(), e);
                continue;
            }
        };

        // ACF 的 installdir 为空（Steam 允许）时 `join("")` 会解析成
        // `steamapps/common` 本身——这个目录通常存在，导致 ghost 误判为
        // 正常安装。空 installdir 一律视为 ghost（没有任何安装目录）。
        let install_path = steamapps.join("common").join(&raw.install_dir);
        let is_ghost = raw.install_dir.is_empty() || !install_path.is_dir();
        let (default_recommended, recommendation_reason) = compute_recommendation(
            raw.size_on_disk,
            raw.last_played,
            is_ghost,
            now_secs,
            raw.appid,
        );
        let workshop_item_count = count_workshop_items(lib_root, raw.appid);

        games.push(SteamGame {
            appid: raw.appid,
            name_en: raw.name.clone(),
            name_cn: None,
            install_dir_name: raw.install_dir.clone(),
            install_path: norm_path(&install_path),
            appmanifest_path: norm_path(&path),
            size_bytes: raw.size_on_disk,
            last_played_ts: raw.last_played,
            last_updated_ts: raw.last_updated,
            bytes_to_download: raw.bytes_to_download,
            bytes_downloaded: raw.bytes_downloaded,
            library_root: norm_path(lib_root),
            state_flags: raw.state_flags,
            is_fully_installed: raw.is_fully_installed(),
            is_ghost,
            default_recommended,
            recommendation_reason,
            workshop_item_count,
        });
    }

    // 汇总只统计真实安装（非 ghost）：「鬼魂安装」是 ACF 残留，其
    // SizeOnDisk 不代表实际占用，计入会让总览/列表总和虚高。未完成的
    // 更新也按 ghost 口径排除（bytes 还没真正落盘成可删占用）。
    let total_size_bytes = games
        .iter()
        .filter(|g| !g.is_ghost)
        .map(|g| g.size_bytes)
        .sum();
    Ok(SteamLibrary {
        root: norm_path(lib_root),
        games,
        total_size_bytes,
    })
}

/// Cheap workshop-subscription count for the detail-rail summary.
/// Counts immediate subdirectories of
/// `<library>/steamapps/workshop/content/<appid>/`. One read_dir, no recursion.
fn count_workshop_items(library_root: &Path, appid: u32) -> u32 {
    let dir = library_root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(appid.to_string());
    let iter = match std::fs::read_dir(&dir) {
        Ok(i) => i,
        Err(_) => return 0,
    };
    let mut count = 0u32;
    for entry in iter.flatten() {
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            count += 1;
        }
    }
    count
}

/// Full workshop-item enumeration for the modal. Walks each item dir for
/// recursive size + reads folder mtime. Run lazily on user click — too
/// slow to do eagerly for every game on every inspect.
pub fn list_workshop_items(library_root: &Path, appid: u32) -> Result<Vec<WorkshopItem>> {
    let dir = library_root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(appid.to_string());
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let id: u64 = match path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|s| s.parse().ok())
        {
            Some(id) => id,
            // Skip "downloads" etc. — workshop item dirs are always numeric.
            None => continue,
        };
        let size_bytes = dir_size_recursive(&path);
        let last_modified_ts = path
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push(WorkshopItem {
            id,
            size_bytes,
            last_modified_ts,
            path: norm_path(&path),
        });
    }
    Ok(out)
}

fn dir_size_recursive(path: &Path) -> u64 {
    let mut total = 0u64;
    let iter = match std::fs::read_dir(path) {
        Ok(i) => i,
        Err(_) => return 0,
    };
    for entry in iter.flatten() {
        let p = entry.path();
        match entry.metadata() {
            Ok(meta) if meta.is_file() => total += meta.len(),
            Ok(meta) if meta.is_dir() => total += dir_size_recursive(&p),
            _ => {}
        }
    }
    total
}

fn is_appmanifest_filename(name: &str) -> bool {
    let prefix = "appmanifest_";
    let suffix = ".acf";
    if !name.starts_with(prefix) || !name.ends_with(suffix) {
        return false;
    }
    let middle = &name[prefix.len()..name.len() - suffix.len()];
    // Empty appid ("appmanifest_.acf") is not a real Steam manifest; reject it.
    !middle.is_empty() && middle.chars().all(|c| c.is_ascii_digit())
}

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
