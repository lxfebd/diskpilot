//! Tools 根目录定位（对齐 tubatools FindToolsRoot：逐级上溯）。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：合法 Tools 根特征判定 + 逐级上溯定位
//! （exe 目录 / 当前目录）+ 图吧绿色版兜底扫描。

use std::path::{Path, PathBuf};

use crate::catalog::ToolbeltError;
use crate::plugin::find_tuba_green_tools;

/// 合法 Tools 根的强特征：至少含两个已知分类子目录，避免误中同名目录
/// （例如仓库里的 apps/desktop/Tools 只剩两个内置工具条目）。
pub(crate) fn looks_like_tools_root(p: &Path) -> bool {
    if !p.is_dir() {
        return false;
    }
    const MARKERS: &[&str] = &["硬盘工具", "综合检测", "其他工具", "处理器工具", "显卡工具"];
    MARKERS.iter().filter(|m| p.join(m).is_dir()).count() >= 2
}

fn ancestors_with_self(start: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    (0..=6).scan(Some(start.to_path_buf()), |st, _| {
        let cur = st.take()?;
        *st = cur.parent().map(|p| p.to_path_buf());
        Some(cur)
    })
}

/// 定位 Tools 根：override → exe 目录及上溯 → 当前目录及上溯。
/// 找不到返回错误并列出尝试过的路径，方便用户把工具箱目录放对位置。
pub fn find_tools_root(explicit: Option<&Path>) -> Result<PathBuf, ToolbeltError> {
    let mut tried: Vec<PathBuf> = Vec::new();
    if let Some(p) = explicit {
        tried.push(p.to_path_buf());
        if looks_like_tools_root(p) {
            return Ok(p.to_path_buf());
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        // 打包形态：<安装目录>/Tools；开发形态：target/debug → 上溯到仓库根
        for dir in ancestors_with_self(&exe) {
            let cand = dir.join("Tools");
            if looks_like_tools_root(&cand) {
                return Ok(cand);
            }
            tried.push(cand);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for dir in ancestors_with_self(&cwd) {
            let cand = dir.join("Tools");
            if looks_like_tools_root(&cand) {
                return Ok(cand);
            }
            tried.push(cand);
        }
    }
    // 兜底：扫描本机各固定盘根，找用户本地安装的「图吧工具箱」绿色版 tools 目录。
    // 图吧绿色版目录名通常是「图吧工具箱20xx」或含 tubagongju/tuba，内部 tools 子目录
    // 直接就是工具分类根（硬盘工具/综合检测/…）。让应用开箱即用，无需手动指定。
    if let Some(hit) = find_tuba_green_tools() {
        return Ok(hit);
    }
    Err(ToolbeltError::ToolsRootMissing {
        tried: tried
            .iter()
            .take(10)
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(" | "),
    })
}
