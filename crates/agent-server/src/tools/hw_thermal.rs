//! 温度采集：HWiNFO 共享内存 → SuperIO 直读 → fancmd(LHM) → ACPI 热区 + GPU，多级降级。
//!
//! 从 hw.rs 拆分（2026-10-12）。
//!
//! 2026-10-12 再拆分：主温度通道 → `therm_temp`，全量传感器快照 → `therm_sensors`，
//! CPU 熔断温度 → `therm_cpu`，GPU 温度 → `therm_gpu`。

mod therm_cpu;
mod therm_gpu;
mod therm_sensors;
mod therm_temp;

pub use therm_cpu::max_cpu_temp;
pub use therm_gpu::gpu_temp_c;
pub use therm_sensors::{collect_sensors, collect_sensors_fast};
pub use therm_temp::collect_temperature;

#[cfg(test)]
pub(crate) use therm_gpu::parse_gpu_temp_line;
