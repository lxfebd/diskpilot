//! 硬件扩展信息域（从 tool_hw.rs 拆分，2026-10-12）。
//! hw_cpu_features / hw_dram_timings / hw_displays。

use crate::tools::params::*;
use crate::tools::{blocking_call, hw, AgentServer};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

#[tool_router(router = tool_router_hw_ext, vis = "pub(crate)")]
impl AgentServer {
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
}
