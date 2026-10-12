//! 硬件/基准/压力/传感器域工具处理器（从 tools/mod.rs 拆分，2026-10-12）。
//!
//! 2026-10-12 拆分：主硬件信息 → `tool_hw_core`，扩展信息 → `tool_hw_ext`，
//! 基准/压力/蓝屏 → `tool_hw_bench`，体检报告/自愈 → `tool_hw_report`。
//! 各块独立 `#[tool_router(router = tool_router_hw_*)]`，mod.rs 合并。
