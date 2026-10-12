//! 硬件/基准/压力/传感器域工具处理器（从 tools/mod.rs 拆分，2026-10-12）。

use crate::tools::params::*;
use crate::tools::{bench, bsod, files, hw, report, selfheal, superio};
use crate::tools::{
    blocking_call, text_result, tool_error, ttl_cached, AgentServer, HW_CPU_CACHE, HW_DISK_CACHE,
    HW_DISK_TTL, HW_MB_CACHE, HW_MEM_CACHE, HW_STATIC_TTL,
};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

#[tool_router(router = tool_router_hw, vis = "pub(crate)")]
impl AgentServer {
    #[tool(
        description = "读取 CPU 信息：型号/核心/线程数/基频与当前频率/缓存/实时占用率（只读，WMI，不需要管理员权限）。用户问『CPU 是什么/几个核/占用高不高』时调用。"
    )]
    async fn hw_cpu(&self, _p: Parameters<HwCpuParams>) -> Result<CallToolResult, McpError> {
        // CPU 静态信息 5 分钟缓存（对齐主进程 HW_SNAPSHOT_TTL=300s）。
        // 实时占用/频率字段随整包缓存，最多滞后 5 分钟——与主进程缓存整包同口径。
        Ok(blocking_call(
            30,
            || ttl_cached(&HW_CPU_CACHE, HW_STATIC_TTL, hw::collect_cpu),
            "CPU 信息采集",
        )
        .await)
    }

    #[tool(
        description = "读取显卡信息：型号/显存/驱动版本与日期/分辨率/刷新率，NVIDIA 卡附实时温度与占用（只读，不需要管理员权限）。用户问『什么显卡/显存多大/GPU 占用』时调用。"
    )]
    async fn hw_gpu(&self, _p: Parameters<HwGpuParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, hw::collect_gpu, "显卡信息读取").await)
    }

    #[tool(
        description = "读取内存信息：每根内存条容量/频率/厂商/型号 + 总容量 + XMP/EXPO 是否生效诊断（只读，不需要管理员权限）。用户问『内存多大/频率多少/该不该开 XMP』时调用。"
    )]
    async fn hw_memory(&self, _p: Parameters<HwMemoryParams>) -> Result<CallToolResult, McpError> {
        // 内存是静态硬件，5 分钟缓存（对齐 HW_SNAPSHOT_TTL）。
        Ok(blocking_call(
            30,
            || ttl_cached(&HW_MEM_CACHE, HW_STATIC_TTL, hw::collect_memory),
            "内存信息读取",
        )
        .await)
    }

    #[tool(
        description = "读取主板/整机信息：主板厂商与型号/BIOS 版本与日期/整机品牌型号（只读，不需要管理员权限）。用户问『这是什么主机/主板是什么』时调用。"
    )]
    async fn hw_motherboard(
        &self,
        _p: Parameters<HwMotherboardParams>,
    ) -> Result<CallToolResult, McpError> {
        // 主板/BIOS 是静态硬件，5 分钟缓存（对齐 HW_SNAPSHOT_TTL）。
        Ok(blocking_call(
            30,
            || ttl_cached(&HW_MB_CACHE, HW_STATIC_TTL, hw::collect_motherboard),
            "主板信息读取",
        )
        .await)
    }

    #[tool(
        description = "读取本机温度：优先走 HWiNFO 共享内存（HWiNFO 后台运行时以普通权限发布 CPU/GPU/主板/磁盘全部温度，无需管理员）；HWiNFO 未运行时按顺序尝试自研 SuperIO 直读（inpoutx64 驱动已装则普通权限直读主板 CPU/主板温度，无需 HWiNFO/LHM）、ACPI 热区 + NVIDIA GPU 实时温度 + 磁盘 SMART（只读，传感器读不到会明确说明，不会假装有数据）。用户问『电脑温度/烫不烫』时调用。"
    )]
    async fn hw_temperature(
        &self,
        _p: Parameters<HwTemperatureParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_temperature, "温度读取").await)
    }

    #[tool(
        description = "自研 SuperIO 直读工具（只读 L0，复刻 HWiNFO/LibreHardwareMonitor 的纯端口直读能力）：动态加载本机已装的端口驱动（inpoutx64 优先，普通权限即可，无需管理员/UAC/Ring0），扫描 0x2E/0x4E 双端口识别 ITE/Nuvoton/Winbond SuperIO 芯片，直读环境控制器寄存器返回主板/CPU 温度 + 风扇转速 + PWM 占空比。HWiNFO 没装/没开共享内存且不想依赖 LHM 时用它兜底 CPU/主板传感器。用户问『主板读到的温度/风扇转速是什么』或 HWiNFO 不可用时调用。纯只读，不写任何寄存器、不调速。"
    )]
    async fn hw_superio(
        &self,
        _p: Parameters<HwSuperioParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, superio::read_text, "SuperIO 直读").await)
    }

    #[tool(
        description = "读取全量硬件传感器快照（AIDA64 同类）：CPU/GPU/主板温度、风扇转速、电压、功耗、频率、负载，按硬件分组输出（只读）。优先走 HWiNFO 共享内存（普通权限即可读全部传感器，无需管理员/Ring0）；HWiNFO 未运行/未启用共享内存时降级 LibreHardwareMonitor 内核（fancmd sensors），仍不可用再回退 ACPI 热区 + GPU + SMART 通道。用户问『温度/风扇/电压/功耗/整机健康』时调用，比 hw_temperature 更全。"
    )]
    async fn hw_sensors(
        &self,
        _p: Parameters<HwSensorsParams>,
    ) -> Result<CallToolResult, McpError> {
        // 同步采集 + 外层超时兜底：fancmd/UAC 提权/系统通道任何单点卡死
        // 都不能拖垮 AI 调用（25s 到点返回明确信息，不挂起）。
        match tokio::time::timeout(
            std::time::Duration::from_secs(25),
            tokio::task::spawn_blocking(move || -> Result<String, String> {
                hw::collect_sensors()
            }),
        )
        .await
        {
            Ok(Ok(Ok(s))) => Ok(text_result(s)),
            Ok(Ok(Err(e))) => Ok(tool_error(e)),
            Ok(Err(_)) => Ok(tool_error("硬件传感器采集线程异常退出")),
            Err(_) => Ok(tool_error(
                "硬件传感器采集超时（>25s，多为 UAC 等待或驱动扫描慢）；请重试或提权后直读",
            )),
        }
    }

    #[tool(
        description = "读取电池信息：电量百分比/电池健康度/充电状态/循环次数/设计容量（只读，不需要管理员权限；台式机返回『未检测到电池』）。用户问『笔记本电池健康吗/续航/要不要换电池』时调用。"
    )]
    async fn hw_battery(
        &self,
        _p: Parameters<HwBatteryParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, hw::collect_battery, "电池信息读取").await)
    }

    #[tool(
        description = "读取磁盘健康（SMART）：每块物理磁盘的容量/接口/健康状态/通电时间/温度/磨损与错误计数（只读，Win10 1607+ 普通权限即可，不需要管理员，不使用已移除的 wmic）。用户问『硬盘健康吗/通电多久/要坏了吗』时调用。"
    )]
    async fn hw_disk_smart(
        &self,
        _p: Parameters<HwDiskSmartParams>,
    ) -> Result<CallToolResult, McpError> {
        // SMART 通电时间/温度/磨损不会秒变，5 分钟缓存（对齐 HW_DISK_TTL）。
        Ok(blocking_call(
            45,
            || ttl_cached(&HW_DISK_CACHE, HW_DISK_TTL, hw::collect_disk_smart),
            "磁盘健康读取",
        )
        .await)
    }

    // ── 网络（net.rs）─────────────────────────────────────────────

    #[tool(
        description = "CPU 指令集与特性（只读 L0，复刻 AIDA64 CPUID）：硬件虚拟化/VT-x/AMD-V、SLAT（EPT/NPT）、VMX/SVM，以及运行时探测确认的指令集（SSE2/SSE4.2/AVX/AVX2/FMA/AES-NI）。AI 在用户问『CPU 支持哪些指令集/虚拟化』时调用。"
    )]
    async fn hw_cpu_features(
        &self,
        _p: Parameters<HwCpuFeaturesParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_cpu_features, "CPU 指令集采集").await)
    }

    #[tool(
        description = "DRAM 时序（只读 L0，复刻 AIDA64 SPD/DRAM Timings）：每根内存条的容量/标称与运行频率/品牌型号 + 频率一致性诊断（XMP/EXPO 是否生效）。完整 CL 时序需 SPD 读取工具（CPU-Z/Thaiphoon），本工具不编造。AI 在用户问『内存频率/时序/XMP 生效没』时调用。"
    )]
    async fn hw_dram_timings(
        &self,
        _p: Parameters<HwDramTimingsParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_dram_timings, "DRAM 时序采集").await)
    }

    #[tool(
        description = "显示器信息（只读 L0，复刻 AIDA64 Monitor）：每台显示器的物理尺寸（cm/英寸）/输入类型/激活状态。分辨率与刷新率见 collect_gpu。AI 在用户问『显示器型号/尺寸/支持什么输入』时调用。"
    )]
    async fn hw_displays(
        &self,
        _p: Parameters<HwDisplaysParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_displays, "显示器采集").await)
    }

    // ── 阶段五：长尾 G12/G14/G17（bench_gpu 简化版 / IPMI 探测 / ACPI 信息） ──

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

    // ── 阶段三：体检报告 / 传感器趋势 / 阈值告警（report.rs） ────────────

    #[tool(
        description = "一键系统体检报告（只读 L0，复刻 AIDA64 Report）：汇总 CPU/GPU/内存/主板/温度/磁盘健康/系统/电池全部只读采集，输出 Markdown 报告。AI 在用户问『体检/电脑状态总结/报告』时调用，拿它当结论依据。"
    )]
    async fn system_report(
        &self,
        _p: Parameters<SystemReportParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(120, report::system_report, "体检报告生成").await)
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
