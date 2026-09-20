//! 图吧工具箱 CLI 工具目录（对齐 luolangaga/tubatools TubaWinUi3 的
//! CliToolboxCatalog + run_cli_tool 设计）。
//!
//! 权威参数表就是《CLI工具使用文档.md》本身（收录 17 个可命令行化调用的
//! 工具，路径/参数/示例/风险全部出自该文），编译期用 `include_str!` 内嵌，
//! 运行时解析——文档即目录，不需要额外维护一份可能过期的参数表。
//!
//! 核心出口：
//! - [`catalog`] / [`find`]：工具索引（分类 / 名称 / 简介 / exe 相对路径）
//! - [`usage`]：单个工具的完整用法（参数表 + 示例 markdown）
//! - [`toolbelt_command_for`]：把「工具名 + 参数」解析成可直接执行的命令
//!   （exe 绝对路径 / 实参 / 工作目录 / 风险级 / 超时）
//! - [`run`]：执行并捕获输出，超时强杀
//!
//! 工具目录旁可放 `toolbelt.toml` 覆盖默认值（schema 同 apps/desktop 的
//! 工具箱约定）：`risk = "low|medium|high"`、`args = [...]`（固定默认参数，
//! 调用方给了实参则被覆盖）、`usage = "..."`、`timeout_secs = 60`。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

pub mod manifest;
pub use manifest::{all_manifests, find_manifest, parser_for, ToolManifest, ToolMode};

pub mod signature;

/// 内嵌的权威 CLI 文档（源：tubatools 仓库根《CLI工具使用文档.md》）。
pub const EMBEDDED_DOC: &str = include_str!("../assets/cli_tools_doc.md");

/// 只解析这些一级章节里的 `###` 工具条目（与 tubatools 白名单一致）。
const CATEGORY_WHITELIST: &[&str] = &["处理器工具", "显卡工具", "硬盘工具", "综合检测", "其他工具"];

/// 执行前需要用户在 UI 确认的最低风险级是 MEDIUM（同 toolbelt.toml 约定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// 只读/导出类，无副作用
    Low,
    /// 可逆但有副作用（烤机、整理、设备禁用…）
    Medium,
    /// 破坏性/变砖风险（刷 BIOS、分区、格式化）
    High,
}

impl Risk {
    pub fn parse(s: &str) -> Option<Risk> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Risk::Low),
            "medium" => Some(Risk::Medium),
            "high" => Some(Risk::High),
            _ => None,
        }
    }
}

impl std::fmt::Display for Risk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Risk::Low => "low",
            Risk::Medium => "medium",
            Risk::High => "high",
        };
        f.write_str(s)
    }
}

/// 每个工具的风险默认值。依据文档里的「注意事项」与副作用程度定级：
/// 工具本身静态一个级别，`toolbelt.toml` 可按目录覆盖。
fn default_risk(tool_name: &str) -> Risk {
    // 名字可能是 "Autoruns / autorunsc" 这种双名，统一小写包含匹配。
    let n = tool_name.to_ascii_lowercase();
    let hit = |keys: &[&str]| keys.iter().any(|k| n.contains(k));
    if hit(&["fpt", "diskgenius", "ventoy", "nvidiainspector"]) {
        Risk::High // 刷 BIOS / 分区 / 格式化目标盘 / 电压超频
    } else if hit(&[
        "prime95",
        "furmark",
        "nvidiaprofileinspector",
        "defraggler",
        "urwtest",
        "usbdeview",
        "windbg",
    ]) {
        Risk::Medium // 烤机 / 数据布局重写 / 写满测试盘 / 卸载设备 / 调试附加
    } else {
        Risk::Low
    }
}

/// 长时间运行的工具（烤机/写盘测试）默认放宽超时（秒）；其余 60s。
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const LONG_RUNNING_TIMEOUT_SECS: u64 = 1800;
pub const MIN_TIMEOUT_SECS: u64 = 5;
pub const MAX_TIMEOUT_SECS: u64 = 3600;

fn default_timeout(tool_name: &str) -> u64 {
    let n = tool_name.to_ascii_lowercase();
    if ["prime95", "furmark", "urwtest", "defraggler", "ventoy"]
        .iter()
        .any(|k| n.contains(k))
    {
        LONG_RUNNING_TIMEOUT_SECS
    } else {
        DEFAULT_TIMEOUT_SECS
    }
}

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
fn catalog_cache() -> &'static Vec<CliTool> {
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

// ── Tools 根目录定位（对齐 tubatools FindToolsRoot：逐级上溯）──────────

/// 合法 Tools 根的强特征：至少含两个已知分类子目录，避免误中同名目录
/// （例如仓库里的 apps/desktop/Tools 只剩两个内置工具条目）。
fn looks_like_tools_root(p: &Path) -> bool {
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

/// 从本地 `.zip` 插件包安装：解析包内 `tool.plugin.json` 的 id/category，
/// 解压到 Tools 根对应分类下（插件目录 = `<分类>/<id>`），已存在则报错（避免
/// 覆盖用户数据，升级走「先卸载再安装」）。路径穿越防护：拒绝 `../` 与绝对路径
/// 条目；文件名解码失败跳过。返回安装后的目录相对 Tools 根路径。
pub fn install_plugin_zip(zip_path: &Path, tools_root: &Path) -> Result<String, ToolbeltError> {
    let file = std::fs::File::open(zip_path).map_err(|e| {
        ToolbeltError::BadArgs(format!("无法打开插件包 {}: {e}", zip_path.display()))
    })?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| ToolbeltError::BadArgs(format!("不是合法的 zip 包: {e}")))?;

    // 1. 解析包内 tool.plugin.json（可能在根，也可能在单层子目录里）
    let (meta, base_dir) = {
        let mut found = None;
        for i in 0..archive.len() {
            let mut f = archive
                .by_index(i)
                .map_err(|e| ToolbeltError::BadArgs(e.to_string()))?;
            if f.is_dir() {
                continue;
            }
            let Some(name) = f.name().split('/').next_back() else {
                continue;
            };
            if name == "tool.plugin.json" {
                let mut buf = String::new();
                use std::io::Read;
                f.read_to_string(&mut buf)
                    .map_err(|e| ToolbeltError::BadArgs(e.to_string()))?;
                let meta: ToolPluginMeta = serde_json::from_str(&buf).map_err(|e| {
                    ToolbeltError::BadArgs(format!("tool.plugin.json 解析失败: {e}"))
                })?;
                // 取该文件所在目录（去尾 '/'），作为解压基目录
                let dir = f
                    .name()
                    .rsplit_once('/')
                    .map(|(d, _)| d.to_string())
                    .unwrap_or_default();
                found = Some((meta, dir));
                break;
            }
        }
        found.ok_or_else(|| ToolbeltError::BadArgs("插件包内缺少 tool.plugin.json".into()))?
    };

    // 1.5 签名校验（B1 插件签名，2026-09-09）：
    // 收集 zip 内除 tool.plugin.json 自身外全部文件（路径字典序 → 签名对象），
    // 按 meta 里的 digest/signature/signer 做 sha256 完整性 + Ed25519 验签。
    // verify 字段 = "none" 显式跳过（本地 zip 直装允许无签名）；"sha256" 只查完整性；
    // "ed25519" 或 signature+signer 齐备则强制验签。校验失败直接拒绝安装。
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut f = match archive.by_index(i) {
            Ok(f) => f,
            Err(e) => {
                return Err(ToolbeltError::BadArgs(format!("读取插件包条目失败: {e}")));
            }
        };
        if f.is_dir() {
            continue;
        }
        let name = f.name().replace('\\', "/");
        let base = name.split('/').next().unwrap_or("");
        if base == "tool.plugin.json"
            || name == "tool.plugin.json"
            || name.ends_with("/tool.plugin.json")
        {
            continue; // 清单自证是循环引用，排除
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            continue;
        }
        entries.push((name, buf));
    }
    let verify_mode = meta.verify.trim().to_ascii_lowercase();
    let require_signed = match verify_mode.as_str() {
        "none" => false,
        "sha256" => {
            if let Err(e) = signature::verify_sha256(&entries, &meta.digest) {
                return Err(ToolbeltError::BadArgs(e));
            }
            false
        }
        "ed25519" => true,
        _ => {
            // 未声明 verify 但带签名域 → 走完整校验；只有 signature 没有 signer
            // 之类半截签名也按「未完整签名」处理，只查 digest（如有）。
            let has_sig = !meta.signature.trim().is_empty() && !meta.signer.trim().is_empty();
            if has_sig {
                true
            } else {
                if let Err(e) = signature::verify_sha256(&entries, &meta.digest) {
                    return Err(ToolbeltError::BadArgs(e));
                }
                false
            }
        }
    };
    if require_signed {
        if meta.signer.trim().is_empty() {
            return Err(ToolbeltError::BadArgs(
                "插件声明了 Ed25519 验签但缺少 signer（签名者公钥）".into(),
            ));
        }
        signature::verify_ed25519(&entries, &meta.signature, &meta.signer)
            .map_err(|e| ToolbeltError::BadArgs(e))?;
    }

    // id/category 净化（到期前是路径穿越的一半：它们直接拼进目录路径）。
    // id 限定 kebab-case（与市场/AI 工具注册共用同一套身份约束），category
    // 只允许「已存在分类或回退到其他工具」——两者都拒绝 `../`、绝对路径段、
    // 分隔符注入，用户不可能通过安装一个 zip 把文件写到 Tools 根之外。
    let plugin_id = validate_plugin_id(&meta.id)?;
    let cat = if meta.category.trim().is_empty() {
        "其他工具".to_string()
    } else {
        validate_plugin_category(&meta.category, tools_root)?
    };
    let cat_dir = tools_root.join(&cat);
    let plugin_dir = cat_dir.join(&plugin_id);
    if plugin_dir.exists() {
        return Err(ToolbeltError::BadArgs(format!(
            "插件「{}」已存在于 {}，先卸载再重装（卸载会移入回收站，可恢复）",
            plugin_id,
            plugin_dir.display()
        )));
    }
    std::fs::create_dir_all(&plugin_dir)
        .map_err(|e| ToolbeltError::BadArgs(format!("创建插件目录失败: {e}")))?;

    // 2. 解压（跳过 tool.plugin.json 本身——已单独解析）。任一文件写入失败
    // 立即整体回滚：删掉刚建的插件目录，不给用户留半个无法启用的插件。
    let extract_err: Option<ToolbeltError> = {
        let mut err = None;
        for i in 0..archive.len() {
            let mut f = match archive.by_index(i) {
                Ok(f) => f,
                Err(e) => {
                    err = Some(ToolbeltError::BadArgs(e.to_string()));
                    break;
                }
            };
            let Some(rel) = f
                .name()
                .strip_prefix(&base_dir)
                .map(|s| s.trim_start_matches('/'))
            else {
                continue;
            };
            if rel.is_empty() || rel == "tool.plugin.json" {
                continue;
            }
            // 路径穿越防护
            let norm = rel.replace('\\', "/");
            if norm.split('/').any(|seg| seg == "..")
                || norm.starts_with('/')
                || (norm.len() >= 2 && norm.as_bytes()[1] == b':')
            {
                continue;
            }
            let dest = plugin_dir.join(norm);
            if f.is_dir() {
                std::fs::create_dir_all(&dest).ok();
            } else {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                let mut out = match std::fs::File::create(&dest) {
                    Ok(o) => o,
                    Err(e) => {
                        err = Some(ToolbeltError::BadArgs(format!(
                            "写入 {} 失败: {e}",
                            dest.display()
                        )));
                        break;
                    }
                };
                if let Err(e) = std::io::copy(&mut f, &mut out) {
                    err = Some(ToolbeltError::BadArgs(format!(
                        "解压 {} 失败: {e}",
                        dest.display()
                    )));
                    break;
                }
            }
        }
        err
    };
    if let Some(e) = extract_err {
        let _ = std::fs::remove_dir_all(&plugin_dir);
        return Err(e);
    }

    Ok(plugin_dir
        .strip_prefix(tools_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default())
}

/// 插件 id 必须是 kebab-case（小写字母/数字开头，可含 `-`），与市场/AI 工具
/// 注册共用同一身份约束。除格式外天然排除 `..`、`/`、`\`、空格等目录越界字符。
fn validate_plugin_id(id: &str) -> Result<String, ToolbeltError> {
    let t = id.trim();
    let valid = !t.is_empty()
        && t.len() <= 64
        && t.as_bytes()[0].is_ascii_lowercase()
        && t.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid {
        return Err(ToolbeltError::BadArgs(format!(
            "插件 id「{id}」非法：只允许小写字母/数字/连字符（kebab-case，1-64 位）"
        )));
    }
    Ok(t.to_string())
}

/// 把任意工具/插件「显示名」净化成稳定的 kebab-case 插件 id：
/// 小写化 → 非字母数字字符折叠为 `-` → 去首尾/重复 `-` → 前缀补 `t`（数字开头），
/// 保证结果一定通过 [`validate_plugin_id`]（市场索引/AI 工具注册的持久化身份）。
/// 空输入返回 Err。
pub fn plugin_id_from_name(name: &str) -> Result<String, ToolbeltError> {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = false;
    for b in name.to_ascii_lowercase().bytes() {
        if b.is_ascii_lowercase() || b.is_ascii_digit() {
            out.push(b as char);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        return Err(ToolbeltError::BadArgs(format!(
            "无法从名称「{name}」生成插件 id（没有可用字符）"
        )));
    }
    // 数字开头非法：补一个稳定前缀
    let mut final_id = out;
    if final_id.as_bytes()[0].is_ascii_digit() {
        final_id.insert(0, 't');
    }
    if final_id.len() > 64 {
        final_id.truncate(64);
        while final_id.ends_with('-') {
            final_id.pop();
        }
    }
    Ok(final_id)
}

/// 插件分类必须落到 Tools 根下已存在的分类目录（'' 映射已在上层处理）。
/// 既防 `../`、绝对路径注入，也避免「安装即新建一个误拼的分类目录」。
fn validate_plugin_category(cat: &str, tools_root: &Path) -> Result<String, ToolbeltError> {
    let t = cat.trim();
    let bad_seg = t
        .split(['/', '\\'])
        .any(|seg| seg.is_empty() || seg == "." || seg == ".." || seg.contains(':'));
    if t.is_empty() || t.len() > 64 || bad_seg || t.contains('\0') {
        return Err(ToolbeltError::BadArgs(format!(
            "插件分类「{cat}」非法：请用 Tools 下已有的分类目录名"
        )));
    }
    let cand = tools_root.join(t);
    if !cand.is_dir() {
        return Err(ToolbeltError::BadArgs(format!(
            "插件分类「{cat}」不存在于 Tools 根（可用分类见工具墙左侧导航）"
        )));
    }
    Ok(t.to_string())
}

/// 把已安装插件目录（根含 `tool.plugin.json`）打包成可分发的 zip，返回
/// 生成的 zip 路径。zip 内结构与 [`install_plugin_zip`] 一致：
/// `tool.plugin.json` 在 zip 根，其余文件原样平铺——这样导出→导入是
/// 完美的 round-trip，别人拿到 zip 直接装就是一样的插件。
///
/// 目标文件已存在时覆盖（重新导出同名包是常见操作）。安全约束：
/// - 只允许打包带 `tool.plugin.json` 的插件目录（复用 catalog 的 id 定位）
/// - 跟随符号链接写入的是目标文件内容，防止把别的目录链进来
pub fn export_plugin_zip(plugin_dir: &Path, out_zip: &Path) -> Result<(), ToolbeltError> {
    let manifest_path = plugin_dir.join("tool.plugin.json");
    if !manifest_path.is_file() {
        return Err(ToolbeltError::BadArgs(format!(
            "「{}」不是插件目录（缺 tool.plugin.json）",
            plugin_dir.display()
        )));
    }
    let meta = std::fs::read_to_string(&manifest_path)
        .map_err(|e| ToolbeltError::BadArgs(format!("读取插件清单失败: {e}")))?
        .trim()
        .to_string();
    if let Some(parent) = out_zip.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let file = std::fs::File::create(out_zip)
        .map_err(|e| ToolbeltError::BadArgs(format!("创建导出包失败: {e}")))?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    // tool.plugin.json 放 zip 根：install 端「单层子目录/根」两处扫描都能命中。
    zw.start_file("tool.plugin.json", opts)
        .map_err(|e| ToolbeltError::BadArgs(format!("写入插件清单失败: {e}")))?;
    zw.write_all(meta.as_bytes())
        .map_err(|e| ToolbeltError::BadArgs(format!("写入插件清单失败: {e}")))?;

    let mut stack = vec![plugin_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            let Ok(meta) = ent.metadata() else { continue };
            if meta.is_dir() {
                stack.push(p);
                continue;
            }
            if !meta.is_file() {
                continue; // 符号链接等：只打普通文件，避免包进意外目标
            }
            // zip 内相对路径 = 相对插件目录（正斜杠），tool.plugin.json 已单独写。
            let rel = p
                .strip_prefix(plugin_dir)
                .map_err(|_| ToolbeltError::BadArgs("路径越界".into()))?;
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if rel_str.is_empty() || rel_str == "tool.plugin.json" {
                continue;
            }
            zw.start_file(&rel_str, opts)
                .map_err(|e| ToolbeltError::BadArgs(format!("写入 {} 失败: {e}", rel_str)))?;
            let Ok(mut f) = std::fs::File::open(&p) else {
                continue;
            };
            let _ = std::io::copy(&mut f, &mut zw);
        }
    }
    zw.finish()
        .map_err(|e| ToolbeltError::BadArgs(format!("封包失败: {e}")))?;
    Ok(())
}

/// 扫描本机固定盘，定位「图吧工具箱」绿色版的 tools 目录。
/// 图吧绿色版通常在盘根放一个「图吧工具箱20xx」文件夹，内含 tools 分类目录。
/// 命中多个时取目录名里版本号最大（最新）的一份，方便用户升级后自动切到新版。
fn find_tuba_green_tools() -> Option<PathBuf> {
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

// ── 参数与命令组装 ──────────────────────────────────────────────────

/// Windows 风格切词：双引号内保留空格，支持 `\"` 转义。用于 AI 传原始
/// 命令行串的场景（前端直接用 Vec<String> 则不经过这里）。
pub fn parse_args(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => in_quote = !in_quote,
            '\\' if chars.peek() == Some(&'"') => {
                chars.next();
                cur.push('"');
            }
            c if c.is_whitespace() && !in_quote => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() || in_quote {
        out.push(cur);
    }
    out
}

/// 读 exe 同目录的 toolbelt.toml（不存在/解析失败 = 无覆盖，只告警）。
fn load_override(exe_dir: &Path) -> Option<ToolOverride> {
    let p = exe_dir.join("toolbelt.toml");
    let text = std::fs::read_to_string(p.clone()).ok()?;
    match toml::from_str::<ToolOverride>(&text) {
        Ok(o) => Some(o),
        Err(e) => {
            tracing_warn(&format!("toolbelt.toml 解析失败 {:?}: {}", p, e));
            None
        }
    }
}

// tauri 侧有 tracing，但这个 crate 不拉依赖——出错只留 stderr，绝不 panic。
fn tracing_warn(msg: &str) {
    eprintln!("[diskpilot-toolbelt] {}", msg);
}

/// **核心**：把「工具名 + 实参」解析成可执行命令（丢失重建版，语义对齐
/// tubatools 的 run_cli_tool + toolbelt.toml 覆盖机制）。
///
/// - 工具名不区分大小写，支持部分匹配（"wiztree" / "autorunsc" 都能命中）
/// - `args` 为实参列表；为空时回退到该目录 toolbelt.toml 的固定默认 `args`
/// - `timeout_secs` 钳制在 5..3600；省略用工具默认（烤机类 1800）
/// - `cwd` 固定为 exe 目录：DiskInfo.txt / cli_done.txt / 日志都落在那里
pub fn toolbelt_command_for(
    name: &str,
    args: &[String],
    tools_root: &Path,
    timeout_secs: Option<u64>,
) -> Result<ResolvedCommand, ToolbeltError> {
    let tool = find(name).ok_or_else(|| ToolbeltError::UnknownTool {
        name: name.to_string(),
        available: catalog_cache()
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(" / "),
    })?;
    let rel = tool
        .exe_rel
        .as_ref()
        .ok_or_else(|| ToolbeltError::NoExePath {
            name: tool.name.clone(),
        })?;

    // 防目录穿越：文档内的相对路径本身可信，但 overrides 场景下做最严校验。
    let exe = tools_root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let exe = exe
        .canonicalize()
        .map_err(|_| ToolbeltError::ExeMissing { path: exe })?;
    // Windows canonicalize 返回 `\\?\` verbatim 前缀路径，std::process::Command 直接
    // spawn 它报 os error 123（语法不正确）→ 剥掉前缀改用普通盘符路径（工具目录在
    // 盘根深处常规长度内，无需超长路径支持）。
    #[cfg(windows)]
    let exe = {
        let s = exe.to_string_lossy();
        if s.starts_with("\\\\?\\") {
            PathBuf::from(s.trim_start_matches("\\\\?\\"))
        } else {
            exe
        }
    };
    if !exe.is_file() {
        return Err(ToolbeltError::ExeMissing { path: exe.clone() });
    }

    let ov = load_override(exe.parent().unwrap_or(tools_root));
    let risk = ov
        .as_ref()
        .and_then(|o| o.risk.as_deref())
        .and_then(Risk::parse)
        .unwrap_or(tool.risk);
    let args = if !args.is_empty() {
        args.to_vec()
    } else {
        ov.as_ref().map(|o| o.args.clone()).unwrap_or_default()
    };
    let timeout = timeout_secs
        .or_else(|| ov.as_ref().and_then(|o| o.timeout_secs))
        .unwrap_or(tool.timeout_secs);
    let timeout = timeout.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS);

    let mut usage = tool.detail.clone();
    if let Some(u) = ov.as_ref().and_then(|o| o.usage.clone()) {
        usage.push_str("\n\n## 本地补充说明（toolbelt.toml）\n");
        usage.push_str(&u);
    }

    Ok(ResolvedCommand {
        tool: tool.name.clone(),
        category: tool.category.clone(),
        cwd: exe
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| tools_root.to_path_buf()),
        program: exe,
        args,
        risk,
        timeout_secs: timeout,
        usage,
    })
}

// ── 执行 ────────────────────────────────────────────────────────────

fn decode_bytes(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        #[cfg(windows)]
        Err(_) => {
            // 控制台中文工具（urwtest 等）输出 GBK，UTF-8 失败则按 GBK 解。
            let (cow, _, _) = encoding_rs::GBK.decode(raw);
            cow.into_owned()
        }
        #[cfg(not(windows))]
        Err(_) => String::from_utf8_lossy(raw).into_owned(),
    }
}

fn read_capped(mut r: impl Read, cap: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match r.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < cap {
                    let keep = (cap - buf.len()).min(n);
                    buf.extend_from_slice(&chunk[..keep]);
                }
            }
            Err(_) => break,
        }
    }
    buf
}

/// 执行已解析命令：捕获 stdout/stderr（各上限 1 MiB），超时强杀。
/// 控制台程序在 Windows 下带 CREATE_NO_WINDOW，避免闪黑框。
pub fn run(cmd: &ResolvedCommand) -> RunOutcome {
    let timeout = Duration::from_secs(cmd.timeout_secs);
    let mut command = Command::new(&cmd.program);
    command
        .args(&cmd.args)
        .current_dir(&cmd.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            // ERROR_ELEVATION_REQUIRED：目标工具声明必须管理员运行。
            // CLI 执行要捕获 stdout，无法用 runas 提权窗口，只能如实告知。
            let detail = if e.raw_os_error() == Some(740) {
                format!(
                    "{}（os error 740）——该工具要求管理员权限，命令行方式无法捕获其输出；请改在工具墙双击启动（会弹出 UAC 提权）",
                    cmd.program.display()
                )
            } else {
                format!("{}（{}）", cmd.program.display(), e)
            };
            return RunOutcome {
                exit_code: None,
                timed_out: false,
                stdout: String::new(),
                stderr: format!("启动失败：{detail}"),
            };
        }
    };

    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let (err_tx, err_rx) = mpsc::channel::<Vec<u8>>();
    if let Some(so) = child.stdout.take() {
        let tx = out_tx;
        thread::spawn(move || {
            let _ = tx.send(read_capped(so, 1 << 20));
        });
    }
    if let Some(se) = child.stderr.take() {
        let tx = err_tx;
        thread::spawn(move || {
            let _ = tx.send(read_capped(se, 1 << 20));
        });
    }

    let started = Instant::now();
    let (status, timed_out) = wait_with_timeout(&mut child, timeout, started);
    let stdout = out_rx
        .recv()
        .ok()
        .map(|v| decode_bytes(&v))
        .unwrap_or_default();
    let stderr = err_rx
        .recv()
        .ok()
        .map(|v| decode_bytes(&v))
        .unwrap_or_default();
    RunOutcome {
        exit_code: status.and_then(|s| s.code()),
        timed_out,
        stdout,
        stderr,
    }
}

fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
    started: Instant,
) -> (Option<std::process::ExitStatus>, bool) {
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return (Some(st), false),
            Ok(None) => {}
            Err(_) => return (None, false),
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return (None, true);
        }
        thread::sleep(Duration::from_millis(80));
    }
}

/// 前端一次性拉取的完整状态：Tools 根 + 每个工具是否就位。
#[derive(Serialize)]
pub struct ToolbeltStatus {
    pub tools_root: Option<String>,
    pub tools: Vec<ToolSpec>,
}

#[derive(Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub category: String,
    pub description: String,
    pub exe_rel: Option<String>,
    pub risk: Risk,
    pub timeout_secs: u64,
    pub installed: bool,
}

pub fn status(explicit_root: Option<&Path>) -> ToolbeltStatus {
    let root = match find_tools_root(explicit_root) {
        Ok(r) => Some(r),
        Err(e) => {
            tracing_warn(&e.to_string());
            None
        }
    };
    let tools = catalog_cache()
        .iter()
        .map(|t| ToolSpec {
            name: t.name.clone(),
            category: t.category.clone(),
            description: t.description.clone(),
            exe_rel: t.exe_rel.clone(),
            risk: t.risk,
            timeout_secs: t.timeout_secs,
            installed: root
                .as_ref()
                .zip(t.exe_rel.as_deref())
                .map(|(r, rel)| {
                    r.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
                        .is_file()
                })
                .unwrap_or(false),
        })
        .collect();
    ToolbeltStatus {
        tools_root: root.map(|p| p.display().to_string()),
        tools,
    }
}

/// 按分类聚合的索引（保持文档出现顺序，前端直接渲染分组）。
pub fn grouped_index() -> Vec<(String, Vec<CliTool>)> {
    let mut order: Vec<String> = Vec::new();
    let mut map: BTreeMap<String, Vec<CliTool>> = BTreeMap::new();
    for t in catalog_cache() {
        map.entry(t.category.clone())
            .and_modify(|v| v.push(t.clone()))
            .or_insert_with(|| {
                order.push(t.category.clone());
                vec![t.clone()]
            });
    }
    order
        .into_iter()
        .map(|k| (k.clone(), map.remove(&k).unwrap_or_default()))
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════
// 缝口 A：全目录扫描（对齐 tubatools ToolCatalog）
//
// 上边的 17 个 CLI 工具是「可命令行化」白名单，由嵌入式文档驱动；而磁盘上
// Tools/ 根还躺着大量只能双击打开的图形工具（MSI / Dism++ / 烤机 / 跑分…）。
// catalog_tree() 遍历整个 Tools/ 根，为主面板提供完整工具墙：
//   - 分类子目录 → 工具目录（去除仅架构后缀不同的重复目录）
//   - 主启动文件判定：精确名 → 去架构后缀 → 兜底任意可执行
//   - link.json 跨分类软链 + builtin 占位
//   - Metadata/tools.json 叠简介/标签
// ═══════════════════════════════════════════════════════════════════════

/// 可双击启动的文件扩展名（同 tubatools LaunchableExtensions）。
const LAUNCHABLE_EXTS: &[&str] = &[".exe", ".bat", ".cmd", ".lnk", ".msc", ".ps1", ".vbs"];

/// 目录名/文件名中的架构后缀（同 C# ArchSuffixes，补 `_x32` 且按**长度降序**）。
/// C# 里 `cpuz_x32` 会被裸 `32` 抢先截成 `cpuz_x`、`cpuz_x64` 截成 `cpuz_`，
/// 归档比较全部落空——这里让 `_x64`/`_x32`/`_ARM64` 这类更具体的后缀先命中，
/// `cpuz_x32` → `cpuz`、`cpuz_x64` → `cpuz`，都能对上目录名 `CPUZ`。
const ARCH_SUFFIXES: &[&str] = &[
    "_Win64", "_Win32", "_ARM64", "ARM64", "_x64", "_x86", "_x32", "_64", "_32", "x64", "x86",
    "w64", "w32", "64", "32",
];

/// 宿主架构优先级（ARM64 > x64 > x86，与 C# PreferredArchPriority 对齐）。
fn preferred_arch_priority() -> &'static [&'static str] {
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

/// `tool.plugin.json`（工具目录根）的插件元数据。与 [`ToolManifest`] 同构但更轻：
/// 专注「插件身份」——id/版本/作者/描述/入口/权限，供工具墙详情卡展示插件徽标、
/// 权限声明，以及后续插件市场（安装/卸载/更新）使用。
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
    /// zip 文件按路径字典序拼接」计算（规范见 [`signature`] 模块注释）。
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

fn is_launchable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            LAUNCHABLE_EXTS
                .iter()
                .any(|x| e.eq_ignore_ascii_case(&x[1..]))
        })
        .unwrap_or(false)
}

// ─══════════════════════════════════════════════════════════════════════
// 抽取 exe 真实图标（Windows）：ExtractIconExW 取 HICON → GetIconInfo 拿
// 颜色位图 → GetDIBits 读 BGRA 像素 → 转 RGBA → PNG 编码 → base64 data URI。
// 非 Windows / 非 exe / 无图标资源时返回 None，前端回退 Lucide 图标。
// ═══════════════════════════════════════════════════════════════════════

/// 小型 base64 编码（避免再引一个 crate）。
fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// 判断某启动文件是否为「纯推广跳转」：只做 `start <URL>` 打开网页、
/// 且 URL 命中推广模式（图吧 `tbtool.cn` 推广/更新/皮肤页、`inviteCode`
/// 返利链）——不是可用的工具软件。加速器/游戏官网下载页（fnjiasu 等）、
/// 在线测试工具（testufo、pcbox、bmcx 等）是真实功能入口，不算推广。
///
/// 识别不依赖文件名，只读文件内容，避免误伤名字带「加速器/领卡」的真工具：
/// 一个 .bat 内容是多行脚本（有 @echo/if/%~dp0 等）就直接放行。
fn is_promotion_bat(launchable: &Path) -> bool {
    let Some(ext) = launchable.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    if !ext.eq_ignore_ascii_case("bat") && !ext.eq_ignore_ascii_case("cmd") {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(launchable) else {
        return false;
    };
    let body = content.trim().to_ascii_lowercase();
    // 多行脚本（含可执行逻辑）不是纯跳转，放行。
    if body.lines().count() > 2 {
        return false;
    }
    let url: String = body
        .lines()
        .find_map(|l| {
            let t = l.trim_start_matches('@').trim();
            t.strip_prefix("start")
                .or_else(|| t.strip_prefix("open"))
                .map(|r| r.trim().to_string())
        })
        .unwrap_or_default();
    if url.is_empty() {
        return false;
    }
    let is_tbtool = url.contains("tbtool.cn"); // 图吧官方域名：/links/* 推广页 + /Version/ 更新页 + /skin/ 皮肤页
    let is_invite = url.contains("invitecode") || url.contains("invite=");
    let is_promo_host = ["wanjiadongli.com", "fxzj.com", "go.funyouyou"]
        .iter()
        .any(|h| url.contains(h));
    is_tbtool || is_invite || is_promo_host
}

/// 抽取某 exe 的真实图标为 PNG data URI。失败返回 None。
fn exe_icon_data_uri(exe: &Path) -> Option<String> {
    let ext = exe
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if ext.as_deref() != Some("exe") {
        return None;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Graphics::Gdi::{
            CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
            BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
        };
        use windows_sys::Win32::UI::Shell::ExtractIconExW;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DestroyIcon, GetIconInfo, HICON, ICONINFO,
        };

        let wide: Vec<u16> = exe
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut h_large: HICON = std::ptr::null_mut();
        let mut h_small: HICON = std::ptr::null_mut();
        let n = unsafe { ExtractIconExW(wide.as_ptr(), 0, &mut h_large, &mut h_small, 1) };
        let _ = n;
        if h_large.is_null() {
            if !h_small.is_null() {
                unsafe { DestroyIcon(h_small) };
            }
            return None;
        }

        let result = (|| -> Option<Vec<u8>> {
            let mut ii: ICONINFO = unsafe { std::mem::zeroed() };
            if unsafe { GetIconInfo(h_large, &mut ii) } == 0 {
                return None;
            }
            let hbm: HBITMAP = if ii.hbmColor.is_null() {
                ii.hbmMask
            } else {
                ii.hbmColor
            };
            if hbm.is_null() {
                return None;
            }
            let mut bm: BITMAP = unsafe { std::mem::zeroed() };
            unsafe {
                GetObjectW(
                    hbm as HGDIOBJ,
                    std::mem::size_of::<BITMAP>() as i32,
                    &mut bm as *mut _ as *mut core::ffi::c_void,
                );
            }
            let w = bm.bmWidth;
            let h = bm.bmHeight;
            if w <= 0 || h <= 0 {
                return None;
            }
            let stride = (bm.bmWidthBytes.max(0) as usize).max((w as usize) * 4);
            let mut buf = vec![0u8; stride * (h as usize)];
            let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -h; // 负值 = 自上而下的像素行
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let hdc: HDC = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
            if hdc.is_null() {
                return None;
            }
            let got = unsafe {
                GetDIBits(
                    hdc,
                    hbm,
                    0,
                    h as u32,
                    buf.as_mut_ptr() as *mut core::ffi::c_void,
                    &mut bmi as *mut BITMAPINFO,
                    DIB_RGB_COLORS,
                )
            };
            unsafe { DeleteDC(hdc) };
            if got == 0 {
                return None;
            }

            // ── 透明度来源：mask 位图（AND mask）──────────────────────
            // 老图标（24bpp）转 32bpp 后 alpha 通道是垃圾/全 0 → 整张透明
            // 不可见（前端 <img> 不触发 onError，fallback 失效）。
            // 现代图标（32bpp 真 alpha）颜色位图自带渐变 alpha，直接采用；
            // 老图标透明形状可靠存放在 hbmMask（1 = 透明，0 = 不透明），
            // 用 mask 二值化重建 alpha。真 alpha 图标的抗锯齿边缘会变硬边，
            // 但形状清晰可见，远胜透明空白。若 mask 读不出（异常图标），
            // 退化为整帧不透明。
            // 真 alpha 判定：颜色位图里出现「中间 alpha」（渐变必有）。
            let mut color_has_real_alpha = false;
            if !ii.hbmColor.is_null() {
                for row in 0..(h as usize) {
                    let src = &buf[row * stride..(row * stride + (w as usize) * 4).min(buf.len())];
                    for c in src.chunks(4) {
                        let a = c[3];
                        if a != 0 && a != 255 {
                            color_has_real_alpha = true;
                            break;
                        }
                    }
                    if color_has_real_alpha {
                        break;
                    }
                }
            }
            let mut mask_bits: Vec<u8> = Vec::new(); // 每像素 1 bit（1=透明）
            if !color_has_real_alpha {
                let mut mb: BITMAP = unsafe { std::mem::zeroed() };
                if !ii.hbmMask.is_null()
                    && unsafe {
                        GetObjectW(
                            ii.hbmMask as HGDIOBJ,
                            std::mem::size_of::<BITMAP>() as i32,
                            &mut mb as *mut _ as *mut core::ffi::c_void,
                        )
                    } != 0
                {
                    let mw = mb.bmWidth;
                    let mh = mb.bmHeight;
                    // monochrome mask 高 = 2h（图标区 + AND/OR 压平区），只取上 h 行
                    if mw >= w && mh >= h {
                        // 关键：GetDIBits 对 1bpp DIB 每行按 32-bit 对齐写入
                        // （stride = ((mw+31)/32)*4），行字节只由我传入的
                        // BITMAPINFO 决定，**与源位图 bmWidthBytes 无关**——
                        // 老位图该字段是未对齐的 div_ceil(mw,8)（如 48px 图标
                        // bmWidthBytes=6，而 GetDIBits 实际写 8），若按它分配
                        // 缓冲区或做行步进，会每行越界写 → STATUS_HEAP_CORRUPTION
                        // 崩溃，读行也会错位。所以分配与读取统一用 stride32。
                        let stride32 = (mw as usize).div_ceil(32) * 4;
                        let mut mbuf = vec![0u8; stride32 * (mh as usize)];
                        let mut mbi: BITMAPINFO = unsafe { std::mem::zeroed() };
                        mbi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                        mbi.bmiHeader.biWidth = mw;
                        mbi.bmiHeader.biHeight = -mh; // 负值 = 自上而下
                        mbi.bmiHeader.biPlanes = 1;
                        mbi.bmiHeader.biBitCount = 1;
                        mbi.bmiHeader.biCompression = BI_RGB;
                        let hdc2: HDC = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
                        if !hdc2.is_null() {
                            let mgot = unsafe {
                                GetDIBits(
                                    hdc2,
                                    ii.hbmMask as HBITMAP,
                                    0,
                                    mh as u32,
                                    mbuf.as_mut_ptr() as *mut core::ffi::c_void,
                                    &mut mbi as *mut BITMAPINFO,
                                    DIB_RGB_COLORS,
                                )
                            };
                            unsafe { DeleteDC(hdc2) };
                            if mgot != 0 && mb.bmBitsPixel <= 1 {
                                // 真 monochrome mask：每行按 stride32（与 GetDIBits 写入一致），前 w 位有效
                                for row in 0..(h as usize) {
                                    let r = &mbuf[row * stride32
                                        ..(row * stride32 + stride32).min(mbuf.len())];
                                    for x in 0..(w as usize) {
                                        let byte = if x / 8 < r.len() { r[x / 8] } else { 0 };
                                        let bit = (byte >> (7 - (x % 8))) & 1;
                                        mask_bits.push(bit);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ── BGRA → RGBA ──────────────────────────────────────────
            let px = (w as usize) * (h as usize);
            let mut rgba = Vec::with_capacity(px * 4);
            if color_has_real_alpha {
                // 真 alpha 图标：直接用颜色位图的 alpha（保留渐变边缘）
                for row in 0..(h as usize) {
                    let src = &buf[row * stride..(row * stride + (w as usize) * 4).min(buf.len())];
                    for c in src.chunks(4) {
                        rgba.push(c[2]);
                        rgba.push(c[1]);
                        rgba.push(c[0]);
                        rgba.push(c[3]);
                    }
                }
            } else if mask_bits.len() == px {
                // mask 二值化 alpha：bit=1 → 透明
                for row in 0..(h as usize) {
                    let src = &buf[row * stride..(row * stride + (w as usize) * 4).min(buf.len())];
                    for (x, c) in src.chunks(4).enumerate() {
                        let a = if mask_bits[row * (w as usize) + x] == 1 {
                            0
                        } else {
                            255
                        };
                        rgba.push(c[2]);
                        rgba.push(c[1]);
                        rgba.push(c[0]);
                        rgba.push(a);
                    }
                }
            } else {
                // 无 mask 可读：整帧不透明（至少不再空白）
                for row in 0..(h as usize) {
                    let src = &buf[row * stride..(row * stride + (w as usize) * 4).min(buf.len())];
                    for c in src.chunks(4) {
                        rgba.push(c[2]);
                        rgba.push(c[1]);
                        rgba.push(c[0]);
                        rgba.push(255);
                    }
                }
            }

            // 位图句柄用完即释放（上面已用毕）
            unsafe {
                DeleteObject(ii.hbmColor as HGDIOBJ);
                DeleteObject(ii.hbmMask as HGDIOBJ);
            }
            let mut out: Vec<u8> = Vec::new();
            {
                let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                let mut writer = enc.write_header().ok()?;
                writer.write_image_data(&rgba).ok()?;
            }
            Some(out)
        })();

        unsafe {
            DestroyIcon(h_large);
            if !h_small.is_null() {
                DestroyIcon(h_small);
            }
        }
        result.map(|bytes| format!("data:image/png;base64,{}", base64_encode(&bytes)))
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        None
    }
}

/// 去掉文件/目录名尾部的架构后缀（同 C# StripArchSuffix）。
/// 注意：不能按字节切片——中文工具名是多字节 UTF-8，切片可能落在字符中间
/// panic。转小写后直接用 `ends_with` 比较（ASCII 后缀大小写不敏感）。
fn strip_arch_suffix(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    for suffix in ARCH_SUFFIXES {
        let sfx = suffix.to_ascii_lowercase();
        if lower.len() > sfx.len() && lower.ends_with(&sfx) {
            return name[..name.len() - suffix.len()].to_string();
        }
    }
    name.to_string()
}

/// 去架构后缀后再去掉残留的 `_`（如 `cpuz_x64` → `cpuz`，`WizTree64` → `WizTree`），
/// 用于与目录名/彼此比较：C# 里 `StripArchSuffix("cpuz_x64")` 得 `cpuz_`，与目录
/// `CPUZ` 比不相等，导致主文件判定/变体收集落空——这里把尾 `_` 也剥掉再比。
fn strip_arch_suffix_trim(name: &str) -> String {
    strip_arch_suffix(name).trim_end_matches('_').to_string()
}

/// 从文件名末尾识别架构（同 C# DetectArch + FormatArchDisplay）。
fn detect_arch(file_stem: &str) -> Option<&'static str> {
    let arm = ["ARM64", "_ARM64", "arm64", "_arm64"];
    let x64 = ["x64", "_x64", "64", "_64", "w64", "_Win64"];
    let x86 = ["x86", "_x86", "32", "_32", "w32", "_Win32"];
    for p in arm {
        if file_stem.ends_with(p) {
            return Some("ARM64");
        }
    }
    for p in x64 {
        if file_stem.ends_with(p) {
            return Some("x64");
        }
    }
    for p in x86 {
        if file_stem.ends_with(p) {
            return Some("x86");
        }
    }
    None
}

/// 已知多架构工具的文件名模式（跨目录关联，同 C# KnownMultiArchTools）。
const KNOWN_MULTI_ARCH: &[(&str, &[&str])] = &[
    (
        "cpuz",
        &[
            "cpuz_x64.exe",
            "cpuz_x32.exe",
            "cpuz_arm64.exe",
            "cpuz64.exe",
            "cpuz32.exe",
        ],
    ),
    (
        "hwinfo",
        &[
            "HWiNFO64.exe",
            "HWiNFO32.exe",
            "HWiNFO_ARM64.exe",
            "HWiNFO.exe",
        ],
    ),
    (
        "hwmonitor",
        &[
            "HWMonitor_x64.exe",
            "HWMonitor_x32.exe",
            "hwmonitor_arm64.exe",
            "HWMonitor.exe",
        ],
    ),
    (
        "dism++",
        &[
            "Dism++x64.exe",
            "Dism++x86.exe",
            "Dism++ARM64.exe",
            "Dism++.exe",
        ],
    ),
    (
        "coretemp",
        &["Core Temp x64.exe", "Core Temp x86.exe", "Core Temp.exe"],
    ),
    (
        "crystaldiskinfo",
        &[
            "DiskInfo64S.exe",
            "DiskInfo32S.exe",
            "DiskInfo.exe",
            "DiskInfo64.exe",
            "DiskInfo32.exe",
        ],
    ),
    (
        "bootice",
        &["BOOTICEx64.exe", "BOOTICEx86.exe", "BOOTICE.exe"],
    ),
    (
        "bluescreenview",
        &[
            "BlueScreenViewx64.exe",
            "BlueScreenViewx86.exe",
            "BlueScreenView.exe",
        ],
    ),
    ("aida64", &["aida64.exe", "aida64.exe.manifest"]),
    (
        "linx",
        &[
            "linpack_xeon64.exe",
            "linpack_xeon32.exe",
            "linpack_xeon.exe",
        ],
    ),
];

/// 收集一个工具目录内的全部可执行文件（递归；同 C# FindAllArchVariants 第一步）。
fn all_launchables(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                stack.push(p);
            } else if is_launchable(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// 主启动文件判定（同 C# FindPrimaryLaunchable，去掉 launchTarget 元数据分支，
/// 保留：精确名 → 去架构后缀 → 任意兜底）。`all` 已由调用方收集（catalog_tree 去重 IO）。
fn find_primary_launchable_with(
    tool_dir: &Path,
    dir_name: &str,
    all: &[PathBuf],
) -> Option<PathBuf> {
    if all.is_empty() {
        return None;
    }
    if all.len() == 1 {
        return all.first().cloned();
    }

    let direct: Vec<PathBuf> = std::fs::read_dir(tool_dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| is_launchable(p))
                .collect()
        })
        .unwrap_or_default();

    let stem_eq = |p: &Path, n: &str| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case(n))
            .unwrap_or(false)
    };
    let stem_stripped_eq = |p: &Path, n: &str| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| strip_arch_suffix_trim(s).eq_ignore_ascii_case(&strip_arch_suffix_trim(n)))
            .unwrap_or(false)
    };

    // 顶层精确名
    if let Some(m) = direct.iter().find(|p| stem_eq(p, dir_name)) {
        return Some(m.clone());
    }
    // 顶层去架构后缀
    let arch_cand: Vec<PathBuf> = direct
        .iter()
        .filter(|p| stem_stripped_eq(p, dir_name))
        .cloned()
        .collect();
    if !arch_cand.is_empty() {
        return Some(pick_preferred_arch(&arch_cand));
    }
    // 递归精确名
    if let Some(m) = all.iter().find(|p| stem_eq(p, dir_name)) {
        return Some(m.clone());
    }
    // 递归去架构后缀
    let arch_cand: Vec<PathBuf> = all
        .iter()
        .filter(|p| stem_stripped_eq(p, dir_name))
        .cloned()
        .collect();
    if !arch_cand.is_empty() {
        return Some(pick_preferred_arch(&arch_cand));
    }
    // 兜底
    if !direct.is_empty() {
        return Some(direct[0].clone());
    }
    all.first().cloned()
}

/// 从候选里按宿主架构优先级挑一个（同 C# PickPreferredArch）。
fn pick_preferred_arch(candidates: &[PathBuf]) -> PathBuf {
    for arch in preferred_arch_priority() {
        let patterns: &[&str] = match *arch {
            "ARM64" => &["ARM64", "_ARM64", "arm64", "_arm64"],
            "x64" => &["x64", "_x64", "64", "_64", "w64", "_Win64"],
            _ => &["x86", "_x86", "32", "_32", "w32", "_Win32"],
        };
        if let Some(m) = candidates.iter().find(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| patterns.iter().any(|pat| s.ends_with(pat)))
                .unwrap_or(false)
        }) {
            return m.clone();
        }
    }
    candidates[0].clone()
}

/// 同目录 + 跨目录（KNOWN_MULTI_ARCH）的架构变体收集（对齐 C# FindAllArchVariants）。
/// `all` 已由调用方收集（catalog_tree 去重 IO）。
fn find_arch_variants_with(
    tools_root: &Path,
    tool_dir: &Path,
    category_root: &Path,
    primary: &Path,
    dir_name: &str,
    all: &[PathBuf],
) -> Vec<ArchVariantOut> {
    let mut variants: Vec<ArchVariantOut> = Vec::new();
    let primary_ext = primary.extension().and_then(|e| e.to_str());

    // 1. 同目录内
    let dir_stripped = strip_arch_suffix_trim(dir_name);
    for f in all {
        if f == primary {
            continue;
        }
        if let Some(ext) = primary_ext {
            if !f
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case(ext))
                .unwrap_or(false)
            {
                continue;
            }
        }
        let Some(stem) = f.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(arch) = detect_arch(stem) else {
            continue;
        };
        let stripped = strip_arch_suffix_trim(stem);
        if !stripped.eq_ignore_ascii_case(&dir_stripped) && !stripped.eq_ignore_ascii_case(dir_name)
        {
            continue;
        }
        let file_rel = normalize_rel(tools_root, f);
        if !variants.iter().any(|v| v.file_rel == file_rel) {
            variants.push(ArchVariantOut {
                name: clean_name(&stripped),
                file_rel,
                arch: arch.to_string(),
            });
        }
    }

    // 2. 跨目录（已知多架构工具映射）
    let primary_file = primary.file_name().and_then(|s| s.to_str()).unwrap_or("");
    for (_, patterns) in KNOWN_MULTI_ARCH {
        if !patterns
            .iter()
            .any(|p| p.eq_ignore_ascii_case(primary_file))
        {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(category_root) else {
            break;
        };
        for ent in rd.flatten() {
            let other = ent.path();
            if other == *tool_dir || !other.is_dir() {
                continue;
            }
            let Some(other_name) = other.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !strip_arch_suffix_trim(other_name).eq_ignore_ascii_case(&dir_stripped) {
                continue;
            }
            for pattern in *patterns {
                let candidate = other.join(pattern);
                if candidate.is_file() && is_launchable(&candidate) {
                    let Some(stem) = candidate.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let Some(arch) = detect_arch(stem) else {
                        continue;
                    };
                    let file_rel = normalize_rel(tools_root, &candidate);
                    if !variants.iter().any(|v| v.file_rel == file_rel) {
                        variants.push(ArchVariantOut {
                            name: clean_name(&strip_arch_suffix(stem)),
                            file_rel,
                            arch: arch.to_string(),
                        });
                    }
                }
            }
        }
        break;
    }
    variants
}

/// `_x64`/`_x86`/`_arm64` → 空格分隔的展示名（同 C# CleanupName 的前四步）。
fn clean_name(stem: &str) -> String {
    let s = stem
        .replace("_x64", " x64")
        .replace("_x86", " x86")
        .replace("_ARM64", " ARM64")
        .replace("_arm64", " ARM64")
        .replace('_', " ");
    s.trim().to_string()
}

/// 相对 Tools 根的正斜杠路径。
fn normalize_rel(tools_root: &Path, p: &Path) -> String {
    p.strip_prefix(tools_root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 合并仅架构后缀不同的目录（同 C# MergeArchDirectories：保第一个，跳过后续同名 stripped）。
fn merge_arch_dirs(dir_names: &[String]) -> Vec<String> {
    let mut consumed = vec![false; dir_names.len()];
    let mut out = Vec::new();
    for (i, n) in dir_names.iter().enumerate() {
        if consumed[i] {
            continue;
        }
        let stripped_i = strip_arch_suffix_trim(n);
        out.push(n.clone());
        for (j, n2) in dir_names.iter().enumerate().skip(i + 1) {
            if consumed[j] {
                continue;
            }
            if strip_arch_suffix_trim(n2).eq_ignore_ascii_case(&stripped_i) {
                consumed[j] = true;
            }
        }
    }
    out
}

/// link.json 解析结果（对齐 C# LinkInfo）。
struct LinkInfo {
    /// "target" 相对 Tools 根的目标目录；builtin 软链为 None。
    target_rel: Option<String>,
    /// "builtin" 内置工具 id。
    builtin_id: Option<String>,
}

impl LinkInfo {
    fn is_builtin(&self) -> bool {
        self.builtin_id.is_some()
    }
}

/// 解析工具目录里的 link.json：目录必须「只有一个文件且无子目录」才视为软链
/// （同 C# TryResolveLink 的形态校验）。
fn try_resolve_link(tool_dir: &Path) -> Option<LinkInfo> {
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
struct JsonToolMeta {
    #[serde(rename = "match", default)]
    match_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(rename = "launchTarget", default)]
    launch_target: Option<String>,
    #[serde(default)]
    publisher: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(rename = "tutorialUrl", default)]
    tutorial_url: Option<String>,
    #[serde(rename = "downloadUrl", default)]
    download_url: Option<String>,
    #[serde(rename = "displayName", default)]
    display_name: Option<String>,
    /// 风险覆盖（tools.json 权威度高于默认，toolbelt.toml 仍可再盖）。
    #[serde(default)]
    risk: Option<String>,
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
fn load_tools_meta(tools_root: &Path) -> Vec<(String, JsonToolMeta)> {
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
fn find_meta<'a>(
    meta: &'a [(String, JsonToolMeta)],
    primary_rel: &str,
    dir_name: &str,
) -> Option<&'a JsonToolMeta> {
    meta.iter()
        .filter(|(m, _)| meta_matches(primary_rel, m) || meta_matches(dir_name, m))
        .max_by_key(|(m, _)| m.chars().count())
        .map(|(_, t)| t)
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
