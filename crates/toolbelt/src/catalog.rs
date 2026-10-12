//! 工具目录：嵌入式文档解析 + 索引（catalog / find / usage）。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：工具索引结构（CliTool / ToolOverride /
//! ToolbeltError）与文档解析管线（parse_catalog / catalog_cache），四个
//! 核心出口 catalog / find / usage 与命令组装共用类型。

use std::path::PathBuf;

use serde::Serialize;

use crate::risk::{default_risk, default_timeout, Risk};
use crate::EMBEDDED_DOC;

/// 只解析这些一级章节里的 `###` 工具条目（与 tubatools 白名单一致）。
const CATEGORY_WHITELIST: &[&str] = &["处理器工具", "显卡工具", "硬盘工具", "综合检测", "其他工具"];

/// 单个工具条目：索引字段 + 完整用法 markdown。
#[derive(Clone, Debug, Serialize)]
pub struct CliTool {
    pub name: String,
    pub category: String,
    pub description: String,
    /// 相对 Tools 根的 exe 路径（反斜杠已归一为正斜杠）。
    pub exe_rel: Option<String>,
    /// 完整用法（路径/参数表/示例/注意事项 的原始 markdown）。
    #[serde(skip)]
    pub detail: String,
    pub risk: Risk,
    pub timeout_secs: u64,
}

/// toolbelt.toml 的可选覆盖（放 exe 同目录）。
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct ToolOverride {
    #[serde(default)]
    pub risk: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub usage: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolbeltError {
    #[error("未找到 CLI 工具「{name}」。可用工具：{available}")]
    UnknownTool { name: String, available: String },
    #[error("工具「{name}」的文档未收录可执行文件路径，请先查看用法")]
    NoExePath { name: String },
    #[error("工具可执行文件不存在：{path}（请确认工具箱 Tools 目录完整）")]
    ExeMissing { path: PathBuf },
    #[error("Tools 根目录未找到。已尝试：{tried}")]
    ToolsRootMissing { tried: String },
    #[error("参数越界：{0}")]
    BadArgs(String),
}

/// 解析后的完整命令，可直接喂给 [`run`]。
#[derive(Clone, Debug, Serialize)]
pub struct ResolvedCommand {
    pub tool: String,
    pub category: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// exe 所在目录：多个工具把报告写在 exe 旁（DiskInfo.txt / cli_done.txt）。
    pub cwd: PathBuf,
    pub risk: Risk,
    pub timeout_secs: u64,
    /// 合并 toolbelt.toml 的 usage 后的完整用法，供 AI/UI 执行前展示。
    #[serde(skip_serializing_if = "String::is_empty")]
    pub usage: String,
}

/// 一次执行的结果。stdout/stderr 已做 UTF-8→GBK 兜底解码。
#[derive(Clone, Debug, Serialize)]
pub struct RunOutcome {
    /// 进程退出码；被超时/取消强杀时为 None。
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
}

// ── 文档解析（结构对齐 tubatools CliToolboxCatalog.Parse）─────────────

fn parse_catalog(text: &str) -> Vec<CliTool> {
    let mut tools: Vec<CliTool> = Vec::new();
    let mut category: Option<String> = None;
    let mut current: Option<CliTool> = None;
    let mut detail: Vec<&str> = Vec::new();

    let close =
        |current: &mut Option<CliTool>, detail: &mut Vec<&str>, tools: &mut Vec<CliTool>| {
            if let Some(mut t) = current.take() {
                t.detail = detail.join("\n").trim().to_string();
                detail.clear();
                t.exe_rel = extract_exe_rel(&t.detail);
                tools.push(t);
            }
        };

    for line in text.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            close(&mut current, &mut detail, &mut tools);
            let head = head.trim();
            category = if CATEGORY_WHITELIST.contains(&head) {
                Some(head.to_string())
            } else {
                None
            };
        } else if category.is_some() && line.starts_with("### ") {
            close(&mut current, &mut detail, &mut tools);
            let header = line[4..].trim();
            // 「名称 —— 简介」；分隔符缺失（如脚本示例小节）则跳过。
            let Some(idx) = header.find(" —— ") else {
                continue;
            };
            if idx == 0 {
                continue;
            }
            let name = header[..idx].trim().to_string();
            let description = header[idx + " —— ".len()..].trim().to_string();
            current = Some(CliTool {
                risk: default_risk(&name),
                timeout_secs: default_timeout(&name),
                name,
                category: category.clone().unwrap_or_default(),
                description,
                exe_rel: None,
                detail: String::new(),
            });
        } else if current.is_some() {
            detail.push(line);
        }
    }
    close(&mut current, &mut detail, &mut tools);
    tools
}

/// 提取 `**路径**：\`xxx\`` 行的第一个反引号段（相对 Tools 根）。
fn extract_exe_rel(detail: &str) -> Option<String> {
    let line = detail
        .lines()
        .find(|l| l.trim_start().starts_with("- **路径**："))?;
    let start = line.find('`')?;
    let rest = &line[start + 1..];
    let end = rest.find('`')?;
    let rel = rest[..end].trim();
    if rel.is_empty() {
        None
    } else {
        Some(rel.replace('\\', "/"))
    }
}

/// 进程级解析缓存（文档是编译期内嵌常量，解析结果恒定）。
pub(crate) fn catalog_cache() -> &'static Vec<CliTool> {
    static CACHE: std::sync::OnceLock<Vec<CliTool>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| parse_catalog(EMBEDDED_DOC))
}

/// 工具索引（分类稳定排序，便于前端分组渲染）。
pub fn catalog() -> Vec<CliTool> {
    catalog_cache().clone()
}

/// 按名字查找：不区分大小写；支持「双名工具」的部分匹配（同 tubatools）。
pub fn find(name: &str) -> Option<&'static CliTool> {
    let q = name.trim().to_ascii_lowercase();
    if q.is_empty() {
        return None;
    }
    catalog_cache().iter().find(|t| {
        let n = t.name.to_ascii_lowercase();
        n == q || n.contains(&q) || q.contains(&n)
    })
}

/// 单工具完整用法 markdown（找不到返回 None）。
pub fn usage(name: &str) -> Option<&'static str> {
    find(name).map(|t| t.detail.as_str())
}

/// `tool.plugin.json`（工具目录根）的插件元数据。与 [`crate::manifest::ToolManifest`]
/// 同构但更轻：专注「插件身份」——id/版本/作者/描述/入口/权限，供工具墙详情卡展示
/// 插件徽标、权限声明，以及后续插件市场（安装/卸载/更新）使用。
#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct ToolPluginMeta {
    /// 稳定插件 id（kebab-case，市场/权限/AI 工具注册都以此为准）。
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// 入口可执行文件（相对插件目录）。
    pub entry: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub web: Option<ToolPluginWeb>,
    /// schema 版本（当前固定 1）。
    #[serde(default)]
    pub schema: u32,
    /// 插件包完整性 sha256（hex，可选）：对「除 tool.plugin.json 自身外的所有
    /// zip 文件按路径字典序拼接」计算（规范见 [`crate::signature`] 模块注释）。
    /// 只有 digest 时 = 完整性校验，不代表签名。
    #[serde(default)]
    pub digest: String,
    /// Ed25519 签名（hex，可选）。与 [`Self::signer`] 同时存在才强制验签。
    #[serde(default)]
    pub signature: String,
    /// 签名者公钥（hex，32 字节；可选）。
    #[serde(default)]
    pub signer: String,
    /// 校验级别：`"none"`（无校验，默认）/ `"sha256"`（仅完整性）/
    /// `"ed25519"`（强制验签）。`signature`/`signer` 齐备时自动按 ed25519 处理。
    #[serde(default)]
    pub verify: String,
}

#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct ToolPluginWeb {
    pub homepage: Option<String>,
    pub download: Option<String>,
}
