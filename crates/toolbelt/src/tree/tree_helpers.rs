//! catalog_tree 的展示/排序辅助函数：风险级、显示名扩展名、架构主文件
//! 相对路径、分类排序。从 tree.rs 拆分（2026-10-12）。纯函数无 IO。

use std::path::Path;

use super::{detect_arch, normalize_rel, JsonToolMeta};
use crate::risk::{default_risk, Risk};

/// 展示用风险级：tools.json risk 优先，否则按工具名默认（`default_risk`）。
pub(crate) fn risk_of(name: &str, meta: Option<&JsonToolMeta>) -> Risk {
    if let Some(r) = meta.and_then(|m| m.risk.as_deref()).and_then(Risk::parse) {
        return r;
    }
    default_risk(name)
}

/// 展示名（尊重 tools.json displayName）+ (description, tags)。
pub(crate) fn meta_display(
    meta: &Option<&JsonToolMeta>,
    primary: &Path,
    dir_name: &str,
) -> (String, (String, Vec<String>)) {
    let name = meta
        .and_then(|m| m.display_name.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| get_display_name(primary, dir_name));
    let desc = meta.and_then(|m| m.description.clone()).unwrap_or_default();
    let tags = meta.map(|m| m.tags.clone()).unwrap_or_default();
    (name, (desc, tags))
}

/// 原版 12 分类显示顺序的磁盘目录名排序权重（未命中 = usize::MAX 排最后）。
/// 前端另有「磁盘目录名 → 原版显示名」映射表；这里只保证排序语义一致。
pub(crate) fn display_order_rank(cat: &str) -> usize {
    match cat {
        "硬件信息" => 1,
        "处理器工具" => 2,
        "CPU工具" => 2,
        "主板工具" => 3,
        "内存工具" => 4,
        "显卡工具" => 5,
        "磁盘工具" => 6,
        "硬盘工具" => 6,
        "显示器工具" => 7,
        "屏幕工具" => 7,
        "综合检测" => 8,
        "综合工具" => 8,
        "常用工具" => 8,
        "外设工具" => 9,
        "烤鸡工具" => 10,
        "游戏工具" => 11,
        "其他工具" => 12,
        _ => usize::MAX,
    }
}

pub(crate) fn primary_rel_arch(root: &Path, primary: &Path) -> (String, String) {
    let rel = normalize_rel(root, primary);
    let arch = primary
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(detect_arch)
        .unwrap_or("")
        .to_string();
    (rel, arch)
}

pub(crate) fn ext_of(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// 展示名：非 start 文件用主文件名（去扩展名）；start.* 用父目录名（同 C# GetDisplayName）。
fn get_display_name(primary: &Path, dir_name: &str) -> String {
    let stem = primary
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(dir_name);
    if stem.eq_ignore_ascii_case("start") {
        dir_name.to_string()
    } else {
        stem.to_string()
    }
}
