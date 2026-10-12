//! Steam 库扫描域：inspect_at 主入口 + ACF 目录遍历 → SteamLibrary/SteamGame。
//! 从 lib.rs 拆分（2026-10-12）。只读：仅解析文本 + 读目录元数据。

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::{
    compute_recommendation, norm_path, now_unix_secs, parse_appmanifest, parse_libraryfolders,
    SteamGame, SteamInventory, SteamLibrary, WorkshopItem,
};

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

pub(crate) fn is_appmanifest_filename(name: &str) -> bool {
    let prefix = "appmanifest_";
    let suffix = ".acf";
    if !name.starts_with(prefix) || !name.ends_with(suffix) {
        return false;
    }
    let middle = &name[prefix.len()..name.len() - suffix.len()];
    // Empty appid ("appmanifest_.acf") is not a real Steam manifest; reject it.
    !middle.is_empty() && middle.chars().all(|c| c.is_ascii_digit())
}
