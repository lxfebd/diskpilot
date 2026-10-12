//! 体检报告/传感器/自愈域（从 tool_hw.rs 拆分，2026-10-12）。
//! system_report / sensor_trend / sensor_alert / fan_selfheal_diag /
//! fan_selfheal_fix / fan_control。

use crate::tools::params::*;
use crate::tools::{
    blocking_call, blocking_call_progress, files, report, selfheal, tool_error, AgentServer,
};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

#[tool_router(router = tool_router_hw_report, vis = "pub(crate)")]
impl AgentServer {
    #[tool(
        description = "一键系统体检报告（只读 L0，复刻 AIDA64 Report）：汇总 CPU/GPU/内存/主板/温度/磁盘健康/系统/电池全部只读采集，输出 Markdown 报告。AI 在用户问『体检/电脑状态总结/报告』时调用，拿它当结论依据。"
    )]
    async fn system_report(
        &self,
        _p: Parameters<SystemReportParams>,
        ctx: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call_progress(
            120,
            move |slot| {
                let cb = |done: usize, total: usize| {
                    if let Some(v) = (done * 1000).checked_div(total) {
                        slot.set(v as u64);
                    }
                };
                report::system_report(Some(&cb))
            },
            "体检报告生成",
            &ctx,
        )
        .await)
    }

    #[tool(
        description = "传感器趋势（只读 L0，写应用自有历史文件）：追加一条当前传感器快照（含 CPU 最高温）到历史 JSONL（%LOCALAPPDATA%/diskpilot/agent-server/sensor-history.jsonl，追加式 + 自动裁剪 500 条——只写应用自己的数据目录，不改不动用户文件），返回最近 n 条（默认 10，上限 50）。format=json 时返回结构化数组供前端折线图。AI 在用户问『温度趋势/散热表现』时调用（多次调用可见随时间变化）。"
    )]
    async fn sensor_trend(
        &self,
        Parameters(p): Parameters<SensorTrendParams>,
    ) -> Result<CallToolResult, McpError> {
        let n = p.n;
        let format = p.format;
        Ok(blocking_call(45, move || report::sensor_trend(n, format), "传感器趋势").await)
    }

    #[tool(
        description = "传感器阈值告警（只读 L0，persist=true 时写应用自有告警日志）：体检 CPU 温度/磁盘温度/GPU 温度/内存占用/磁盘剩余是否超阈值，输出告警清单（只读采集 + 阈值比较，无系统改动）。参数 cpu_temp=CPU 温度阈值℃（默认 85）、disk_temp=磁盘温度阈值℃（默认 55）、gpu_temp=GPU 阈值℃（默认 85）、mem_percent=内存占用阈值%（默认 90）、disk_free_gb=磁盘剩余阈值 GB（默认 20）、persist=是否把告警写历史到 %LOCALAPPDATA%/diskpilot/agent-server/sensor-alerts.jsonl（默认 false，只写应用自有数据目录）。AI 在用户问『电脑状态健康吗/该注意什么』时调用。"
    )]
    async fn sensor_alert(
        &self,
        Parameters(p): Parameters<SensorAlertParams>,
    ) -> Result<CallToolResult, McpError> {
        let cpu_temp = p.cpu_temp;
        let disk_temp = p.disk_temp;
        let gpu_temp = p.gpu_temp;
        let mem_percent = p.mem_percent;
        let disk_free_gb = p.disk_free_gb;
        let persist = p.persist;
        Ok(blocking_call(
            45,
            move || {
                report::sensor_alert(
                    cpu_temp,
                    disk_temp,
                    gpu_temp,
                    mem_percent,
                    disk_free_gb,
                    persist,
                )
            },
            "传感器告警",
        )
        .await)
    }

    // ── 阶段四：信息补全（hw.rs 扩展） ───────────────────────────────────

    #[tool(
        description = "诊断风扇写控通道（只读 L0）：调用 fancmd diag json，返回驱动是否加载/驱动名/RTC 端口验证/SuperIO 芯片与 env_base/风扇列表/通道可写性，并给出明确的可修复点清单（缺驱动？芯片不可识别？可写但需 reset？）。用户在其他机器上发现『风扇控制不可用』时，先调这个拿诊断，再决定是否让用户授权 fan_selfheal_fix 修复。"
    )]
    async fn fan_selfheal_diag(
        &self,
        _p: Parameters<FanSelfhealDiagParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, selfheal::selfheal_diag, "风扇通道诊断").await)
    }

    #[tool(
        description = "修复风扇写控通道（**写操作 L2**，由主项目权限中心确认门 + 管理员权限双重守护）。action 可选：install_driver=安装并启动 inpoutx64 端口驱动（需管理员，复制 DLL 到 System32 + sc create/start，绝不覆盖已存在文件）；reset_fan <idx>=恢复指定风扇为主板自动控制（安全可逆，不需管理员）。dry_run=true 只出计划不执行。AI 应先跑 fan_selfheal_diag 确认根因，再用本工具修复，并明确告知用户要做的操作、请用户确认。"
    )]
    async fn fan_selfheal_fix(
        &self,
        Parameters(p): Parameters<FanSelfhealFixParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let action = p.action.clone();
        let dry_run = p.dry_run.unwrap_or(true);
        Ok(blocking_call(
            60,
            move || selfheal::selfheal_fix(&action, dry_run),
            "风扇自修复",
        )
        .await)
    }

    #[tool(
        description = "直接调速风扇（**写操作 L2**，由主项目权限中心确认门守护）：把指定风扇设为软件控制的固定转速百分比。参数 idx=风扇索引（**字符串**，先用 fan_selfheal_diag 查 fans 列表确认，如 \"0\"）、pct=目标转速 0-100（数字，越界自动钳制）。执行前自动 diag 校验写通道（驱动未加载/芯片不可识别时拒绝）。dry_run=true 只出计划不执行。软件接管后可随时用 fan_selfheal_fix action=reset_fan <idx> 恢复主板自动控制。AI 应先 fan_selfheal_diag 看可写性与风扇列表，dry-run 给用户确认后再传 dry_run=false 真正调速。"
    )]
    async fn fan_control(
        &self,
        Parameters(p): Parameters<FanControlParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let idx = p.idx.clone();
        let pct = p.pct;
        let dry_run = p.dry_run.unwrap_or(true);
        Ok(blocking_call(
            30,
            move || selfheal::fan_control(&idx, pct, dry_run),
            "风扇调速",
        )
        .await)
    }
}
