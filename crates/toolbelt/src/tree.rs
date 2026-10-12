//! 工具墙全目录扫描（对齐 tubatools ToolCatalog）。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：遍历 Tools/ 根为主面板提供完整工具墙：
//! 分类子目录去重（仅架构后缀不同）、主启动文件判定、link.json 跨分类软链 +
//! builtin 占位、Metadata/tools.json 元数据叠加、exe 真实图标抽取。
//!
//! 2026-10-12 二次拆分：exe 图标抽取 → `tree_icons`，启动文件/架构变体 →
//! `tree_arch`，link/tools 元数据 → `tree_meta`。本文件保留类型定义与
//! `catalog_tree` 主出口。

use std::path::Path;

use serde::Serialize;

use crate::catalog::ToolPluginMeta;
use crate::risk::{default_risk, Risk};
use crate::root::find_tools_root;
use crate::run::tracing_warn;

mod tree_arch;
mod tree_icons;
mod tree_meta;

pub(crate) use tree_arch::{
    all_launchables, detect_arch, find_arch_variants_with, find_primary_launchable_with,
    merge_arch_dirs, normalize_rel,
};
pub(crate) use tree_icons::{exe_icon_data_uri, is_promotion_bat};
pub(crate) use tree_meta::{find_meta, load_tools_meta, try_resolve_link, JsonToolMeta};

/// 可双击启动的文件扩展名（同 tubatools LaunchableExtensions）。
pub(crate) const LAUNCHABLE_EXTS: &[&str] =
    &[".exe", ".bat", ".cmd", ".lnk", ".msc", ".ps1", ".vbs"];

/// 目录名/文件名中的架构后缀（同 C# ArchSuffixes，补 `_x32` 且按**长度降序**）。
/// C# 里 `cpuz_x32` 会被裸 `32` 抢先截成 `cpuz_x`、`cpuz_x64` 截成 `cpuz_`，
/// 归档比较全部落空——这里让 `_x64`/`_x32`/`_ARM64` 这类更具体的后缀先命中，
/// `cpuz_x32` → `cpuz`、`cpuz_x64` → `cpuz`，都能对上目录名 `CPUZ`。
pub(crate) const ARCH_SUFFIXES: &[&str] = &[
    "_Win64", "_Win32", "_ARM64", "ARM64", "_x64", "_x86", "_x32", "_64", "_32", "x64", "x86",
    "w64", "w32", "64", "32",
];

/// 宿主架构优先级（ARM64 > x64 > x86，与 C# PreferredArchPriority 对齐）。
pub(crate) fn preferred_arch_priority() -> &'static [&'static str] {
    #[cfg(target_arch = "aarch64")]
    return &["ARM64", "x64", "x86"];
    #[cfg(target_arch = "x86")]
    return &["x86"];
    #[cfg(target_arch = "x86_64")]
    return &["x64", "x86"];
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86", target_arch = "x86_64")))]
    return &["x64", "x86"];
}

/// 单条工具墙条目（序列化给前端面板渲染）。
#[derive(Clone, Debug, Serialize)]
pub struct ToolbeltCatalogItem {
    /// 展示名：主启动文件去掉扩展名；`start.exe` 则用父目录名（同 C# GetDisplayName）。
    pub name: String,
    /// 所在分类（Tools 根下的一级子目录名）。
    pub category: String,
    /// 工具目录相对 Tools 根（正斜杠）。
    pub dir_rel: String,
    /// 主启动文件相对 Tools 根（正斜杠）；找不到可执行时为 None（如 builtin 软链）。
    pub exe_rel: Option<String>,
    /// 主启动文件扩展名（小写，无点）；builtin 软链为 "内置"。
    pub extension: String,
    /// 主启动文件（exe）的真实图标，编码为 `data:image/png;base64,...`；
    /// 抽取失败（非 exe / 无图标资源）时为 None，前端回退到 Lucide 图标。
    pub icon: Option<String>,
    /// tools.json 叠来的简介；未收录则空串。
    pub description: String,
    /// tools.json 叠来的标签。
    pub tags: Vec<String>,
    /// 是否为 link.json 跨分类软链。
    pub is_linked: bool,
    /// 是否为 link.json builtin 占位（点开走内置逻辑，无本地文件）。
    pub is_builtin_link: bool,
    /// 主启动文件检测到的架构（"x64"/"x86"/"ARM64"，未知为空）。
    pub arch: String,
    /// 同目录/跨目录检测到的其他架构版本（文件名 → 架构）。
    pub arch_variants: Vec<ArchVariantOut>,
    /// 反向引用：哪些分类的 link.json 链到了本工具（跨分类复用的「主分类」标注）。
    pub linked_from: Vec<String>,
    /// 展示用风险级（"low"/"medium"/"high"；tools.json risk 优先，否则按名默认）。
    pub risk: String,
    /// tools.json 的 launchTarget（覆盖主启动文件，原版语义：指定入口 exe）。
    pub launch_target: Option<String>,
    /// tools.json 透传字段（详情卡「关于」区）。
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub tutorial_url: Option<String>,
    pub download_url: Option<String>,
    /// 是否为推广项：启动脚本（.bat）只做 `start <推广URL>` 跳转（流量卡/加速器
    /// 返利链等），不是真正可用的工具软件。前端据此灰化并给「推广」徽标，
    /// 提供一键隐藏；官网下载/在线工具跳转不算推广（is_promotion=false）。
    pub is_promotion: bool,
    /// 目录内 `tool.plugin.json` 的插件元数据（dsh 万物皆插件模式的本地落地）：
    /// 有则说明该工具是一个可装卸的插件单元；无则 null（传统工具照旧）。
    pub plugin: Option<ToolPluginMeta>,
}

/// 读取工具目录的 `tool.plugin.json`；缺失/解析失败返回 None（传统工具不是插件）。
fn load_plugin_meta(tool_dir: &Path) -> Option<ToolPluginMeta> {
    let p = tool_dir.join("tool.plugin.json");
    let text = std::fs::read_to_string(&p).ok()?;
    let meta: ToolPluginMeta = serde_json::from_str(&text).ok()?;
    if meta.id.trim().is_empty() || meta.entry.trim().is_empty() {
        return None;
    }
    Some(meta)
}

/// 一个架构变体（面板「其他版本」下拉用）。
#[derive(Clone, Debug, Serialize)]
pub struct ArchVariantOut {
    pub name: String,
    pub file_rel: String,
    pub arch: String,
}

/// 全目录扫描结果。
#[derive(Clone, Serialize)]
pub struct ToolbeltCatalog {
    pub tools_root: Option<String>,
    /// 分类 → 工具条目（按原版 12 分类顺序，未命中按目录名字母序排后）。
    pub categories: Vec<CategoryOut>,
    /// 工具总数（去重后）。
    pub total: usize,
    /// 原版分类显示顺序（前端左栏导航按此渲染）。
    pub category_order: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct CategoryOut {
    pub name: String,
    pub tools: Vec<ToolbeltCatalogItem>,
}

/// **缝口 A 核心出口**：扫描整个 Tools/ 根，返回完整工具墙。
///
/// 不依赖任何嵌入式文档——磁盘上有什么就列什么。每分类下先 `merge_arch_dirs`
/// 去重，再逐目录判主启动文件、收架构变体、解 link.json、叠 tools.json 元数据。
///
/// 重构（全应用重构 W1）：
/// - 分类按原版 12 分类顺序排序（`display_order_rank`），未命中按目录名字母序排后，
///   并在返回值带 `category_order`（按原版顺序排好的分类名，前端左栏导航用）。
/// - 反向收集 link.json 跨分类引用：每个工具条目带 `linked_from`（哪些分类链到了它）。
/// - 每个工具目录只 `all_launchables` 一次（主文件判定 + 架构变体共享），去重 IO。
pub fn catalog_tree(explicit_root: Option<&Path>) -> ToolbeltCatalog {
    let root = match find_tools_root(explicit_root) {
        Ok(r) => r,
        Err(e) => {
            tracing_warn(&e.to_string());
            return ToolbeltCatalog {
                tools_root: None,
                categories: Vec::new(),
                total: 0,
                category_order: Vec::new(),
            };
        }
    };

    let meta = load_tools_meta(&root);

    // 第一趟：列出全部分类 + 每分类目录，并轻量收集 link.json 的跨分类 target。
    // linked_from 表：target 目录相对根（正斜杠，去尾斜杠）→ 链到它的分类名列表。
    let mut cat_dirs: Vec<(String, Vec<String>)> = Vec::new();
    let mut linked_from: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut cat_names: Vec<String> = std::fs::read_dir(&root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    // 原版 12 分类顺序优先，未命中按字母序排后。
    cat_names.sort_by(|a, b| {
        let ra = display_order_rank(a);
        let rb = display_order_rank(b);
        if ra == rb {
            a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
        } else {
            ra.cmp(&rb)
        }
    });

    for cat in &cat_names {
        let category_root = root.join(cat);
        let mut dir_names: Vec<String> = std::fs::read_dir(&category_root)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        dir_names.sort_by_key(|a| a.to_ascii_lowercase());
        cat_dirs.push((cat.clone(), dir_names.clone()));

        for dir_name in merge_arch_dirs(&dir_names) {
            let tool_dir = category_root.join(&dir_name);
            if let Some(link) = try_resolve_link(&tool_dir) {
                if let Some(target) = link.target_rel.as_deref() {
                    if !target.trim().is_empty() {
                        linked_from
                            .entry(target.trim_end_matches('/').to_string())
                            .or_default()
                            .push(cat.clone());
                    }
                }
            }
        }
    }

    let mut categories = Vec::new();
    let mut total = 0usize;

    for (cat, dir_names) in cat_dirs {
        let category_root = root.join(&cat);
        let mut tools = Vec::new();
        for dir_name in merge_arch_dirs(&dir_names) {
            let tool_dir = category_root.join(&dir_name);
            // 本目录真实归属的「目标工具目录」相对根（普通目录=自身；link=target）。
            // 用于查 linked_from 反向引用（排除自身分类）。
            let mut link_cats: Vec<String> = Vec::new();
            if let Some(cats) = linked_from.get(&normalize_rel(&root, &tool_dir)) {
                link_cats = cats.iter().filter(|c| **c != cat).cloned().collect();
            }

            // link.json 软链优先（builtin / 跨分类 target）
            if let Some(link) = try_resolve_link(&tool_dir) {
                if link.is_builtin() {
                    total += 1;
                    tools.push(ToolbeltCatalogItem {
                        name: dir_name.clone(),
                        category: cat.clone(),
                        dir_rel: normalize_rel(&root, &tool_dir),
                        exe_rel: None,
                        extension: "内置".to_string(),
                        icon: None,
                        description: String::new(),
                        tags: Vec::new(),
                        is_linked: true,
                        is_builtin_link: true,
                        arch: String::new(),
                        arch_variants: Vec::new(),
                        linked_from: link_cats,
                        risk: risk_of(&dir_name, None).to_string(),
                        launch_target: None,
                        publisher: None,
                        version: None,
                        tutorial_url: None,
                        download_url: None,
                        is_promotion: false,
                        plugin: load_plugin_meta(&tool_dir),
                    });
                    continue;
                }
                // 跨分类 target：主文件在目标目录里解析，展示分类仍挂在当前分类下
                let Some(target_rel) = link.target_rel.as_deref() else {
                    continue;
                };
                let target_full = root.join(target_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
                if !target_full.is_dir() {
                    continue;
                }
                let Some(target_dir_name) = target_full.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                let target_all = all_launchables(&target_full);
                if let Some(primary) =
                    find_primary_launchable_with(&target_full, target_dir_name, &target_all)
                {
                    let meta_hit =
                        find_meta(&meta, &normalize_rel(&root, &primary), target_dir_name);
                    let (primary_rel, arch) = primary_rel_arch(&root, &primary);
                    let variants = find_arch_variants_with(
                        &root,
                        &target_full,
                        &target_full
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_default(),
                        &primary,
                        target_dir_name,
                        &target_all,
                    );
                    // link 条目的 linked_from 用自身目录查（它本身可能是其他分类链来的）
                    let self_link_cats = linked_from
                        .get(&normalize_rel(&root, &tool_dir))
                        .map(|c| c.iter().filter(|x| **x != cat).cloned().collect())
                        .unwrap_or_default();
                    let (name, meta_disp) = meta_display(&meta_hit, &primary, target_dir_name);
                    total += 1;
                    tools.push(ToolbeltCatalogItem {
                        name,
                        category: cat.clone(),
                        dir_rel: normalize_rel(&root, &tool_dir),
                        exe_rel: Some(primary_rel),
                        extension: ext_of(&primary),
                        icon: exe_icon_data_uri(&primary),
                        description: meta_disp.0,
                        tags: meta_disp.1,
                        is_linked: true,
                        is_builtin_link: false,
                        arch,
                        arch_variants: variants,
                        linked_from: self_link_cats,
                        risk: risk_of(target_dir_name, meta_hit).to_string(),
                        launch_target: meta_hit.and_then(|m| m.launch_target.clone()),
                        publisher: meta_hit.and_then(|m| m.publisher.clone()),
                        version: meta_hit.and_then(|m| m.version.clone()),
                        tutorial_url: meta_hit.and_then(|m| m.tutorial_url.clone()),
                        download_url: meta_hit.and_then(|m| m.download_url.clone()),
                        is_promotion: is_promotion_bat(&primary),
                        plugin: load_plugin_meta(&target_full),
                    });
                }
                continue;
            }

            // 普通工具目录：一次收集，主文件 + 架构变体共享
            let all = all_launchables(&tool_dir);
            if let Some(primary) = find_primary_launchable_with(&tool_dir, &dir_name, &all) {
                let meta_hit = find_meta(&meta, &normalize_rel(&root, &primary), &dir_name);
                let (primary_rel, arch) = primary_rel_arch(&root, &primary);
                let variants = find_arch_variants_with(
                    &root,
                    &tool_dir,
                    &category_root,
                    &primary,
                    &dir_name,
                    &all,
                );
                let (name, meta_disp) = meta_display(&meta_hit, &primary, &dir_name);
                total += 1;
                tools.push(ToolbeltCatalogItem {
                    name,
                    category: cat.clone(),
                    dir_rel: normalize_rel(&root, &tool_dir),
                    exe_rel: Some(primary_rel),
                    extension: ext_of(&primary),
                    icon: exe_icon_data_uri(&primary),
                    description: meta_disp.0,
                    tags: meta_disp.1,
                    is_linked: false,
                    is_builtin_link: false,
                    arch,
                    arch_variants: variants,
                    linked_from: link_cats,
                    risk: risk_of(&dir_name, meta_hit).to_string(),
                    launch_target: meta_hit.and_then(|m| m.launch_target.clone()),
                    publisher: meta_hit.and_then(|m| m.publisher.clone()),
                    version: meta_hit.and_then(|m| m.version.clone()),
                    tutorial_url: meta_hit.and_then(|m| m.tutorial_url.clone()),
                    download_url: meta_hit.and_then(|m| m.download_url.clone()),
                    is_promotion: is_promotion_bat(&primary),
                    plugin: load_plugin_meta(&tool_dir),
                });
            }
        }
        if !tools.is_empty() {
            categories.push(CategoryOut { name: cat, tools });
        }
    }

    ToolbeltCatalog {
        tools_root: Some(root.display().to_string()),
        categories,
        total,
        category_order: cat_names,
    }
}

/// 展示用风险级：tools.json risk 优先，否则按工具名默认（`default_risk`）。
fn risk_of(name: &str, meta: Option<&JsonToolMeta>) -> Risk {
    if let Some(r) = meta.and_then(|m| m.risk.as_deref()).and_then(Risk::parse) {
        return r;
    }
    default_risk(name)
}

/// 展示名（尊重 tools.json displayName）+ (description, tags)。
fn meta_display(
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
fn display_order_rank(cat: &str) -> usize {
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

fn primary_rel_arch(root: &Path, primary: &Path) -> (String, String) {
    let rel = normalize_rel(root, primary);
    let arch = primary
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(detect_arch)
        .unwrap_or("")
        .to_string();
    (rel, arch)
}

fn ext_of(p: &Path) -> String {
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
