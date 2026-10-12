//! 网络/安全/游戏/清理/工具包域工具处理器（从 tools/mod.rs 拆分，2026-10-12）。

use crate::tools::params::*;
use crate::tools::{blocking_call, text_result, tool_error, AgentServer};
use crate::tools::{cleanup, files, net, process, sec, steam, toolbelt};
use diskpilot_toolbelt::Risk;
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

#[tool_router(router = tool_router_misc, vis = "pub(crate)")]
impl AgentServer {
    #[tool(
        description = "读取本机网络状态：每个网卡名称/连接状态/IP/链接速率/是否虚拟（只读，不需要管理员权限）。用户问『电脑联网了吗/网卡是什么/IP 多少/网速多少』时调用。"
    )]
    async fn net_status(
        &self,
        _p: Parameters<NetStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, net::collect_net_status, "网络状态读取").await)
    }

    #[tool(
        description = "读取网络连接列表：本地/远端地址与端口/状态/所属进程（只读，不需要管理员权限）。默认只看已建立的连接；可按 PID 或关键字（进程名/IP）过滤，也可查看全部。用户问『谁在连网/哪个程序在联网/某某连接是什么』时调用。"
    )]
    async fn net_connections(
        &self,
        Parameters(p): Parameters<NetConnectionsParams>,
    ) -> Result<CallToolResult, McpError> {
        let pid = p.pid;
        let keyword = p.keyword.clone();
        let top_n = p.top_n.unwrap_or(30);
        Ok(blocking_call(
            45,
            move || net::collect_net_connections(pid, keyword, top_n),
            "网络连接列表读取",
        )
        .await)
    }

    #[tool(
        description = "实测网络实时速率：采集 interval_ms 毫秒（1000-10000，默认 2000）窗口内的收发流量并换算为 KB/s（只读，需要跑一次 PowerShell 采样，约 2-10 秒）。用户问『当前网速快不快/下载多少兆』时调用。"
    )]
    async fn net_speed(
        &self,
        Parameters(p): Parameters<NetSpeedParams>,
    ) -> Result<CallToolResult, McpError> {
        let interval_ms = p.interval_ms.unwrap_or(2000);
        Ok(blocking_call(
            45,
            move || net::collect_net_speed(interval_ms),
            "网络速率采集",
        )
        .await)
    }

    #[tool(
        description = "读取本机共享文件夹（SMB 共享）列表：共享名/路径/权限类型（只读；会话/打开文件统计需要管理员权限，非管理员会明确降级说明）。用户问『电脑共享了什么文件夹/开了哪些共享』时调用。"
    )]
    async fn net_share(&self, _p: Parameters<NetShareParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, net::collect_net_share, "共享文件夹读取").await)
    }

    #[tool(
        description = "读取无线网络信息：本机无线网卡状态 + 已保存的 WiFi 网络名称列表（绝不读取密码，只读）。用户问『WiFi 连的是什么/保存了哪些 WiFi』时调用；无无线网卡会明确说明。"
    )]
    async fn net_wifi(&self, _p: Parameters<NetWifiParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, net::collect_net_wifi, "WiFi 信息读取").await)
    }

    #[tool(
        description = "读取每张网卡的详细配置：状态/速率/逐 IP 地址（IPv4 带子网掩码、IPv6 带前缀长度）/MAC 地址/网关/DNS 服务器（只读，不需要管理员权限）。用户问『我的 IP 是什么/子网掩码是多少/MAC 地址/网关 DNS』时调用。"
    )]
    async fn net_adapter_detail(
        &self,
        _p: Parameters<NetAdapterDetailParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, net::collect_adapter_detail, "网卡配置读取").await)
    }

    // ── 系统服务/驱动/启动（sys.rs）───────────────────────────────

    #[tool(
        description = "盘点本机 Steam 游戏库：库根/每库游戏数/总占用 + 各游戏（appid/名称/安装目录/大小/上次游玩/是否幽灵安装/是否建议清理及原因）。只读（注册表定位 Steam + 读 ACF 清单）。用户问『装了哪些 Steam 游戏/哪个游戏占地/Steam 该清谁』时调用。"
    )]
    async fn steam_games(
        &self,
        Parameters(p): Parameters<SteamGamesParams>,
    ) -> Result<CallToolResult, McpError> {
        let top_n = p.top_n.unwrap_or(20);
        Ok(blocking_call(45, move || steam::collect_steam_games(top_n), "Steam 盘点").await)
    }

    #[tool(
        description = "按软件分类给出某目录（**必须传盘根，如 C:\\**）的可清理建议：统计各已知软件（scaffold）的缓存/临时目录占用（只读，单遍并行遍历 + glob 匹配）。**只出建议，绝不删除任何文件**（执行需用户确认走主项目清理流程）。用户问『C 盘/某盘有什么可清的/微信 QQ 缓存占了多少』时调用；未命中已知软件会明确说明。"
    )]
    async fn cleanup_suggestions(
        &self,
        Parameters(p): Parameters<CleanupSuggestionsParams>,
    ) -> Result<CallToolResult, McpError> {
        let root = p.root.clone();
        let top_n = p.top_n.unwrap_or(20);
        Ok(blocking_call(
            120,
            move || cleanup::collect_cleanup_suggestions(root, top_n),
            "清理建议计算",
        )
        .await)
    }

    #[tool(
        description = "读取进程 CPU 占用 Top 30（同步采样窗口 200ms：两次 GetProcessTimes 差值，只读）。占用率为单核百分比（多核满载 = 核数 × 100%）。用户问『哪个进程吃 CPU/为什么卡/谁在烧 CPU』时调用，与 list_processes（内存视角）互补。"
    )]
    async fn process_cpu_usage(
        &self,
        _p: Parameters<ProcessCpuUsageParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, process::collect_cpu_usage, "CPU 占用采样").await)
    }

    #[tool(
        description = "读取系统事件日志：指定日志名（逗号分隔，默认 System）、最近 hours 小时、最多 max_events 条（只读）。安全日志无管理员权限会降级说明。用户问『最近系统出了什么错误/蓝屏/警告/日志』时调用。"
    )]
    async fn security_event_logs(
        &self,
        Parameters(p): Parameters<EventLogsParams>,
    ) -> Result<CallToolResult, McpError> {
        let logs = p.logs.clone();
        let hours = p.hours.unwrap_or(24);
        let max_events = p.max_events.unwrap_or(20);
        Ok(blocking_call(
            45,
            move || sec::collect_event_logs(logs, hours, max_events),
            "事件日志读取",
        )
        .await)
    }

    #[tool(
        description = "列出防火墙规则：方向（默认 Inbound 入站）与动作（默认 Allow 允许）过滤，最多 top_n 条（只读，不需要管理员权限）。用户问『防火墙开了什么/某程序被放行了吗』时调用。"
    )]
    async fn security_firewall_rules(
        &self,
        Parameters(p): Parameters<FirewallRulesParams>,
    ) -> Result<CallToolResult, McpError> {
        let direction = p.direction.clone();
        let action = p.action.clone();
        let top_n = p.top_n.unwrap_or(20);
        Ok(blocking_call(
            45,
            move || sec::collect_firewall_rules(direction, action, top_n),
            "防火墙规则读取",
        )
        .await)
    }

    #[tool(
        description = "读取最近的登录/注销事件（安全日志 4624/4625 + PowerShell 脚本块日志 4104，只读）。需要管理员权限才读得到安全日志，无权限会明确降级。用户问『谁登录过这台电脑/有没有异常登录/有没有人跑过脚本』时调用。"
    )]
    async fn security_login_events(
        &self,
        Parameters(p): Parameters<LoginEventsParams>,
    ) -> Result<CallToolResult, McpError> {
        let max_events = p.max_events.unwrap_or(20);
        Ok(blocking_call(
            45,
            move || sec::collect_login_events(max_events),
            "登录事件读取",
        )
        .await)
    }

    #[tool(
        description = "列出图吧工具箱集成工具清单（只读 L0）：工具名/分类/CLI|GUI/权限级/风险/用途。AI 判断『这个场景该用工具箱哪个工具』时调用，或先于 toolbelt_run 查询可用工具。CLI 工具（A 类）可被 AI 直接执行，GUI 工具（B 类）只能启动提示用户。"
    )]
    async fn toolbelt_list(
        &self,
        _p: Parameters<ToolbeltListParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || Ok(toolbelt::list_tools()), "工具箱清单读取").await)
    }

    #[tool(
        description = "执行图吧工具箱的 CLI 工具（**写操作 L2**，三层防线：L3 永久禁止如 FPT64 刷 BIOS；medium/high 风险必须 confirmed=true 用户确认；cli.run 权限未开启拒绝）。参数 tool=工具名（先 toolbelt_list 确认名称，如 crystaldiskinfo/wiztree）、args=参数列表（透传不拼 shell，空则用工具默认）、confirmed=用户已确认（medium+ 风险必须 true）、timeout_secs=超时秒（默认 30）。**此工具会启动外部程序，主项目侧会先弹确认门**。AI 在用户明确要求『用 XX 工具跑一下』时调用；只读工具（风险 low 如 CrystalDiskInfo/WizTree）可不确认，写类（风险 medium/high）必须先展示命令给用户确认再传 confirmed=true。"
    )]
    async fn toolbelt_run(
        &self,
        Parameters(p): Parameters<ToolbeltRunParams>,
    ) -> Result<CallToolResult, McpError> {
        let tool = p.tool.clone();
        let args = p.args.clone().unwrap_or_default();
        let confirmed = p.confirmed.unwrap_or(false);
        let timeout = p.timeout_secs;
        // 反绕过确认门只对 write 类（medium/high 风险）生效——low 风险只读工具
        // （CrystalDiskInfo/WizTree 等）本就无副作用、可按描述裸跑，不必过桥。
        // L3 永禁 / medium/high 的 confirmed 校验仍由 run_tool 内二次兜底。
        let needs_gate = diskpilot_toolbelt::find_manifest(&tool)
            .and_then(|m| diskpilot_toolbelt::Risk::parse(&m.risk))
            .map(|r| r >= Risk::Medium)
            .unwrap_or(true); // 未知/无 manifest 一律从严过门
        if needs_gate {
            if let Err(e) = files::require_confirmed() {
                return Ok(tool_error(e));
            }
        }
        match tokio::task::spawn_blocking(move || {
            toolbelt::run_tool(&tool, &args, confirmed, timeout)
        })
        .await
        {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("工具箱执行任务崩溃: {e}"))),
        }
    }

    // ── 阶段二：AIDA64 核心复刻 —— 基准 + 压测（bench.rs） ──────────────
}
