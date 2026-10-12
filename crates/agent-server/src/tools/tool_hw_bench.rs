//! 基准/压力/蓝屏分析域（从 tool_hw.rs 拆分，2026-10-12）。
//! hw_ipmi / hw_acpi / bench_cpu / bench_memory / bench_disk / bench_gpu /
//! stress_test / stress_test_gpu / stress_cancel / mem_test / bsod_analyze。

use crate::tools::params::*;
use crate::tools::{bench, blocking_call, bsod, files, hw, text_result, tool_error, AgentServer};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

#[tool_router(router = tool_router_hw_bench, vis = "pub(crate)")]
impl AgentServer {
    #[tool(
        description = "IPMI 服务器接口探测（只读 L0，复刻 AIDA64 IPMI）：检测主板是否带 BMC/IPMI 接口（服务器/HEDT 板才有，家用台式机通常无——如实返回未检测到，不编造传感器）。检测到时列出接口实例。AI 在用户问『这板子支不支持 IPMI/远程管理』时调用。"
    )]
    async fn hw_ipmi(&self, _p: Parameters<HwIpmiParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_ipmi, "IPMI 探测").await)
    }

    #[tool(
        description = "ACPI 固件信息（只读 L0，复刻 AIDA64 ACPI Browser 简化版）：BIOS 厂商/版本/发布日期/序列号 + 机箱厂商/型号/系统类型。完整 ACPI 表浏览（FADT/DSDT 反汇编）需 Ring0 工具，属专业向，这里给到 WMI 可读的固件/电源顶层信息。AI 在用户问『BIOS 版本/主板固件/ACPI』时调用。"
    )]
    async fn hw_acpi(&self, _p: Parameters<HwAcpiParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_acpi, "ACPI 采集").await)
    }
    #[tool(
        description = "CPU 基准（只读 L0，复刻 AIDA64 CPU Queen/FPU Julia）：多线程整数（质数筛）+ 浮点（π 级数）吞吐测试。参数 secs=每项时长秒（默认 3，上限 30）、threads=线程数（默认全部可用）。纯计算不写盘，几秒出结果。AI 在用户问『CPU 性能/跑分』时调用。"
    )]
    async fn bench_cpu(
        &self,
        Parameters(p): Parameters<BenchCpuParams>,
    ) -> Result<CallToolResult, McpError> {
        let secs = p.secs;
        let threads = p.threads;
        match tokio::task::spawn_blocking(move || bench::bench_cpu(secs, threads)).await {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("CPU 基准任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "内存基准（只读 L0，复刻 AIDA64 Cache & Memory Benchmark）：读/写/复制带宽（MB/s）+ 延迟（指针追逐 ns）。参数 secs=每项时长秒（默认 3，上限 20）。纯只读（写基准写自身堆内存），几秒出结果。AI 在用户问『内存性能/带宽/延迟』时调用。"
    )]
    async fn bench_memory(
        &self,
        Parameters(p): Parameters<BenchMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let secs = p.secs;
        match tokio::task::spawn_blocking(move || bench::bench_memory(secs)).await {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("内存基准任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "磁盘基准（只读 L0 为主，复刻 AIDA64 Disk Benchmark）：顺序/随机读 + 顺序/随机写（MB/s）。参数 path=测试目录（默认系统临时目录，**必须用临时目录或用户指定目录，绝不碰用户数据**）、size_mb=测试文件大小（默认 256，16-2048）、dry_run=只出计划不创建文件（默认 true）。dry_run=true 先给 AI 看计划；用户同意后再传 dry_run=false 真正跑。测完临时文件立即删除。AI 在用户问『磁盘速度/读写性能』时调用。"
    )]
    async fn bench_disk(
        &self,
        Parameters(p): Parameters<BenchDiskParams>,
    ) -> Result<CallToolResult, McpError> {
        let dry_run = p.dry_run.unwrap_or(true);
        // 真跑（dry_run=false）会在目标目录创建临时文件写入——写操作，必须过确认门
        if !dry_run {
            if let Err(e) = files::require_confirmed() {
                return Ok(tool_error(e));
            }
        }
        let path = p.path.clone();
        let size_mb = p.size_mb;
        match tokio::task::spawn_blocking(move || bench::bench_disk(path, size_mb, Some(dry_run)))
            .await
        {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("磁盘基准任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "GPU 快照（只读 L0，复刻 AIDA64 GPGPU Benchmark 的只读部分）：显卡型号/显存/驱动/分辨率/温度（N 卡）。深度 GPGPU 基准（像素填充率/OpenCL）需 GPU 厂商 SDK，本工具集未实现，如实标注。AI 在用户问『显卡参数/驱动/温度』或要求 GPU 基准时先调用它。"
    )]
    async fn bench_gpu(&self, _p: Parameters<BenchGpuParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_gpu_snapshot, "GPU 快照采集").await)
    }

    #[tool(
        description = "稳定性压测（**写操作 L2**，复刻 AIDA64 System Stability Test）：CPU 多线程满载压测，期间每 1s 采样 CPU 温度，超过 max_temp 阈值自动熔断停机保护硬件。参数 secs=时长秒（默认 5 演示，上限 600）、max_temp=熔断阈值℃（默认 95，60-110）、threads=线程数（默认全部可用）。纯计算不写盘。**满负荷压测会大幅升温/占 CPU，AI 必须先告知用户风险并等用户确认（主项目确认门），用户同意后才调用**。压测结束时给出峰值温度与是否熔断。"
    )]
    async fn stress_test(
        &self,
        Parameters(p): Parameters<StressTestParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let secs = p.secs;
        let max_temp = p.max_temp;
        let threads = p.threads;
        match tokio::task::spawn_blocking(move || bench::stress_test(secs, max_temp, threads)).await
        {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("压测任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "GPU 稳定性压测（**写操作 L1**，复刻 FurMark 的温度熔断守门版）：纯后端无 OpenCL/CUDA/WebGL 引擎无法让 GPU 满载（深度压载需厂商 SDK，同 GPGPU 基准结论），本工具承担温度熔断守门——你正在用前端甜甜圈压测页 / 外挂 FurMark / 游戏 / 渲染任务做 GPU 负载时，本工具在 secs 秒内持续采样 GPU 温度（N 卡 nvidia-smi），超 max_temp 即报熔断信号保护硬件。参数 secs=监控秒数（默认 5，上限 120）、max_temp=熔断阈值℃（默认 90，60-110）。无 N 卡/无 nvidia-smi 时如实降级「无温度护栏」。hw.stress 权限门，需用户确认。"
    )]
    async fn stress_test_gpu(
        &self,
        Parameters(p): Parameters<StressTestGpuParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let secs = p.secs;
        let max_temp = p.max_temp;
        match tokio::task::spawn_blocking(move || bench::stress_test_gpu(secs, max_temp)).await {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("GPU 压测任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "停止当前压测（只读 L0 控制信号，无参数）：向正在运行的 stress_test / stress_test_gpu 发送停止信号（进程内 CANCEL flag + 跨进程停止文件，两种通道任一命中即退出），压测循环检测到后立即退出并报告「已按停止信号结束」。前端「停止」按钮 / 用户在 AI 对话里说『停』时调用。无压测运行时调用是安全空操作（返回『当前无压测在跑』）。"
    )]
    async fn stress_cancel(&self) -> Result<CallToolResult, McpError> {
        let had = bench::stress_cancel();
        Ok(text_result(if had {
            "已发送停止信号：正在运行的压测将在下一次采样循环检测后退出。".to_string()
        } else {
            "当前无压测在运行（停止信号为空操作）。".to_string()
        }))
    }

    #[tool(
        description = "内存稳定性测试（只读 L0，MemTest86 简化版）：进程内分配 size_mb 缓冲，用 全零/全一/AA55/递增/伪随机 5 种 pattern 反复写入并读回校验，发现位翻转/不稳定内存。参数 size_mb=缓冲大小（默认 256，64-4096）、rounds=轮数（默认 1，1-10）。纯进程堆读写，无磁盘副作用。用于『内存条稳不稳/超频是否稳定』场景；注意只能覆盖进程寻址内存，无法替代 pre-boot 版 MemTest86 的硬件寻址层。"
    )]
    async fn mem_test(
        &self,
        Parameters(p): Parameters<MemTestParams>,
    ) -> Result<CallToolResult, McpError> {
        let size_mb = p.size_mb;
        let rounds = p.rounds;
        match tokio::task::spawn_blocking(move || bench::mem_test(size_mb, rounds)).await {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("内存测试任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "蓝屏转储分析（只读 L0，复刻 BlueScreenView）：扫描 C:\\Windows\\Minidump\\*.dmp 与根目录 MEMORY.DMP，解析每个转储的崩溃时间 / bugcheck 代码 / 4 参数，映射为中文含义（0x124 WHEA=CPU/内存硬件、0x116 VIDEO_TDR=显卡驱动、0x1A MEMORY_MANAGEMENT=内存等 24 个常见代码）。参数 max_dumps=最多解析几个（默认 10，上限 30）、include_memory_dmp=是否读根目录 MEMORY.DMP（默认 true）。无转储时返回『未记录到蓝屏』正面结论。用于『为什么蓝屏了/蓝屏代码是什么意思』场景。纯只读不改删文件。"
    )]
    async fn bsod_analyze(
        &self,
        Parameters(p): Parameters<BsodAnalyzeParams>,
    ) -> Result<CallToolResult, McpError> {
        let max_dumps = p.max_dumps;
        let include_memory_dmp = p.include_memory_dmp;
        Ok(blocking_call(
            60,
            move || bsod::analyze_bsod(max_dumps, include_memory_dmp),
            "蓝屏转储分析",
        )
        .await)
    }
}
