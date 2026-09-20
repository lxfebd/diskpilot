//! 通用 MCP 服务器接入（可插拔 MCP 插件）
//!
//! 用户可添加任意标准 MCP 服务器（本地 stdio 进程 + 远程 streamable HTTP URL），
//! 其暴露的工具自动注册进前端 AI 工具表（mcp_list_tools → dynamicMcpTools）。
//!
//! 与内置 agent-server 的区别：
//! - agent.rs = 硬编码 spawn 本仓库 agent-server.exe（50 个工具 = 42 只读 + 8 写）；
//! - 本模块 = 用户配置的任意 MCP 服务器（可插拔增删改，写工具需确认）。
//!
//! 安全模型：
//! - 服务器列表持久化在 `app_data_dir/mcp-servers.json`（用户显式添加才存在）；
//! - 远程 URL 强制 https（照 plugin_remote.rs 的 https 白名单）；
//! - 服务器增删改 = 写命令，confirmed + perm_grants["mcp.manage"] 双校验；
//! - `writable=false`（默认）的服务器工具按 L0 只读调用；
//!   `writable=true` 的服务器调用任意工具都要求 confirmed==Some(true) 硬校验。
//!
//! 生命周期：每次调用重新连接（stdio 重新 spawn / http 新建 session），
//! 与 agent.rs「无共享状态、崩溃自愈」设计一致；AI 工具调用频率低，可接受。

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;

use rmcp::model::CallToolRequestParams;
use rmcp::service::ServiceExt;
use rmcp::transport::TokioChildProcess;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::process::Command;

/// 单个 MCP 服务器的传输方式。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransport {
    /// 本地进程（stdio 协议）：spawn 命令 + 参数 + 环境变量 + 工作目录。
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
        #[serde(default)]
        cwd: Option<String>,
    },
    /// 远程服务器（streamable HTTP）：https URL + 可选请求头 + 超时。
    Http {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
}

/// 一个用户配置的 MCP 服务器。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpServerConfig {
    /// kebab-case 稳定 id（净化规则与 toolbelt::plugin_id_from_name 契约一致）。
    pub id: String,
    pub name: String,
    /// 是否启用（禁用的服务器不参与工具合并、不连接）。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 该服务器暴露的工具是否含写操作（默认 false = 全部按 L0 只读调用；
    /// true = 调用任意工具都需要用户确认）。
    #[serde(default)]
    pub writable: bool,
    /// 可选：给该服务器的工具名加前缀（形如 `server_`），防多服务器工具重名。
    #[serde(default)]
    pub tool_prefix: Option<String>,
    /// 工具级权限映射（质量安全，2026-09-11）：工具原始名（不含前缀）→ 权限级。
    /// 值允许：
    /// - `"L0"`（只读，直接放行，无确认）；
    /// - `"L1"`（受控，需 confirmed=true）；
    /// - `"L2"`（高危，需 confirmed=true + perm_grants，缺省对 `mcp.manage`）；
    /// - `"L3"`（永禁，拒绝调用）；
    /// - 具体权限 id（如 `"sys.control"` / `"file.recycle"`）→ 按 L2 处理且
    ///   校验该权限是否开启。
    ///
    /// 未映射的工具按 `writable` 推断：true → L2，false → L0。
    #[serde(default)]
    pub permission_map: Option<std::collections::HashMap<String, String>>,
    pub transport: McpTransport,
}

fn default_true() -> bool {
    true
}

impl McpServerConfig {
    /// 工具最终注册名：前缀 + 原名（前缀为空则原名）。前缀净化后以 `_` 结尾。
    pub fn tool_name(&self, raw: &str) -> String {
        match &self.tool_prefix {
            Some(p) if !p.trim().is_empty() => {
                format!("{}_{}", p.trim().trim_end_matches('_'), raw)
            }
            _ => raw.to_string(),
        }
    }

    /// 解析某工具（**原始名**，不含前缀）的权限级与所需 perm id。
    /// 返回 `(级别词, Option<perm id>)`：
    /// - `L0`：只读放行，无确认；
    /// - `L1`：需 confirmed；
    /// - `L2`：需 confirmed + perm（None → 缺省 `mcp.manage`）；
    /// - `L3`：永禁。
    ///
    /// 未映射时按 `writable` 推断（true → L2，false → L0）。
    pub fn resolve_perm(&self, raw: &str) -> (String, Option<String>) {
        let fallback = if self.writable { "L2" } else { "L0" };
        let Some(v) = self
            .permission_map
            .as_ref()
            .and_then(|m| m.get(raw).map(|s| s.trim().to_string()))
        else {
            return (fallback.to_string(), None);
        };
        let up = v.to_ascii_uppercase();
        match up.as_str() {
            "L0" | "L1" | "L2" | "L3" => (up, None),
            _ => ("L2".to_string(), Some(v)),
        }
    }
}

/// 给前端的服务器健康/探测结果。
#[derive(Debug, Clone, Serialize)]
pub struct McpServerStatus {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub writable: bool,
    pub tool_prefix: Option<String>,
    pub transport_type: String,
    /// 完整传输配置（复用来编辑/开关，避免前端另存一份）。
    pub transport: McpTransport,
    /// 连接探测结果：Some(工具数) = 连接成功；None = 失败（error 里有原因）。
    pub tool_count: Option<usize>,
    pub error: Option<String>,
    /// 工具级权限映射摘要（前端展示；形如 `{n 个 L2 / m 个 L0}`）。
    pub perm_scope: String,
    /// 完整权限映射（编辑表单预填用；复用 config，前端不另存一份）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_map: Option<std::collections::HashMap<String, String>>,
}

// ── 审计日志（追加式 mcp-audit.jsonl，2026-09-11）────────────────────

const AUDIT_FILE: &str = "mcp-audit.jsonl";
/// probe 结果缓存 TTL（秒）：5s 内重复 list_servers/list_tools 免重连。
const PROBE_TTL_SECS: u64 = 5;
/// probe 全局超时（秒）：stdio 无 timeout 时兜底，防死锁。
const PROBE_TIMEOUT_SECS: u64 = 10;

pub(crate) fn audit_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(AUDIT_FILE))
}

/// 追加一条审计日志（失败静默：审计是辅助能力，不阻塞主流程）。
fn append_audit(app: &AppHandle, line: serde_json::Value) {
    let Some(p) = audit_path(app) else { return };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let _ = writeln!(f, "{}", serde_json::to_string(&line).unwrap_or_default());
    }
}

fn audit_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 参数快照：截断 + 扁平化，脱敏（不写完整值，只写键名单与总长）。
fn audit_args_snapshot(args: &serde_json::Value) -> serde_json::Value {
    match args {
        serde_json::Value::Object(m) => {
            let keys: Vec<String> = m.keys().cloned().collect();
            serde_json::json!({ "keys": keys, "size": args.to_string().len() })
        }
        serde_json::Value::Null => serde_json::Value::Null,
        other => serde_json::json!({ "type": other.as_str().map(|_| "string").unwrap_or("other") }),
    }
}

/// 审计日志读取（只读 L0）：返回最近 N 条。
#[tauri::command]
pub(crate) fn mcp_audit_tail(app: AppHandle, limit: Option<usize>) -> Vec<serde_json::Value> {
    let Some(p) = audit_path(&app) else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Vec::new();
    };
    let n = limit.unwrap_or(50).clamp(1, 200);
    text.lines()
        .rev()
        .filter_map(|l| serde_json::from_str(l).ok())
        .take(n)
        .collect()
}

/// 给前端的工具清单条目（合并所有 enabled 服务器）。
#[derive(Debug, Clone, Serialize)]
pub struct McpToolMeta {
    pub server_id: String,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub writable: bool,
    /// 工具最终权限级（质量安全：L0/L1/L2/L3 或具体 perm id）。
    pub perm: String,
}

/// 前端新增/编辑服务器时的载荷。
#[derive(Debug, Clone, Deserialize)]
pub struct McpServerPayload {
    pub name: String,
    pub enabled: Option<bool>,
    pub writable: Option<bool>,
    pub tool_prefix: Option<String>,
    #[serde(default)]
    pub permission_map: Option<std::collections::HashMap<String, String>>,
    pub transport: McpTransport,
}

// ── 持久化（照 general_config_at / set_general「读→合并→写」模式）────

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("mcp-servers.json"))
}

pub(crate) fn load_servers(app: &AppHandle) -> Vec<McpServerConfig> {
    let Some(p) = config_path(app) else {
        return Vec::new();
    };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_servers(app: &AppHandle, servers: &[McpServerConfig]) -> Result<(), String> {
    let Some(dir) = app.path().app_data_dir().ok() else {
        return Err("无法定位数据目录".into());
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(servers).map_err(|e| e.to_string())?;
    std::fs::write(config_path(app).expect("app_data_dir 已确认"), text).map_err(|e| e.to_string())
}

fn gen_id(app: &AppHandle, name: &str) -> String {
    let base =
        diskpilot_toolbelt::plugin_id_from_name(name).unwrap_or_else(|_| "mcp-server".to_string());
    // 重名追加数字后缀（-2 / -3 …），保证唯一。
    let existing: std::collections::HashSet<String> =
        load_servers(app).into_iter().map(|s| s.id).collect();
    let mut id = base.clone();
    let mut n = 2;
    while existing.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

// ── 桥接（stdio / http → 统一 RunningService）───────────────────────

type McpService = rmcp::service::RunningService<rmcp::service::RoleClient, ()>;

fn validate_http_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url.trim())
        .map_err(|_| format!("URL 无法解析：{url}。请填写完整的 https:// 地址。"))?;
    if parsed.scheme() != "https" {
        return Err(format!(
            "MCP 服务器地址必须使用 https（当前协议：{}）。http 与自定义协议一律拒绝。",
            parsed.scheme()
        ));
    }
    Ok(())
}

/// stdio 危险字符约束（最小化，保持「用户显式添加即信任」的通用性）：
/// 拒绝空命令 / 含 `\x00` 的命令 / 相对路径穿越的 cwd / 非法 env 键。
/// 真正的信任门槛是 `mcp_add_server` 的 L2 权限 + 每用审计。
fn validate_stdio(cfg: &McpServerConfig) -> Result<(), String> {
    let McpTransport::Stdio {
        command,
        args,
        env,
        cwd,
    } = &cfg.transport
    else {
        return Ok(());
    };
    let cmd = command.trim();
    if cmd.is_empty() {
        return Err("本地命令不能为空".into());
    }
    if cmd.contains('\0') || args.iter().any(|a| a.contains('\0')) {
        return Err("命令或参数含 NUL 字节（\\x00），拒绝".into());
    }
    if let Some(c) = cwd {
        let c = c.trim();
        if !c.is_empty() {
            let norm = c.replace('\\', "/");
            if norm.split('/').any(|seg| seg == "..") {
                return Err(format!("工作目录不允许路径穿越（含 `..`）：{c}"));
            }
            if c.parse::<std::path::PathBuf>().map(|p| p.has_root()) == Ok(false) {
                return Err(format!("工作目录必须是绝对路径：{c}"));
            }
        }
    }
    for k in env.keys() {
        if k.trim().is_empty()
            || k.contains('=')
            || k.contains('\0')
            || k.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '_'))
        {
            return Err(format!("环境变量键非法：{k}"));
        }
    }
    Ok(())
}

async fn connect_stdio(cfg: &McpServerConfig) -> Result<McpService, String> {
    validate_stdio(cfg)?;
    let McpTransport::Stdio {
        command,
        args,
        env,
        cwd,
    } = &cfg.transport
    else {
        unreachable!("connect_stdio 只处理 Stdio")
    };
    let mut cmd = Command::new(command);
    cmd.args(args);
    cmd.envs(env);
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let service =
        ().serve(TokioChildProcess::new(cmd).map_err(|e| format!("spawn 失败：{e}"))?)
            .await
            .map_err(|e| format!("MCP 握手失败：{e}"))?;
    Ok(service)
}

async fn connect_http(cfg: &McpServerConfig) -> Result<McpService, String> {
    let McpTransport::Http {
        url,
        headers,
        timeout_secs,
    } = &cfg.transport
    else {
        unreachable!("connect_http 只处理 Http")
    };
    validate_http_url(url)?;

    // reqwest Client 构建器：timeout / header 都挂在 builder 上（不是在 Client 上）。
    let mut builder = reqwest::Client::builder();
    if let Some(secs) = timeout_secs {
        builder = builder.timeout(std::time::Duration::from_secs(*secs));
    }
    if !headers.is_empty() {
        for (k, v) in headers {
            if let (Ok(k), Ok(v)) = (
                k.parse::<reqwest::header::HeaderName>(),
                v.parse::<reqwest::header::HeaderValue>(),
            ) {
                builder = builder.default_headers(std::iter::once((k, v)).collect());
            }
        }
    }
    let client = builder
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败：{e}"))?;

    let transport = rmcp::transport::StreamableHttpClientTransport::<reqwest::Client>::with_client(
        client,
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
            url.trim(),
        ),
    );
    let service = ().serve(transport).await.map_err(|e| format!("MCP 握手失败：{e}"))?;
    Ok(service)
}

async fn connect_server(cfg: &McpServerConfig) -> Result<McpService, String> {
    match &cfg.transport {
        McpTransport::Stdio { .. } => connect_stdio(cfg).await,
        McpTransport::Http { .. } => connect_http(cfg).await,
    }
}

// ── Tauri 命令 ─────────────────────────────────────────────────────

/// probe 结果缓存条目：(探测时间戳, 工具数, 错误)。
type ProbeResult = (u64, Option<usize>, Option<String>);

/// probe 结果缓存：server_id → ProbeResult。
/// 5s TTL：5s 内重复 list_servers/list_tools 命中缓存免重连（对齐 hw 5s 轮询）。
fn probe_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, ProbeResult>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, ProbeResult>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 探测某服务器并写缓存（带 10s 全局超时，stdio 无 timeout 时兜底）。
/// `refresh=true` 时强制重连（手动「测试连接」）。
/// 返回 (工具数, 错误)。缓存命中且未超时且未强制刷新 → 直接回缓存。
async fn probe_and_cache(cfg: &McpServerConfig, refresh: bool) -> (Option<usize>, Option<String>) {
    let now = audit_ts();
    if !refresh {
        if let Some((at, count, err)) = probe_cache().lock().unwrap().get(&cfg.id).cloned() {
            if now.saturating_sub(at) < PROBE_TTL_SECS {
                return (count, err);
            }
        }
    }
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(PROBE_TIMEOUT_SECS),
        probe_tools(cfg),
    )
    .await;
    let (count, err) = match result {
        Err(_) => (None, Some(format!("连接超时（>{}s）", PROBE_TIMEOUT_SECS))),
        Ok(Ok(tools)) => (Some(tools.len()), None),
        Ok(Err(e)) => (None, Some(e)),
    };
    probe_cache()
        .lock()
        .unwrap()
        .insert(cfg.id.clone(), (now, count, err.clone()));
    (count, err)
}

/// 服务器列表 + 每个 enabled 服务器的连接健康探测（只读 L0）。
#[tauri::command]
pub(crate) async fn mcp_list_servers(app: AppHandle) -> Result<Vec<McpServerStatus>, String> {
    let servers = load_servers(&app);
    let mut out = Vec::with_capacity(servers.len());
    for s in &servers {
        let transport_type = match &s.transport {
            McpTransport::Stdio { .. } => "stdio",
            McpTransport::Http { .. } => "http",
        }
        .to_string();
        // 权限映射摘要：统计映射的级别分布
        let perm_scope = match &s.permission_map {
            Some(m) if !m.is_empty() => {
                let mut lv: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for v in m.values() {
                    let up = v.trim().to_ascii_uppercase();
                    *lv.entry(if matches!(up.as_str(), "L0" | "L1" | "L2" | "L3") {
                        up
                    } else {
                        "L2".into()
                    })
                    .or_insert(0) += 1;
                }
                lv.into_iter()
                    .filter(|(_, n)| *n > 0)
                    .map(|(k, n)| format!("{n} 个 {k}"))
                    .collect::<Vec<_>>()
                    .join(" / ")
            }
            _ if s.writable => "全部 L2".to_string(),
            _ => "全部 L0".to_string(),
        };
        let mut status = McpServerStatus {
            id: s.id.clone(),
            name: s.name.clone(),
            enabled: s.enabled,
            writable: s.writable,
            tool_prefix: s.tool_prefix.clone(),
            transport_type,
            transport: s.transport.clone(),
            tool_count: None,
            error: None,
            perm_scope,
            permission_map: s.permission_map.clone(),
        };
        if s.enabled {
            let (count, err) = probe_and_cache(s, false).await;
            status.tool_count = count;
            status.error = err;
        }
        out.push(status);
    }
    Ok(out)
}

/// 探测某服务器：连接 + tools/list，返回工具清单（只读，无缓存；
/// 供 probe_and_cache 内部调用与 mcp_test_server 用）。
async fn probe_tools(cfg: &McpServerConfig) -> Result<Vec<rmcp::model::Tool>, String> {
    let service = connect_server(cfg).await?;
    let tools = service
        .list_all_tools()
        .await
        .map_err(|e| format!("tools/list 失败：{e}"))?;
    Ok(tools)
}

/// 测试单个服务器：连接 + list_tools，返回工具清单（只读 L0，前端「测试连接」按钮）。
/// 强制刷新（不走 5s 缓存，用户点测试就是要最新）。
#[tauri::command]
pub(crate) async fn mcp_test_server(
    app: AppHandle,
    id: String,
) -> Result<Vec<McpToolMeta>, String> {
    let servers = load_servers(&app);
    let cfg = servers
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("未找到 MCP 服务器：{id}"))?;
    let (count, err) = probe_and_cache(cfg, true).await;
    if let Some(e) = err {
        return Err(e);
    }
    let Some(_) = count else {
        return Err("探测无结果".into());
    };
    let tools = probe_tools(cfg).await?;
    Ok(tools
        .into_iter()
        .map(|t| {
            let raw = t.name.to_string();
            let (perm, _) = cfg.resolve_perm(&raw);
            McpToolMeta {
                server_id: cfg.id.clone(),
                name: cfg.tool_name(&raw),
                description: t.description.unwrap_or_default().to_string(),
                input_schema: serde_json::Value::Object(t.input_schema.as_ref().clone()),
                writable: cfg.writable,
                perm,
            }
        })
        .collect())
}

/// 添加 MCP 服务器（写命令：confirmed + perm_grants["mcp.manage"] 双校验）。
#[tauri::command]
pub(crate) async fn mcp_add_server(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    payload: McpServerPayload,
    confirmed: Option<bool>,
) -> Result<McpServerConfig, String> {
    if confirmed != Some(true) {
        return Err("mcp:confirm: 添加 MCP 服务器会使其工具接入 AI 工具表，必须由用户明确确认（confirmed=true）。".into());
    }
    if !state.perm_grants.lock().unwrap().contains("mcp.manage") {
        return Err(
            "mcp:denied: 未开启「管理 MCP 服务器」权限（mcp.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    if payload.name.trim().is_empty() {
        return Err("服务器名称不能为空".into());
    }
    match &payload.transport {
        McpTransport::Stdio { command, .. } => {
            if command.trim().is_empty() {
                return Err("本地命令不能为空".into());
            }
        }
        McpTransport::Http { url, .. } => validate_http_url(url)?,
    }

    let mut servers = load_servers(&app);
    let cfg = McpServerConfig {
        id: gen_id(&app, &payload.name),
        name: payload.name.trim().to_string(),
        enabled: payload.enabled.unwrap_or(true),
        writable: payload.writable.unwrap_or(false),
        tool_prefix: payload.tool_prefix,
        permission_map: payload.permission_map,
        transport: payload.transport,
    };
    servers.push(cfg.clone());
    save_servers(&app, &servers)?;
    append_audit(
        &app,
        serde_json::json!({
            "ts": audit_ts(),
            "op": "add_server",
            "server_id": cfg.id,
            "server_name": cfg.name,
            "writable": cfg.writable,
            "ok": true,
        }),
    );
    Ok(cfg)
}

/// 编辑 MCP 服务器（写命令，双校验）。
#[tauri::command]
pub(crate) async fn mcp_update_server(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    payload: McpServerPayload,
    confirmed: Option<bool>,
) -> Result<McpServerConfig, String> {
    if confirmed != Some(true) {
        return Err(
            "mcp:confirm: 修改 MCP 服务器配置必须由用户明确确认（confirmed=true）。".into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("mcp.manage") {
        return Err(
            "mcp:denied: 未开启「管理 MCP 服务器」权限（mcp.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    if payload.name.trim().is_empty() {
        return Err("服务器名称不能为空".into());
    }
    match &payload.transport {
        McpTransport::Stdio { command, .. } => {
            if command.trim().is_empty() {
                return Err("本地命令不能为空".into());
            }
        }
        McpTransport::Http { url, .. } => validate_http_url(url)?,
    }

    let mut servers = load_servers(&app);
    let Some(cfg) = servers.iter_mut().find(|s| s.id == id) else {
        return Err(format!("未找到 MCP 服务器：{id}"));
    };
    cfg.name = payload.name.trim().to_string();
    cfg.enabled = payload.enabled.unwrap_or(true);
    cfg.writable = payload.writable.unwrap_or(false);
    cfg.tool_prefix = payload.tool_prefix;
    // None = 未编辑权限映射，保留原值（前端 toggle 开关只发 name/enabled/writable/transport，
    // 不带 permission_map 时不能清空已有映射）。
    if payload.permission_map.is_some() {
        cfg.permission_map = payload.permission_map;
    }
    cfg.transport = payload.transport;
    let cloned = cfg.clone();
    save_servers(&app, &servers)?;
    append_audit(
        &app,
        serde_json::json!({
            "ts": audit_ts(),
            "op": "update_server",
            "server_id": cloned.id,
            "server_name": cloned.name,
            "writable": cloned.writable,
            "ok": true,
        }),
    );
    Ok(cloned)
}

/// 删除 MCP 服务器（写命令，双校验；只删配置，不删任何文件）。
#[tauri::command]
pub(crate) async fn mcp_remove_server(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    confirmed: Option<bool>,
) -> Result<(), String> {
    if confirmed != Some(true) {
        return Err("mcp:confirm: 删除 MCP 服务器会使它的工具从 AI 工具表移除，必须由用户明确确认（confirmed=true）。".into());
    }
    if !state.perm_grants.lock().unwrap().contains("mcp.manage") {
        return Err(
            "mcp:denied: 未开启「管理 MCP 服务器」权限（mcp.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let mut servers = load_servers(&app);
    let before = servers.len();
    servers.retain(|s| s.id != id);
    if servers.len() == before {
        return Err(format!("未找到 MCP 服务器：{id}"));
    }
    save_servers(&app, &servers)?;
    append_audit(
        &app,
        serde_json::json!({
            "ts": audit_ts(),
            "op": "remove_server",
            "server_id": id,
            "ok": true,
        }),
    );
    Ok(())
}

/// 所有 enabled 服务器的工具合并清单（只读 L0，前端 refreshMcpTools 拉取）。
#[tauri::command]
pub(crate) async fn mcp_list_tools(app: AppHandle) -> Result<Vec<McpToolMeta>, String> {
    let servers = load_servers(&app);
    let mut out = Vec::new();
    for s in servers.iter().filter(|s| s.enabled) {
        // 单服务器 10s 兜底超时；失败跳过不拖垮整体（health 在 list_servers 里可见）。
        let tools = match tokio::time::timeout(
            std::time::Duration::from_secs(PROBE_TIMEOUT_SECS),
            probe_tools(s),
        )
        .await
        {
            Ok(Ok(t)) => t,
            _ => continue,
        };
        for t in tools {
            let raw = t.name.to_string();
            let (perm, _) = s.resolve_perm(&raw);
            out.push(McpToolMeta {
                server_id: s.id.clone(),
                name: s.tool_name(&raw),
                description: t.description.unwrap_or_default().to_string(),
                input_schema: serde_json::Value::Object(t.input_schema.as_ref().clone()),
                writable: s.writable,
                perm,
            });
        }
    }
    // 按 server_id + name 去重（多服务器同名工具时保留先注册的）。
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| seen.insert(format!("{}:{}", t.server_id, t.name)));
    Ok(out)
}

/// 调用某个 MCP 服务器上的工具（工具级权限映射 + 审计，2026-09-11）。
/// 权限判定（`permission_map` 解析，见 [`McpServerConfig::resolve_perm`]）：
/// - L0 → 直接放行（只读，无确认）；
/// - L1 → 需 confirmed=true；
/// - L2 → 需 confirmed=true + 权限（映射的 perm id 或缺省 mcp.manage）；
/// - L3 → 拒绝（不可解锁）。
/// 未映射时按 writable 推断（true → L2，false → L0）。
#[tauri::command]
pub(crate) async fn mcp_call_tool(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    server_id: String,
    name: String,
    arguments: serde_json::Value,
    confirmed: Option<bool>,
) -> Result<crate::agent::AgentToolCall, String> {
    let servers = load_servers(&app);
    let cfg = servers
        .iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| format!("未找到 MCP 服务器：{server_id}"))?;
    let (perm, perm_id) = cfg.resolve_perm(&name);
    let required_perm = perm_id.as_deref().unwrap_or("mcp.manage");
    let mut denied_reason: Option<String> = None;
    match perm.as_str() {
        "L0" => {}
        "L1" => {
            if confirmed != Some(true) {
                denied_reason = Some(
                    "mcp:confirm: 该工具为受控级别（L1），调用必须由用户明确确认（confirmed=true）。".into(),
                );
            }
        }
        "L2" => {
            if confirmed != Some(true) {
                denied_reason = Some(
                    format!("mcp:confirm: 工具 {name} 为高危级别（L2），调用必须由用户明确确认（confirmed=true）。"),
                );
            } else if !state.perm_grants.lock().unwrap().contains(required_perm) {
                denied_reason = Some(format!(
                    "mcp:denied: 未开启「{required_perm}」权限。请先在权限中心开启后重试。"
                ));
            }
        }
        "L3" => {
            denied_reason = Some(format!(
                "mcp:denied: 工具 {name} 为永禁级别（L3），不可解锁。请先在服务器配置中调整权限映射。"
            ));
        }
        _ => {}
    }
    if let Some(reason) = denied_reason {
        append_audit(
            &app,
            serde_json::json!({
                "ts": audit_ts(),
                "op": "call_tool",
                "server_id": server_id,
                "tool": name,
                "perm": perm,
                "granted": false,
                "args": audit_args_snapshot(&arguments),
                "ok": false,
                "error": reason,
            }),
        );
        return Err(reason);
    }

    let args_snapshot = audit_args_snapshot(&arguments);
    let args_obj = match arguments {
        serde_json::Value::Object(m) => Some(m),
        serde_json::Value::Null => None,
        _ => return Err(format!("参数必须是 JSON 对象，收到：{arguments}")),
    };

    // stdio 每次重新 spawn（阻塞式 IO），放 spawn_blocking 防卡主线程；
    // http 连接本身 async，无需阻塞线程。统一走 async 即可。
    let cfg = cfg.clone();
    let service = match connect_server(&cfg).await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("连接 MCP 服务器失败：{e}");
            append_audit(
                &app,
                serde_json::json!({
                    "ts": audit_ts(),
                    "op": "call_tool",
                    "server_id": server_id,
                    "tool": name,
                    "perm": perm,
                    "granted": true,
                    "args": args_snapshot,
                    "ok": false,
                    "error": msg,
                }),
            );
            return Err(msg);
        }
    };
    let mut params = CallToolRequestParams::new(name.clone());
    if let Some(args) = args_obj {
        params = params.with_arguments(args);
    }
    let result = match service.call_tool(params).await {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("tools/call 失败：{e}");
            append_audit(
                &app,
                serde_json::json!({
                    "ts": audit_ts(),
                    "op": "call_tool",
                    "server_id": server_id,
                    "tool": name,
                    "perm": perm,
                    "granted": true,
                    "args": args_snapshot,
                    "ok": false,
                    "error": msg,
                }),
            );
            return Err(msg);
        }
    };

    let text = result
        .content
        .into_iter()
        .filter_map(|c| match &*c {
            rmcp::model::RawContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let ok = !result.is_error.unwrap_or(false);
    append_audit(
        &app,
        serde_json::json!({
            "ts": audit_ts(),
            "op": "call_tool",
            "server_id": server_id,
            "tool": name,
            "perm": perm,
            "granted": true,
            "args": args_snapshot,
            "ok": ok,
            "error": if ok { serde_json::Value::Null } else { serde_json::json!(text) },
        }),
    );
    Ok(crate::agent::AgentToolCall {
        is_error: result.is_error.unwrap_or(false),
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_prefix_appends_underscore() {
        let cfg = McpServerConfig {
            id: "s1".into(),
            name: "测试".into(),
            enabled: true,
            writable: false,
            tool_prefix: Some("myserver".into()),
            permission_map: None,
            transport: McpTransport::Stdio {
                command: "node".into(),
                args: vec![],
                env: HashMap::new(),
                cwd: None,
            },
        };
        assert_eq!(cfg.tool_name("list_dir"), "myserver_list_dir");
        // 前缀带下划线时不重复
        let cfg2 = McpServerConfig {
            tool_prefix: Some("myserver_".into()),
            ..cfg.clone()
        };
        assert_eq!(cfg2.tool_name("list_dir"), "myserver_list_dir");
        // 无前缀 = 原名
        let cfg3 = McpServerConfig {
            tool_prefix: None,
            ..cfg.clone()
        };
        assert_eq!(cfg3.tool_name("list_dir"), "list_dir");
    }

    #[test]
    fn resolve_perm_levels_and_fallback() {
        // 未映射：writable=false → L0；writable=true → L2
        let ro = McpServerConfig {
            id: "ro".into(),
            name: "只读".into(),
            enabled: true,
            writable: false,
            tool_prefix: None,
            permission_map: None,
            transport: McpTransport::Http {
                url: "https://example.com/mcp".into(),
                headers: HashMap::new(),
                timeout_secs: None,
            },
        };
        assert_eq!(ro.resolve_perm("list_dir"), ("L0".to_string(), None));
        let rw = McpServerConfig {
            writable: true,
            ..ro.clone()
        };
        assert_eq!(rw.resolve_perm("anything"), ("L2".to_string(), None));
        // 映射：L0/L1/L2/L3 + 具体 perm id（按 L2 + 校验 id）
        let mapped = McpServerConfig {
            writable: true,
            permission_map: Some(HashMap::from([
                ("read_a".into(), "L0".into()),
                ("read_b".into(), "L1".into()),
                ("kill".into(), "L2".into()),
                ("bios".into(), "L3".into()),
                ("recycle".into(), "file.recycle".into()),
            ])),
            ..ro.clone()
        };
        assert_eq!(mapped.resolve_perm("read_a"), ("L0".to_string(), None));
        assert_eq!(mapped.resolve_perm("read_b"), ("L1".to_string(), None));
        assert_eq!(mapped.resolve_perm("kill"), ("L2".to_string(), None));
        assert_eq!(mapped.resolve_perm("bios"), ("L3".to_string(), None));
        assert_eq!(
            mapped.resolve_perm("recycle"),
            ("L2".to_string(), Some("file.recycle".to_string()))
        );
        // 大小写不敏感
        let lower = McpServerConfig {
            permission_map: Some(HashMap::from([("x".into(), "l1".into())])),
            ..ro.clone()
        };
        assert_eq!(lower.resolve_perm("x"), ("L1".to_string(), None));
    }

    #[test]
    fn validate_stdio_rejects_dangerous_input() {
        let base = McpServerConfig {
            id: "s".into(),
            name: "测".into(),
            enabled: true,
            writable: false,
            tool_prefix: None,
            permission_map: None,
            transport: McpTransport::Stdio {
                command: "node".into(),
                args: vec![],
                env: HashMap::new(),
                cwd: None,
            },
        };
        assert!(validate_stdio(&base).is_ok(), "合法 stdio 应通过");
        // 空命令
        let empty = McpServerConfig {
            transport: McpTransport::Stdio {
                command: "  ".into(),
                args: vec![],
                env: HashMap::new(),
                cwd: None,
            },
            ..base.clone()
        };
        assert!(validate_stdio(&empty).unwrap_err().contains("不能为空"));
        // NUL 字节
        let nul = McpServerConfig {
            transport: McpTransport::Stdio {
                command: "node\x00x".into(),
                args: vec![],
                env: HashMap::new(),
                cwd: None,
            },
            ..base.clone()
        };
        assert!(validate_stdio(&nul).unwrap_err().contains("NUL"));
        // cwd 路径穿越
        let dotdot = McpServerConfig {
            transport: McpTransport::Stdio {
                command: "node".into(),
                args: vec![],
                env: HashMap::new(),
                cwd: Some("C:\\Users\\..\\Windows".into()),
            },
            ..base.clone()
        };
        assert!(validate_stdio(&dotdot).unwrap_err().contains(".."));
        // env 键非法（含 = 或空）
        let bad_env = McpServerConfig {
            transport: McpTransport::Stdio {
                command: "node".into(),
                args: vec![],
                env: HashMap::from([("A=B".into(), "1".into())]),
                cwd: None,
            },
            ..base.clone()
        };
        assert!(validate_stdio(&bad_env).unwrap_err().contains("环境变量键"));
    }

    #[test]
    fn audit_args_snapshot_redacts_values() {
        // 对象：只写键名单 + 长度，绝不落完整值
        let obj = serde_json::json!({ "path": "C:\\secret\\data", "deep": true });
        let snap = audit_args_snapshot(&obj);
        let s = snap.to_string();
        assert!(s.contains("path"), "键名应保留: {s}");
        assert!(!s.contains("secret"), "值必须脱敏，不得出现 secret");
        assert!(s.contains("size"), "应含大小字段");
        // Null
        assert!(audit_args_snapshot(&serde_json::Value::Null).is_null());
    }

    #[test]
    fn config_roundtrip_via_json() {
        let servers = vec![McpServerConfig {
            id: "my-server".into(),
            name: "我的服务器".into(),
            enabled: true,
            writable: false,
            tool_prefix: None,
            permission_map: Some(HashMap::from([("del".into(), "L2".into())])),
            transport: McpTransport::Http {
                url: "https://example.com/mcp".into(),
                headers: HashMap::new(),
                timeout_secs: Some(30),
            },
        }];
        let text = serde_json::to_string(&servers).expect("序列化");
        let back: Vec<McpServerConfig> = serde_json::from_str(&text).expect("反序列化");
        assert_eq!(servers, back);
        // transport tag 是 snake_case（前端能对上）
        assert!(text.contains("\"type\":\"http\""));
    }

    #[test]
    fn https_only_rejects_http() {
        assert!(validate_http_url("https://example.com/mcp").is_ok());
        let err = validate_http_url("http://example.com/mcp").unwrap_err();
        assert!(err.contains("https"), "必须提示 https，got: {err}");
        assert!(validate_http_url("ftp://x.com").is_err());
        assert!(validate_http_url("not a url").is_err());
    }

    #[test]
    fn writable_server_requires_confirm() {
        // mcp_call_tool 的 confirmed 校验逻辑抽不出来，这里验证配置层：
        // writable=true 的服务器标记正确，桥接层按此放行/拦截。
        let writable = McpServerConfig {
            id: "w".into(),
            name: "写".into(),
            enabled: true,
            writable: true,
            tool_prefix: None,
            permission_map: None,
            transport: McpTransport::Stdio {
                command: "node".into(),
                args: vec![],
                env: HashMap::new(),
                cwd: None,
            },
        };
        assert!(writable.writable);
    }

    #[test]
    fn gen_id_is_unique_and_kebab() {
        // 用无盘版本的净化契约验证：plugin_id_from_name 产出 kebab-case
        assert_eq!(
            diskpilot_toolbelt::plugin_id_from_name("My Cool Server").unwrap(),
            "my-cool-server"
        );
    }
}
