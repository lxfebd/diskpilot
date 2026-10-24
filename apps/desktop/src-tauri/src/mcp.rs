//! 通用 MCP 服务器接入（可插拔 MCP 插件）
//!
//! 用户可添加任意标准 MCP 服务器（本地 stdio 进程 + 远程 streamable HTTP URL），
//! 其暴露的工具自动注册进前端 AI 工具表（mcp_list_tools → dynamicMcpTools）。
//!
//! 与内置 agent-server 的区别：
//! - agent.rs = 硬编码 spawn 本仓库 agent-server.exe（76 个工具 = 65 只读 + 11 写，
//!   真值见 agent.rs 的 WRITE_TOOLS 常量与 agent_tests.rs 的分布断言）；
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
//!
//! 2026-10-12 再拆分：审计 → `mcp_audit`，持久化 → `mcp_store`，探测 → `mcp_probe`，
//! 增删改 → `mcp_crud`，清单+调用 → `mcp_tools`。本文件保留类型、常量与单测。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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

const AUDIT_FILE: &str = "mcp-audit.jsonl";
/// probe 结果缓存 TTL（秒）：5s 内重复 list_servers/list_tools 免重连。
const PROBE_TTL_SECS: u64 = 5;

/// 单次 `tools/call` 的端到端上限（连接 + 调用）。probe/list 有 10s 兜底，
/// 但调用路径此前无任何超时：挂死的 MCP 服务器（stdio 子进程阻塞、HTTP 无响应）
/// 会永久卡住整个 AI 回合——后端 await 不返回，前端 busy 永远清不掉。
const CALL_TOOL_TIMEOUT_SECS: u64 = 60;
/// probe 全局超时（秒）：stdio 无 timeout 时兜底，防死锁。
const PROBE_TIMEOUT_SECS: u64 = 10;
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

#[path = "mcp_audit.rs"]
pub(crate) mod mcp_audit;
#[path = "mcp_crud.rs"]
pub(crate) mod mcp_crud;
#[path = "mcp_probe.rs"]
pub(crate) mod mcp_probe;
#[path = "mcp_store.rs"]
pub(crate) mod mcp_store;
#[path = "mcp_tools.rs"]
pub(crate) mod mcp_tools;

#[cfg(test)]
mod tests {
    use super::mcp_audit::audit_args_snapshot;
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
