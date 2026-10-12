//! 基准/压测/报告参数域（从 params.rs 拆分，2026-10-12）。
//! BenchCpuParams..SensorAlertParams。

use serde::Deserialize;

use crate::tools::lenient;

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
