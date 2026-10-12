//! link.json / tools.json 元数据解析（从 tree.rs 拆分，2026-10-12）。
//! LinkInfo + JsonToolMeta/JsonToolsDoc + try_resolve_link + load_tools_meta + find_meta。

use std::path::Path;

use crate::run::tracing_warn;

pub(crate) struct LinkInfo {
    /// "target" 相对 Tools 根的目标目录；builtin 软链为 None。
    pub(crate) target_rel: Option<String>,
    /// "builtin" 内置工具 id。
    pub(crate) builtin_id: Option<String>,
}

impl LinkInfo {
    pub(crate) fn is_builtin(&self) -> bool {
        self.builtin_id.is_some()
    }
}

/// 解析工具目录里的 link.json：目录必须「只有一个文件且无子目录」才视为软链
/// （同 C# TryResolveLink 的形态校验）。
pub(crate) fn try_resolve_link(tool_dir: &Path) -> Option<LinkInfo> {
    let link_path = tool_dir.join("link.json");
    if !link_path.is_file() {
        return None;
    }
    let files: Vec<_> = std::fs::read_dir(tool_dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let dirs: Vec<_> = files.iter().filter(|p| p.is_dir()).collect();
    if files.len() != 1 || !dirs.is_empty() {
        return None;
    }
    let text = std::fs::read_to_string(&link_path).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    if let Some(b) = doc.get("builtin").and_then(|v| v.as_str()) {
        if !b.trim().is_empty() {
            return Some(LinkInfo {
                target_rel: None,
                builtin_id: Some(b.to_string()),
            });
        }
    }
    if let Some(t) = doc.get("target").and_then(|v| v.as_str()) {
        if !t.trim().is_empty() {
            return Some(LinkInfo {
                target_rel: Some(t.to_string()),
                builtin_id: None,
            });
        }
    }
    None
}

/// tools.json 单条元数据（面板 + 详情卡展示字段；launchTarget 供启动走
/// 指定入口，与原版 TubaWinUi3 的 launchTarget 语义一致）。
#[derive(Debug, Default, Clone, serde::Deserialize)]
pub(crate) struct JsonToolMeta {
    #[serde(rename = "match", default)]
    pub(crate) match_name: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(rename = "launchTarget", default)]
    pub(crate) launch_target: Option<String>,
    #[serde(default)]
    pub(crate) publisher: Option<String>,
    #[serde(default)]
    pub(crate) version: Option<String>,
    #[serde(rename = "tutorialUrl", default)]
    pub(crate) tutorial_url: Option<String>,
    #[serde(rename = "downloadUrl", default)]
    pub(crate) download_url: Option<String>,
    #[serde(rename = "displayName", default)]
    pub(crate) display_name: Option<String>,
    /// 风险覆盖（tools.json 权威度高于默认，toolbelt.toml 仍可再盖）。
    #[serde(default)]
    pub(crate) risk: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct JsonToolsDoc {
    #[serde(default)]
    tools: Vec<JsonToolMeta>,
}

/// 匹配 tools.json 的 `match`（宽松：包含 / 去空格-下划线后包含，同 C# MatchesFlexible）。
fn meta_matches(source: &str, m: &str) -> bool {
    if source
        .to_ascii_lowercase()
        .contains(&m.to_ascii_lowercase())
    {
        return true;
    }
    let norm = |s: &str| s.replace([' ', '-', '_'], "").to_ascii_lowercase();
    norm(source).contains(&norm(m))
}

/// 加载 Tools 根旁（或根内）的 Metadata/tools.json，返回 (match, meta) 列表。
/// 找不到/解析失败返回空，不 panic。
pub(crate) fn load_tools_meta(tools_root: &Path) -> Vec<(String, JsonToolMeta)> {
    let mut cands = Vec::new();
    if let Some(parent) = tools_root.parent() {
        cands.push(parent.join("Metadata").join("tools.json"));
        cands.push(parent.join("tools.json"));
    }
    cands.push(tools_root.join("Metadata").join("tools.json"));
    cands.push(tools_root.join("tools.json"));
    for c in cands {
        let Ok(text) = std::fs::read_to_string(&c) else {
            continue;
        };
        match serde_json::from_str::<JsonToolsDoc>(&text) {
            Ok(doc) => {
                return doc
                    .tools
                    .into_iter()
                    .filter_map(|t| t.match_name.clone().map(|m| (m, t)))
                    .collect();
            }
            Err(e) => tracing_warn(&format!("tools.json 解析失败 {:?}: {}", c, e)),
        }
    }
    Vec::new()
}

/// 查 tools.json 元数据：主文件相对路径（或目录名）优先精确包含 match，
/// 相同匹配长度取最长 match（同 C# FindJsonMetadata 的 OrderByDescending Match.Length）。
pub(crate) fn find_meta<'a>(
    meta: &'a [(String, JsonToolMeta)],
    primary_rel: &str,
    dir_name: &str,
) -> Option<&'a JsonToolMeta> {
    meta.iter()
        .filter(|(m, _)| meta_matches(primary_rel, m) || meta_matches(dir_name, m))
        .max_by_key(|(m, _)| m.chars().count())
        .map(|(_, t)| t)
}
