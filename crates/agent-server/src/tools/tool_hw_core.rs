//! 主硬件信息域（从 tool_hw.rs 拆分，2026-10-12）。
//! hw_cpu / hw_gpu / hw_memory / hw_motherboard / hw_temperature / hw_superio /
//! hw_sensors / hw_battery / hw_disk_smart。

use crate::tools::params::*;
use crate::tools::{
    blocking_call, hw, superio, text_result, tool_error, ttl_cached, AgentServer, HW_CPU_CACHE,
    HW_DISK_CACHE, HW_DISK_TTL, HW_MB_CACHE, HW_MEM_CACHE, HW_STATIC_TTL,
};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

#[tool_router(router = tool_router_hw_core, vis = "pub(crate)")]
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
}
