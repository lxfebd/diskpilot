//! agent-server 入口：MCP stdio 服务。
//! 启动后通过 stdin/stdout 与客户端（DiskPilot 后端 / 任意 MCP 客户端）通信，
//! 暴露 tools/list + tools/call。

mod ps;
mod server;
mod tools;

use rmcp::ServiceExt;
use tools::AgentServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 日志走 stderr（stdio 通道必须保持干净，只走 MCP 帧）
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let service = AgentServer::new()
        .serve(rmcp::transport::io::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}
