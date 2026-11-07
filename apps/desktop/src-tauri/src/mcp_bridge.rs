//! MCP 桥接层：stdio / http 传输 → 统一 RunningService。
//!
//! 从 `mcp.rs` 拆分（2026-10-12）：连接/校验逻辑独立成模块，
//! 命令层只管「调 connect_server 拿服务、再调工具」。

use std::process::Stdio;

use rmcp::service::ServiceExt;
use rmcp::transport::TokioChildProcess;
use tokio::process::Command;

use crate::mcp::{McpServerConfig, McpTransport};

/// 统一的服务句柄（rmcp 客户端运行态）。
type McpService = rmcp::service::RunningService<rmcp::service::RoleClient, ()>;

/// 纯 scheme/格式校验（同步，测试可用）。
pub(crate) fn validate_http_url_scheme(url: &str) -> Result<(), String> {
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

/// R505 完整校验 = scheme + SSRF 总闸（字面量 + DNS 二次校验）。
/// 配置落盘后每次 connect 也要走这里，不能只在 mcp_add_server 时验一次。
pub(crate) async fn validate_http_url(url: &str) -> Result<(), String> {
    validate_http_url_scheme(url)?;
    if let Some(blocked) = crate::ai::ssrf_block_reason_async(url.trim()).await {
        return Err(format!("MCP 服务器地址不可用（{blocked}）：{url}"));
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
    for (k, v) in env {
        if k.trim().is_empty()
            || k.contains('=')
            || k.contains('\0')
            || k.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '_'))
        {
            return Err(format!("环境变量键非法：{k}"));
        }
        // `std::process::Command::env` 对含 NUL 的**值**同样会返回
        // "nul byte found in provided data"（实测）→ 该服务器永远连不上。
        // 与键校验对齐，在配置入库前就拒绝，宁可报配置错也别留一个
        // 永远 spawn 失败的僵尸服务器。
        if v.contains('\0') {
            return Err(format!("环境变量值含 NUL 字节（\\x00）：{k}"));
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
    // R505：每次 connect 都重新做 SSRF 校验（配置落盘后不能只信一次）。
    validate_http_url(url).await?;

    // reqwest Client 构建器：timeout / header 都挂在 builder 上（不是在 Client 上）。
    let mut builder = reqwest::Client::builder();
    match timeout_secs {
        // 0 在 reqwest 里表示「立即超时」（不是「不限时」），会把一切请求打失败；
        // 配置者填 0 的意图是「用默认/不限」，这里显式按未设置处理。
        Some(0) => {}
        Some(secs) => builder = builder.timeout(std::time::Duration::from_secs(*secs)),
        None => {}
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

pub(crate) async fn connect_server(cfg: &McpServerConfig) -> Result<McpService, String> {
    match &cfg.transport {
        McpTransport::Stdio { .. } => connect_stdio(cfg).await,
        McpTransport::Http { .. } => connect_http(cfg).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_stdio() -> McpServerConfig {
        McpServerConfig {
            id: "s".into(),
            name: "测".into(),
            enabled: true,
            writable: false,
            tool_prefix: None,
            permission_map: None,
            transport: McpTransport::Stdio {
                command: "node".into(),
                args: vec![],
                env: std::collections::HashMap::new(),
                cwd: None,
            },
        }
    }

    #[test]
    fn validate_stdio_rejects_dangerous_input() {
        let base = base_stdio();
        assert!(validate_stdio(&base).is_ok(), "合法 stdio 应通过");
        // 空命令
        let empty = McpServerConfig {
            transport: McpTransport::Stdio {
                command: "  ".into(),
                args: vec![],
                env: std::collections::HashMap::new(),
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
                env: std::collections::HashMap::new(),
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
                env: std::collections::HashMap::new(),
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
                env: std::collections::HashMap::from([("A=B".into(), "1".into())]),
                cwd: None,
            },
            ..base.clone()
        };
        assert!(validate_stdio(&bad_env).unwrap_err().contains("环境变量键"));
        // env **值**含 NUL：Command::env 实测会拒（nul byte found），必须在
        // 配置层先拦，否则服务器永远 spawn 失败、UI 却显示"已添加"。
        let nul_val = McpServerConfig {
            transport: McpTransport::Stdio {
                command: "node".into(),
                args: vec![],
                env: std::collections::HashMap::from([("TOKEN".into(), "a\x00b".into())]),
                cwd: None,
            },
            ..base.clone()
        };
        assert!(validate_stdio(&nul_val).unwrap_err().contains("值含 NUL"));
    }

    #[test]
    fn https_only_rejects_http() {
        assert!(validate_http_url_scheme("https://example.com/mcp").is_ok());
        let err = validate_http_url_scheme("http://example.com/mcp").unwrap_err();
        assert!(err.contains("https"), "必须提示 https，got: {err}");
        assert!(validate_http_url_scheme("ftp://x.com").is_err());
        assert!(validate_http_url_scheme("not a url").is_err());
    }

    #[tokio::test]
    async fn ssrf_blocks_loopback_and_metadata() {
        // R505：内网/元数据地址即使 https 也拒绝（含 DNS 二次校验路径）。
        assert!(validate_http_url("https://127.0.0.1/mcp").await.is_err());
        assert!(validate_http_url("https://169.254.169.254/latest")
            .await
            .is_err());
        assert!(validate_http_url("https://localhost:9000/mcp")
            .await
            .is_err());
    }
}
