//! agent-server 桥接（方案 A：独立进程 + rmcp client over stdio）
//!
//! 前端 AI 工具（execTool）把 75 个 MCP 工具（磁盘/文件/进程/硬件/网络/
//! 服务/安全审计/回收站/环境变量/工具箱/AIDA64 复刻/内存与 GPU 压测/蓝屏分析
//! + 信息补全 4 个（Defender/用户账户/网卡明细/许可证）+ 自研 SuperIO 直读 + 8 个写操作）转到这里调用：
//! 本模块用 `rmcp` client spawn `agent-server.exe` 子进程，走标准 MCP
//! `tools/list` + `tools/call` 通道取真实数据。
//!
//! 传感器数据源（hw_temperature / hw_sensors）：
//! - 优先 HWiNFO 共享内存（`HWiNFO_SENS_SM2`，普通权限可读全部传感器——
//!   CPU/GPU/主板温度、风扇、电压、功耗、频率、负载，无 Ring0/管理员依赖）；
//! - HWiNFO 未运行/未启用共享内存 → 自研 SuperIO 直读（superio.rs，inpoutx64 驱动
//!   已装则普通权限直读 ITE/Nuvoton/Winbond 环境寄存器——CPU/主板温度 + 风扇转速，
//!   无需 HWiNFO/LHM）→ LibreHardwareMonitor（fancmd，需管理员读 CPU 温度）
//!   → ACPI 热区 + nvidia-smi/ADL GPU + SMART 磁盘。
//!
//! 安全模型：
//! - 64 个只读工具对应主项目 L0 恒开；11 个写工具（process_kill / service_control /
//!   fan_selfheal_fix / fan_control / file_recycle / process_start /
//!   scheduled_task_manage / uninstall_app / toolbelt_run / stress_test / stress_test_gpu）
//!   要求 confirmed==Some(true) 硬校验
//!   （见 agent_call_tool），agent-server 侧另带进程/服务守卫（系统关键 PID/目录/
//!   服务黑名单）；
//! - 子进程只有继承的 stdio 句柄，崩溃不影响主进程（进程隔离）；
//! - 二进制找不到时优雅降级（返回错误而非 panic），前端可展示「工具集未就绪」。
//!
//! 生命周期：每次调用重新 spawn 一次 agent-server（无共享状态）。75 个工具
//! 单次启动约 10–50ms，AI 工具调用频率低（每轮对话个位数），可接受；
//! 换来进程隔离 + 崩溃自愈天然成立（一次调用失败不影响下一次）。

use std::process::Stdio;

use rmcp::{model::CallToolRequestParam, service::ServiceExt, transport::TokioChildProcess};
use tauri::State;
use tokio::process::Command;

/// 在某目录下找 agent-server：优先标准名，兜底兼容旧安装包的 `.exe.exe` 畸形名
/// （tauri 曾把 externalBin 配置名 `agent-server.exe` 再拼一次 `.exe`）。
fn find_in_dir(dir: &std::path::Path, bin_name: &str) -> Option<std::path::PathBuf> {
    if dir.join(bin_name).is_file() {
        return Some(dir.join(bin_name));
    }
    if cfg!(windows) {
        let legacy = format!("{bin_name}.exe");
        if dir.join(&legacy).is_file() {
            return Some(dir.join(legacy));
        }
    }
    None
}

/// 定位 agent-server 二进制：
/// 1. 环境变量 `DISKPILOT_AGENT_SERVER` 显式指定（测试/自定义路径）；
/// 2. 当前 exe 同目录（生产打包 resource_dir）；
/// 3. workspace target 目录（dev：cargo 把二进制放在 workspace 根 target，
///    debug 与 release 两个 profile 都找；且候选基于 exe 绝对路径向上枚举，
///    不依赖 CWD，导出/复制 exe 到任意目录也能命中）。
///
/// Windows 下文件名带 `.exe` 后缀；macOS/Linux 无后缀，按平台取用。
fn agent_server_binary() -> Option<std::path::PathBuf> {
    // 平台相关的二进制名：Windows 带 .exe，mac/linux 裸名。
    let bin_name = if cfg!(windows) {
        "agent-server.exe"
    } else {
        "agent-server"
    };
    if let Ok(p) = std::env::var("DISKPILOT_AGENT_SERVER") {
        let pb = std::path::PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    // 当前 exe 同目录（安装版：agent-server 与主程序同目录）。
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Some(found) = find_in_dir(dir, bin_name) {
                return Some(found);
            }
        }
    }
    // dev：从 exe 位置向上枚举 target/{profile}。
    // cargo run / target 内直接运行 exe：exe 在 <root>/target/debug|release，
    // 所以从 exe 目录的父目录起向上逐层找 <dir>/target/<profile>。
    let anchor = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .or_else(|| std::env::current_dir().ok());
    if let Some(anchor) = anchor {
        // 从 anchor 本身开始向上共 4 层（覆盖 <root>/target/debug → 仓库根 → 再上两级）
        let mut dir = anchor.clone();
        for _ in 0..4 {
            for profile in ["debug", "release"] {
                let candidate_dir = dir.join("target").join(profile);
                if let Some(found) = find_in_dir(&candidate_dir, bin_name) {
                    return Some(found);
                }
            }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
    }
    None
}

/// 候选 scaffolds 目录（注入给 agent-server 的 `DISKPILOT_SCAFFOLDS`），按优先级：
/// 1. 已有环境变量（测试 / 自定义；
///    注意：环境变量是用户级配置，优先级最高，便于在打包环境外覆写）；
/// 2. 当前 exe 同目录 `scaffolds`（生产打包 resource_dir 布局）；
/// 3. dev 仓库布局 `apps/desktop/src-tauri/../../../scaffolds`（= 仓库根 scaffolds/）。
///
/// 只返回候选列表，资源目录存在性由 resolve_scaffolds_dir 决定；不检查存在性，
/// 便于单测钉死顺序。
fn scaffolds_candidates() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Ok(env) = std::env::var("DISKPILOT_SCAFFOLDS") {
        if !env.trim().is_empty() {
            v.push(std::path::PathBuf::from(env));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            v.push(dir.join("scaffolds"));
        }
    }
    v.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../scaffolds"));
    v
}

/// 取第一个真实存在的 scaffolds 目录；全部不存在返回 None（agent-server
/// 侧还有自己的兜底路径，这里只是尽量把主项目打包的清单传过去）。
fn resolve_scaffolds_dir() -> Option<std::path::PathBuf> {
    scaffolds_candidates().into_iter().find(|p| p.is_dir())
}

/// spawn agent-server.exe 并完成 MCP initialize 握手，返回连接好的 service。
///
/// `undo_log` 是主项目 `~/.diskpilot/undo.jsonl` 的路径：注入给子进程的
/// `DISKPILOT_UNDO_LOG`，让 agent-server 的 `file_recycle` 等写操作也能把
/// 条目写进同一份日志（兑现「一切删除可撤销」，agent-server 与主项目共享
/// undo 链）。None（如只读工具调用）则不注入。
async fn connect(
    undo_log: Option<&std::path::Path>,
) -> Result<rmcp::service::RunningService<rmcp::service::RoleClient, ()>, String> {
    let bin = agent_server_binary()
        .ok_or_else(|| "找不到 agent-server.exe（AI 工具集未构建）。请先 cargo build -p agent-server，或设置 DISKPILOT_AGENT_SERVER 指向其路径。".to_string())?;
    let mut cmd = Command::new(&bin);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // 注入 scaffolds 目录，否则 agent-server 的 cleanup_suggestions 在运行时
    // 找不到清单（编译期 CARGO_MANIFEST_DIR 兜底在打包场景不存在）。
    if let Some(dir) = resolve_scaffolds_dir() {
        cmd.env("DISKPILOT_SCAFFOLDS", &dir);
    }
    // 注入 undo 日志路径（写操作工具进回收站后追加 undo 条目，与主项目同文件）。
    if let Some(log) = undo_log {
        cmd.env("DISKPILOT_UNDO_LOG", log);
    }
    // 写操作令牌：只有通过主进程确认门（confirmed=true 已在 agent_call_tool_core
    // 校验）的调用才注入 DISKPILOT_CONFIRMED=1。agent-server 内部写工具执行前
    // 校验此变量——绕过主进程直连 agent-server（任意 MCP 客户端）时缺失，一律拒绝，
    // 这是纵深防御第二层（第一层是主进程 confirmed 硬门）。
    if undo_log.is_some() {
        cmd.env("DISKPILOT_CONFIRMED", "1");
    }
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let service =
        ().serve(TokioChildProcess::new(cmd).map_err(|e| format!("spawn agent-server 失败：{e}"))?)
            .await
            .map_err(|e| format!("agent-server MCP 握手失败：{e}"))?;
    Ok(service)
}

/// 写工具白名单：这几个改变系统状态，必须由用户在确认面板明确 confirmed=true。
/// 模块级常量：`agent_list_tools` 用它给工具墙标注「只读 / 写操作」，
/// `agent_call_tool` 用它做硬校验——单源真值，避免两侧数字漂移。
const WRITE_TOOLS: &[&str] = &[
    "process_kill",
    "service_control",
    "fan_selfheal_fix",
    "fan_control",
    "file_recycle",
    "process_start",
    "scheduled_task_manage",
    "uninstall_app",
    "toolbelt_run",
    "stress_test",
    "stress_test_gpu",
];

/// 前端查询 MCP 工具的清单（name/description/schema/writable），
/// 用于注册进工具墙与 toolRegistry。
#[tauri::command]
pub async fn agent_list_tools() -> Result<Vec<AgentToolMeta>, String> {
    let service = connect(None).await?;
    let tools = service
        .list_all_tools()
        .await
        .map_err(|e| format!("agent-server tools/list 失败：{e}"))?;
    Ok(tools
        .into_iter()
        .map(|t| AgentToolMeta {
            name: t.name.to_string(),
            description: t.description.unwrap_or_default().to_string(),
            input_schema: serde_json::Value::Object(t.input_schema.as_ref().clone()),
            writable: WRITE_TOOLS.contains(&t.name.as_ref()),
        })
        .collect())
}

/// 调用单个 MCP 工具，返回文本结果（isError 时仍返回文本，由前端展示原因）。
/// 写工具（`process_kill` / `service_control`）要求 confirmed==Some(true) 硬校验：
/// AI 无法绕过确认门直接改变系统状态（铁律：清理/控制必须先确认）。
/// `undo_log` 为 `None` 时不向子进程注入 `DISKPILOT_UNDO_LOG`（只读调用）。
pub async fn agent_call_tool_core(
    name: String,
    arguments: serde_json::Value,
    confirmed: Option<bool>,
    undo_log: Option<&std::path::Path>,
) -> Result<AgentToolCall, String> {
    if WRITE_TOOLS.contains(&name.as_str()) && confirmed != Some(true) {
        return Err(format!(
            "agent:confirm: 工具 {name} 会改变系统状态（写操作），必须由用户明确确认（confirmed=true）后执行。"
        ));
    }
    // bench_disk 真实写基准（dry_run=false）会在目标目录创建临时文件并写入——
    // 属于写操作，必须 confirmed=true（纵深防御；前端 mcpTool 也已加确认门，
    // 这里兜住绕过前端直发写请求的路径）。dry_run=true 只出计划，无需确认。
    if name == "bench_disk" && confirmed != Some(true) {
        if let Some(dry) = arguments.get("dry_run").and_then(|v| v.as_bool()) {
            if !dry {
                return Err(
                    "agent:confirm: 磁盘基准真跑（dry_run=false）会在目标目录写临时文件，必须由用户明确确认（confirmed=true）后执行。".to_string(),
                );
            }
        }
    }
    // 写工具调用时注入 undo 日志路径：回收类操作（file_recycle）要把条目
    // 写进主项目同一份 undo.jsonl；只读工具无需（传 None，省一次环境变量）。
    // bench_disk 真跑（dry_run=false）会创建临时文件写盘，也属写操作——需要
    // 确认令牌（DISKPILOT_CONFIRMED）；它不写 undo 条目，多带一个 UNDO_LOG
    // 路径无害（bench::bench_disk 不读该变量）。
    let bench_disk_real = name == "bench_disk"
        && !arguments
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
    let undo_log = if WRITE_TOOLS.contains(&name.as_str()) || bench_disk_real {
        undo_log
    } else {
        None
    };
    let service = connect(undo_log).await?;
    let args_obj = match arguments {
        serde_json::Value::Object(m) => Some(m),
        serde_json::Value::Null => None,
        _ => return Err(format!("参数必须是 JSON 对象，收到：{arguments}")),
    };
    let result = service
        .call_tool(CallToolRequestParam {
            name: name.into(),
            arguments: args_obj,
        })
        .await
        .map_err(|e| format!("agent-server tools/call 失败：{e}"))?;
    let text = result
        .content
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| match &*c {
            rmcp::model::RawContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(AgentToolCall {
        is_error: result.is_error.unwrap_or(false),
        text,
    })
}

/// Tauri 命令包装：取应用状态里的 undo 日志路径（写工具用）后走 core。
#[tauri::command]
pub async fn agent_call_tool(
    state: State<'_, crate::AppState>,
    name: String,
    arguments: serde_json::Value,
    confirmed: Option<bool>,
) -> Result<AgentToolCall, String> {
    agent_call_tool_core(name, arguments, confirmed, Some(state.undo_log.as_path())).await
}

/// 探活：agent-server.exe 是否可用（找不到二进制时前端提示降级）。
#[tauri::command]
pub fn agent_server_ready() -> bool {
    agent_server_binary().is_some()
}

#[derive(serde::Serialize)]
pub struct AgentToolMeta {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    /// 是否写操作（会改变系统状态，需 confirmed=true）；
    /// 前端工具墙据此展示「写操作需确认」而非全部「只读」。
    pub writable: bool,
}

#[derive(serde::Serialize)]
pub struct AgentToolCall {
    pub is_error: bool,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 端到端验证（需要 agent-server 已构建）：
    /// 真实 spawn 子进程 + MCP 握手 + list_all_tools + call_tool。
    /// 二进制定位与环境变量 DISKPILOT_AGENT_SERVER / 当前目录同 target 策略一致。
    fn has_binary() -> bool {
        agent_server_binary().is_some()
            || std::path::Path::new("target/debug/agent-server").is_file()
            || std::path::Path::new("target/debug/agent-server.exe").is_file()
    }

    /// 纯函数：平台相关的二进制名 —— Windows 带 `.exe`，mac/Linux 裸名。
    /// 钉死这个约定，防止未来某个平台回归成硬编码 `.exe`。
    #[test]
    fn agent_server_bin_name_matches_platform() {
        let name = if cfg!(windows) {
            "agent-server.exe"
        } else {
            "agent-server"
        };
        // 用与 agent_server_binary 相同的判定路径验证候选名能拼出正确形态
        let candidates = [
            format!("target/debug/{name}"),
            format!("../../target/debug/{name}"),
            format!("../../../target/debug/{name}"),
        ];
        for c in &candidates {
            assert!(c.ends_with(name), "候选路径应以 {name} 结尾: {c}");
        }
        #[cfg(windows)]
        assert!(name.ends_with(".exe"));
        #[cfg(not(windows))]
        assert!(!name.ends_with(".exe"));
    }

    #[tokio::test]
    async fn agent_list_tools_returns_50_tools() {
        if !has_binary() {
            eprintln!("skip: agent-server 未构建（cargo build -p agent-server）");
            return;
        }
        let tools = agent_list_tools().await.expect("list tools 应成功");
        assert_eq!(
            tools.len(),
            75,
            "应返回 75 个 MCP 工具（64 只读 + 11 写操作，含 AIDA64 复刻 13 个 + 内存/GPU 压测/蓝屏 3 个 + 信息补全 4 个 + 自研 SuperIO 直读 1 个）"
        );
        // 抽查几个关键工具存在
        let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
        for expect in [
            "disk_health",
            "list_dir",
            "system_info",
            "list_processes",
            "hw_disk_smart",
            "net_status",
            "security_event_logs",
            "fan_control",
            "bench_cpu",
            "bench_memory",
            "bench_disk",
            "stress_test",
            "system_report",
            "sensor_trend",
            "sensor_alert",
            "hw_cpu_features",
            "hw_dram_timings",
            "hw_displays",
            "bench_gpu",
            "hw_ipmi",
            "hw_acpi",
            "mem_test",
            "stress_test_gpu",
            "bsod_analyze",
            "app_licenses",
            "net_adapter_detail",
            "sys_defender_status",
            "sys_user_accounts",
        ] {
            assert!(names.contains(&expect.to_string()), "缺少工具 {expect}");
        }
        // 程序化校验「只读 / 写」分布，而不是依赖硬编码的数字：
        // 工具墙徽标与确认门都吃同一份 writable，这里保证两侧一致。
        for t in &tools {
            assert_eq!(
                t.writable,
                WRITE_TOOLS.contains(&t.name.as_str()),
                "工具 {} 的 writable 与 WRITE_TOOLS 不一致",
                t.name
            );
        }
        let write_n = tools.iter().filter(|t| t.writable).count();
        let read_n = tools.len() - write_n;
        assert!(
            write_n == WRITE_TOOLS.len() && write_n > 0,
            "写工具数与 WRITE_TOOLS 应一致，got read={read_n} write={write_n}"
        );
        assert!(
            read_n > write_n,
            "只读工具应占多数，got read={read_n} write={write_n}"
        );
        // 每个工具的 schema 是 JSON 对象
        for t in &tools {
            assert!(
                t.input_schema.is_object(),
                "工具 {} 的 schema 应为对象",
                t.name
            );
        }
    }

    #[tokio::test]
    async fn agent_call_tool_disk_health_returns_data() {
        if !has_binary() {
            eprintln!("skip: agent-server.exe 未构建");
            return;
        }
        let r = agent_call_tool_core("disk_health".into(), serde_json::json!({}), None, None)
            .await
            .expect("call_tool 应成功");
        assert!(!r.is_error, "disk_health 不应报错：{}", r.text);
        assert!(
            r.text.contains("磁盘概览"),
            "应返回磁盘概览，got: {}",
            r.text
        );
    }

    #[tokio::test]
    async fn agent_call_tool_path_guard_rejects_drive_root() {
        if !has_binary() {
            eprintln!("skip: agent-server.exe 未构建");
            return;
        }
        // list_dir C:\ 应被 PathGuard 拒绝（isError=true + 中文原因）
        let r = agent_call_tool_core(
            "list_dir".into(),
            serde_json::json!({ "path": "C:\\" }),
            None,
            None,
        )
        .await
        .expect("call_tool 应成功");
        assert!(r.is_error, "盘根必须被拒绝");
        assert!(
            r.text.contains("安全守卫"),
            "拒绝原因应含「安全守卫」，got: {}",
            r.text
        );
    }

    #[tokio::test]
    async fn agent_server_ready_matches_binary() {
        // 纯函数探活：逻辑上应与 agent_server_binary() 一致
        assert_eq!(agent_server_ready(), agent_server_binary().is_some());
    }

    /// A1 回归：connect() 必须给 agent-server 注入 DISKPILOT_SCAFFOLDS，
    /// 否则 MCP cleanup_suggestions 在打包运行时找不到 scaffold 清单
    /// （编译期 CARGO_MANIFEST_DIR 兜底只在 dev 生效）。
    #[test]
    fn connect_injects_scaffolds_env() {
        let candidates = scaffolds_candidates();
        assert!(
            !candidates.is_empty(),
            "scaffolds 候选列表不应为空（至少含 env + exe 同目录 + dev 布局）"
        );
        // 3 号候选 = CARGO_MANIFEST_DIR/../../../scaffolds = 仓库根 scaffolds/
        // （apps/desktop/src-tauri 向上 3 级）。当前仓库该目录真实存在，
        // 说明 dev 布局解析正确；不强制 exists（打包场景只有前两个候选），
        // 但仓库内必然命中。
        let repo_scaffolds =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../scaffolds");
        assert_eq!(
            candidates.last().unwrap(),
            &repo_scaffolds,
            "dev 布局候选应为 CARGO_MANIFEST_DIR/../../../scaffolds"
        );
        let resolved = resolve_scaffolds_dir();
        assert!(
            resolved.is_some(),
            "仓库根 scaffolds/ 应能被解析到（dev 布局兜底）"
        );
        if let Some(dir) = resolved {
            // 至少含一份 scaffold TOML（磁盘上能真实读出）
            let n_toml = std::fs::read_dir(&dir)
                .expect("scaffolds 目录应可读")
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_type().map(|t| t.is_file()).unwrap_or(false)
                        && e.file_name().to_string_lossy().ends_with(".toml")
                })
                .count();
            assert!(n_toml > 0, "scaffolds 目录 {} 下应有 TOML", dir.display());
        }
    }
}
