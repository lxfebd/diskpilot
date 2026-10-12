//! GPU 温度域（从 hw_thermal.rs 拆分，2026-10-12）。
//! gpu_temp_c：NVIDIA nvidia-smi → AMD ADL 双通道，熔断护栏用。

// N 卡/A 卡实时通道仅 Windows 有实现，随 cfg(windows) 的 gpu_temp_c 一起编译。
#[cfg(windows)]
use crate::tools::hw_common::{amd_adl_live, nvidia_gpu_live};

#[cfg(windows)]
pub fn gpu_temp_c() -> Result<f64, String> {
    let mut nv_err: Option<String> = None;
    if let Ok(Some(g)) = nvidia_gpu_live() {
        if g.temp > 0 {
            return Ok(g.temp as f64);
        }
        nv_err = Some("GPU 温度读数为 0（nvidia-smi 未就绪）".into());
    }
    if let Ok(Some(list)) = amd_adl_live() {
        if let Some(a) = list.into_iter().find(|a| a.temp_c > 0) {
            return Ok(a.temp_c as f64);
        }
    }
    Err(nv_err
        .unwrap_or_else(|| "未检测到 NVIDIA（nvidia-smi）或 AMD（atiadlxx ADL）GPU 温度".into()))
}

#[cfg(not(windows))]
pub fn gpu_temp_c() -> Result<f64, String> {
    Err("gpu_temp_c 仅支持 Windows".into())
}

/// 从一行文本里解析 `GPU（厂商）：温度 ℃`（前端熔断正则的等价解析，测试用）。
#[cfg(test)]
pub(crate) fn parse_gpu_temp_line(line: &str) -> Option<f64> {
    let rest = line
        .split("）：")
        .nth(1)
        .or_else(|| line.split("):").nth(1))?;
    // rest 形如「 58 ℃（atiadlxx ADL 实时）」，取第一个空白前数字
    let head = rest.trim_start();
    let num_end = head
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(head.len());
    head[..num_end].parse::<f64>().ok().filter(|t| *t > 0.0)
}
