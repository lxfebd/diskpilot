//! 文件/磁盘/进程/应用参数域（从 params.rs 拆分，2026-10-12）。
//! DiskHealthParams..AppLicensesParams。

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
