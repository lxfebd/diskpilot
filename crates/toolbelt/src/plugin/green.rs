//! 图吧工具箱绿色版兜底扫描：find_tuba_green_tools / parse_green_version。
//! 从 plugin.rs 拆分（2026-10-12）。只探盘根三层内，多份取版本号最大者。

use std::path::{Path, PathBuf};

use crate::root::looks_like_tools_root;
/// 扫描本机固定盘，定位「图吧工具箱」绿色版的 tools 目录。
/// 图吧绿色版通常在盘根放一个「图吧工具箱20xx」文件夹，内含 tools 分类目录。
/// 命中多个时取目录名里版本号最大（最新）的一份，方便用户升级后自动切到新版。
pub(crate) fn find_tuba_green_tools() -> Option<PathBuf> {
    let mut best: Option<(PathBuf, u64)> = None;
    // 只探盘根下三层以内，避免全盘递归拖慢启动。
    let mut probe = |p: &Path, depth: u8| {
        if depth > 3 || !p.is_dir() {
            return;
        }
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for ent in rd.flatten() {
            let sub = ent.path();
            if !sub.is_dir() {
                continue;
            }
            let Some(fname) = sub.file_name() else {
                continue;
            };
            let name = fname.to_string_lossy().to_lowercase();
            let is_tuba = name.contains("图吧")
                || name.contains("工具箱")
                || name.contains("tubagongju")
                || name.contains("tuba");
            if !is_tuba {
                continue;
            }
            let tools_dir = sub.join("tools");
            if looks_like_tools_root(&tools_dir) {
                let ver = parse_green_version(&name);
                if best.as_ref().is_none_or(|(_, bv)| ver > *bv) {
                    best = Some((tools_dir.clone(), ver));
                }
            }
        }
    };
    for letter in b'C'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        probe(std::path::Path::new(&root), 0);
    }
    best.map(|(p, _)| p)
}

/// 从「图吧工具箱202406」这类目录名里解析版本号，用于多份间选最新；解析不出返回 0。
fn parse_green_version(name: &str) -> u64 {
    let mut v: u64 = 0;
    for chunk in name
        .chars()
        .collect::<Vec<_>>()
        .chunk_by(|a, b| a.is_ascii_digit() == b.is_ascii_digit())
    {
        let s: String = chunk.iter().collect();
        if let Ok(n) = s.parse::<u64>() {
            v = v * 100 + n;
        }
    }
    v
}
