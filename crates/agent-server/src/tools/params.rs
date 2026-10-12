//! 全部 MCP 工具的入参 struct（参数 → 纯函数 → 格式化结果 薄壳的参数定义层）。
//!
//! 从 tools/mod.rs 拆分（2026-10-12）：参数 struct 是纯数据定义，与路由/方法体分离，
//! 按域分组（磁盘/文件/系统/网络/安全/硬件/基准/报告/盘点）。mod.rs 里以
//! `mod params;` + `use params::*;` 引入（原 impl 体内引用各 struct 的点不变）。
//!
//! 注意：宏 `#[tool]` 参数类型引用这些 struct 时走 `crate::tools::params::xxx`，
//! rmcp 要求类型在宏展开处可见，故 `mod params` 声明为同 crate 内（无 pub 即可）。
//! `schemars::JsonSchema` 是 derive 里的完整路径，无需 `use schemars`。

use serde::Deserialize;

use crate::tools::lenient;

// ── 参数 struct（放在 impl 外，宏需要见 `struct` 定义） ─────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DiskHealthParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct PartitionUsageParams {
    #[schemars(description = "要查的路径（返回该路径所在卷的用量），如 C:\\Users")]
    pub(crate) path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct VolumeMetaParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FileTypeStatsParams {
    #[schemars(description = "要统计的目录绝对路径，如 C:\\Users\\me\\Downloads")]
    pub(crate) path: String,
    #[schemars(description = "返回前 N 大占用扩展名，默认 20")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FileTreeParams {
    #[schemars(description = "目录绝对路径，如 C:\\Users\\me\\Downloads")]
    pub(crate) path: String,
    #[schemars(description = "目录树最大深度（默认 3，上限 10）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) max_depth: Option<usize>,
    #[schemars(description = "最多返回多少节点（默认 100，上限 500）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) max_nodes: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SystemInfoParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListDirParams {
    #[schemars(description = "目录绝对路径，如 C:\\Users\\me\\Downloads")]
    pub(crate) path: String,
    #[schemars(description = "最多返回多少项，默认 100")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ReadFileParams {
    #[schemars(description = "文件绝对路径，如 C:\\Users\\me\\Documents\\notes.txt")]
    pub(crate) path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FindFilesParams {
    #[schemars(description = "搜索根目录绝对路径，如 C:\\Users\\me")]
    pub(crate) root: String,
    #[schemars(description = "文件名关键字（大小写不敏感），如 report")]
    pub(crate) keyword: String,
    #[schemars(description = "最多返回多少条命中，默认 50")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) max_hits: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListProcessesParams {
    #[schemars(description = "最多返回多少个进程（按内存降序截断），默认 50")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ProcessInfoParams {
    #[schemars(description = "进程 PID，如 1234")]
    pub(crate) pid: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ProcessKillParams {
    #[schemars(description = "要结束的进程 PID，如 1234（系统关键进程会被守卫拒绝）")]
    pub(crate) pid: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ProcessStartParams {
    #[schemars(
        description = "要启动的程序的绝对路径（必须 .exe，且在系统目录或已安装程序目录下，如 C:\\Windows\\System32\\notepad.exe）"
    )]
    pub(crate) command: String,
    #[schemars(description = "传给程序的参数列表（可选；原样传入，不拼 shell 字符串）")]
    pub(crate) args: Option<Vec<String>>,
    #[schemars(description = "进程工作目录（可选；必须是已存在的目录，不存在就用默认）")]
    pub(crate) cwd: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FileRecycleParams {
    #[schemars(
        description = "要移入回收站的文件/目录绝对路径（盘根/系统目录/用户主目录根会被守卫拒绝）"
    )]
    pub(crate) path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ScheduledTaskManageParams {
    #[schemars(
        description = "计划任务名（先调 sys_boot_items include_scheduled=true 确认名称；系统内置任务会被守卫拒绝修改）"
    )]
    pub(crate) name: String,
    #[schemars(
        description = "动作：enable 启用 / disable 禁用 / query 只读查询状态（不开放 delete/run）"
    )]
    pub(crate) action: String,
    #[schemars(description = "只出计划不执行（默认 false）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AppListParams {
    #[schemars(description = "按名称/发布者关键字过滤（可选），如 chrome")]
    pub(crate) keyword: Option<String>,
    #[schemars(description = "最多返回多少个程序（默认 50，上限 200）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct UninstallAppParams {
    #[schemars(
        description = "要卸载的已安装程序名（DisplayName，先调 app_list 确认，大小写不敏感）"
    )]
    pub(crate) name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AppLicensesParams {}

// ── 硬件（hw.rs）参数 ─────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwCpuParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwGpuParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwMemoryParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwMotherboardParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwTemperatureParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwSuperioParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwSensorsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwBatteryParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwDiskSmartParams {}

// ── 网络（net.rs）参数 ────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetStatusParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetAdapterDetailParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetConnectionsParams {
    #[schemars(description = "按进程 PID 过滤（可选），如 1234")]
    #[serde(default, deserialize_with = "lenient::opt_u32")]
    pub(crate) pid: Option<u32>,
    #[schemars(description = "按关键字过滤（可选）：匹配进程名或远端 IP，大小写不敏感")]
    pub(crate) keyword: Option<String>,
    #[schemars(description = "最多返回多少条（默认 30，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetSpeedParams {
    #[schemars(description = "采样窗口毫秒（1000-10000，默认 2000）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) interval_ms: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetShareParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct NetWifiParams {}

// ── 系统服务/驱动/启动（sys.rs）参数 ──────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SysServicesParams {
    #[schemars(description = "按服务名/显示名关键字过滤（可选），如 sql")]
    pub(crate) keyword: Option<String>,
    #[schemars(description = "是否包含已禁用的服务（默认 false 只看启用/运行中）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) show_disabled: Option<bool>,
    #[schemars(description = "最多返回多少个服务（默认 50，上限 200）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ServiceControlParams {
    #[schemars(description = "服务名（先调 sys_services 确认名字），如 spooler")]
    pub(crate) name: String,
    #[schemars(description = "动作：start 启动 / stop 停止 / restart 重启")]
    pub(crate) action: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SysDriversParams {
    #[schemars(description = "按驱动名关键字过滤（可选），如 nvidia")]
    pub(crate) keyword: Option<String>,
    #[schemars(description = "最多返回多少个驱动（默认 50，上限 200）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SysBootItemsParams {
    #[schemars(description = "是否附带读取计划任务启动项（较慢，默认 false）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) include_scheduled: Option<bool>,
}

// ── 安全/审计（sec.rs）参数 ───────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct EventLogsParams {
    #[schemars(description = "日志名（逗号分隔），如 System 或 System,Application（默认 System）")]
    pub(crate) logs: Option<String>,
    #[schemars(description = "回溯小时数（默认 24，上限 720）")]
    #[serde(default, deserialize_with = "lenient::opt_u32")]
    pub(crate) hours: Option<u32>,
    #[schemars(description = "每个日志最多返回多少条（默认 20，上限 50）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) max_events: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FirewallRulesParams {
    #[schemars(description = "方向过滤：Inbound / Outbound / Any（默认 Inbound）")]
    pub(crate) direction: Option<String>,
    #[schemars(description = "动作过滤：Allow / Block / Any（默认 Allow）")]
    pub(crate) action: Option<String>,
    #[schemars(description = "最多返回多少条（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct LoginEventsParams {
    #[schemars(description = "最多返回多少条（默认 20，上限 50）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) max_events: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DefenderStatusParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct UserAccountsParams {}

// ── 磁盘深入（diskx.rs）参数 ──────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct BiggestFilesParams {
    #[schemars(description = "要搜索的目录绝对路径，如 C:\\Users\\me\\Downloads")]
    pub(crate) path: String,
    #[schemars(description = "最多返回多少个文件（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DuplicateFilesParams {
    #[schemars(description = "要搜索的目录绝对路径，如 C:\\Users\\me\\Downloads")]
    pub(crate) path: String,
    #[schemars(description = "最多返回多少组重复（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DiskIoUsageParams {
    #[schemars(description = "采样窗口毫秒（默认 1000）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) sample_ms: Option<u64>,
}

// ── 自修复（selfheal.rs）参数 ─────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FanSelfhealDiagParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FanSelfhealFixParams {
    #[schemars(
        description = "修复动作：install_driver（安装 inpoutx64 驱动，需管理员）/ reset_fan <idx>（恢复风扇 idx 主板控制）"
    )]
    pub(crate) action: String,
    #[schemars(
        description = "只出计划不执行（默认 true）。AI 先 dry-run 给用户确认后再传 false 真正执行"
    )]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FanControlParams {
    #[schemars(description = "风扇索引（数字，先 fan_selfheal_diag 查 fans 列表确认）")]
    pub(crate) idx: String,
    #[schemars(description = "目标转速百分比 0-100（越界自动钳制）")]
    pub(crate) pct: f64,
    #[schemars(
        description = "只出计划不执行（默认 true）。AI 先 dry-run 给用户确认后再传 false 真正执行"
    )]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ToolbeltListParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ToolbeltRunParams {
    #[schemars(
        description = "工具箱工具名（先调 toolbelt_list 确认名称），如 crystaldiskinfo / wiztree / defraggler"
    )]
    pub(crate) tool: String,
    #[schemars(
        description = "传给工具的 CLI 参数列表（可选，透传不拼 shell；空则用工具默认参数）"
    )]
    pub(crate) args: Option<Vec<String>>,
    #[schemars(
        description = "用户已确认执行（medium/high 风险必须 true，low 风险可省略）。AI 应先把要执行的命令展示给用户，用户同意后再传 true"
    )]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) confirmed: Option<bool>,
    #[schemars(description = "执行超时秒数（默认 30，上限 300；超过强制终止）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) timeout_secs: Option<u64>,
}

// ── 阶段二：基准 + 压测（bench.rs）参数 ──────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct BenchCpuParams {
    #[schemars(description = "每项基准时长秒（默认 3，上限 30）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) secs: Option<u64>,
    #[schemars(description = "线程数（默认全部可用，上限 2 倍）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) threads: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct BenchMemoryParams {
    #[schemars(description = "每项基准时长秒（默认 3，上限 20）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) secs: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct BenchDiskParams {
    #[schemars(
        description = "测试目录（默认系统临时目录；必须临时目录或用户指定，绝不碰用户数据）"
    )]
    pub(crate) path: Option<String>,
    #[schemars(description = "测试文件大小 MB（默认 256，16-2048）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) size_mb: Option<u64>,
    #[schemars(description = "只出计划不创建文件（默认 true）；用户确认后可传 false 真正跑")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct StressTestParams {
    #[schemars(description = "压测时长秒（默认 5 演示，上限 600）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) secs: Option<u64>,
    #[schemars(description = "超温熔断阈值℃（默认 95，60-110）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    pub(crate) max_temp: Option<f64>,
    #[schemars(description = "线程数（默认全部可用）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) threads: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct StressTestGpuParams {
    #[schemars(description = "温度熔断守门监控秒数（默认 5，上限 120）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) secs: Option<u64>,
    #[schemars(description = "GPU 超温熔断阈值℃（默认 90，60-110）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    pub(crate) max_temp: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct MemTestParams {
    #[schemars(description = "测试缓冲大小 MB（默认 256，64-4096）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    pub(crate) size_mb: Option<u64>,
    #[schemars(description = "测试轮数（默认 1，1-10）")]
    #[serde(default, deserialize_with = "lenient::opt_u32")]
    pub(crate) rounds: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct BsodAnalyzeParams {
    #[schemars(description = "最多解析几个转储（默认 10，上限 30）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) max_dumps: Option<usize>,
    #[schemars(description = "是否读根目录 MEMORY.DMP（默认 true）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) include_memory_dmp: Option<bool>,
}

// ── 阶段三：体检报告 / 传感器趋势 / 阈值告警（report.rs）参数 ────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SystemReportParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SensorTrendParams {
    #[schemars(description = "返回最近 N 条趋势（默认 10，上限 50）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) n: Option<usize>,
    #[schemars(description = "输出格式：text（默认，人类可读）/ json（结构化数组，供前端折线图）")]
    pub(crate) format: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SensorAlertParams {
    #[schemars(description = "CPU 温度阈值℃（默认 85，50-110）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    pub(crate) cpu_temp: Option<f64>,
    #[schemars(description = "磁盘温度阈值℃（默认 55，30-90）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    pub(crate) disk_temp: Option<f64>,
    #[schemars(description = "GPU 温度阈值℃（默认 85，50-110）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    pub(crate) gpu_temp: Option<f64>,
    #[schemars(description = "内存占用阈值%（默认 90，50-100）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    pub(crate) mem_percent: Option<f64>,
    #[schemars(description = "磁盘剩余阈值 GB（默认 20，1-500）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    pub(crate) disk_free_gb: Option<f64>,
    #[schemars(description = "把告警写历史 JSONL（默认 false 只读体检）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    pub(crate) persist: Option<bool>,
}

// ── 阶段四：信息补全（hw.rs 扩展）参数 ───────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwCpuFeaturesParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwDramTimingsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwDisplaysParams {}

// ── 阶段五：长尾（bench_gpu / IPMI / ACPI）参数 ────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct BenchGpuParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwIpmiParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct HwAcpiParams {}

// ── 盘点/建议（steam.rs + cleanup.rs）参数 ────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SteamGamesParams {
    #[schemars(description = "每个库最多列出多少个游戏（按大小降序，默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct CleanupSuggestionsParams {
    #[schemars(
        description = "要统计的目录绝对路径：**通常直接传盘根**（如 C:\\），本工具为只读统计允许盘根；也可传盘下任意目录（系统目录/用户主目录根仍会被守卫拒绝）"
    )]
    pub(crate) root: String,
    #[schemars(description = "返回按可清理字节降序的 Top N 个项（默认 20，上限 50）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct TopDirectoriesParams {
    #[schemars(description = "要统计的目录绝对路径，如 C:\\Users\\me\\Downloads")]
    pub(crate) path: String,
    #[schemars(description = "返回最大的 N 个子目录（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    pub(crate) top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ProcessCpuUsageParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct UsbDevicesParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DiskSmartRawParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RecycleBinStatsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct EnvVarsParams {
    pub(crate) name: Option<String>,
}
