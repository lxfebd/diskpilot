//! GPU 采集：型号/显存/驱动/分辨率 + N 卡/A 卡实时通道。
//!
//! 从 hw.rs 拆分（2026-10-12）。
use crate::tools::fmt_bytes;
use crate::tools::hw_common::{
    amd_adl_live, fmt_wmi_date, get_int, json_arr_of, nvidia_gpu_live, nvidia_gpu_name,
};
use crate::tools::v_str;

// ── 2. GPU ────────────────────────────────────────────────────────────────

const GPU_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
# 部分机器 Get-CimInstance 对 Win32_VideoController 抛「调用已取消」返回空，
# 旧版 Get-WmiObject 仍可读 → 失败自动回退，保证显卡型号/驱动/显存能探明。
$out = $null
try {
  $out = @(Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion,DriverDate,CurrentRefreshRate,CurrentHorizontalResolution,CurrentVerticalResolution,DeviceID)
} catch { $out = $null }
if (-not $out -or $out.Count -eq 0) {
  $out = @(Get-WmiObject Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion,DriverDate,CurrentRefreshRate,CurrentHorizontalResolution,CurrentVerticalResolution,DeviceID)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 显卡采集：型号/显存/驱动/分辨率/刷新率，外加 N 卡实时占用/温度/显存。
#[cfg(windows)]
pub fn collect_gpu() -> Result<String, String> {
    let arr = json_arr_of(GPU_PS, "显卡")?;
    if arr.is_empty() {
        return Ok("显卡信息读不到（Win32_VideoController 无返回）".into());
    }
    let mut s = String::new();
    let mut nvidia_via_wmi = false;
    for (i, g) in arr.iter().enumerate() {
        let nm = if v_str(g, "Name").is_empty() {
            "未知显卡".to_string()
        } else {
            v_str(g, "Name")
        };
        s.push_str(&format!("- 显卡 {}：{nm}\n", i + 1));
        // 显存 32 位溢出坑：≥4 GB 的卡回绕，<= 4GB-1MB 也是回绕产物。
        let vram = get_int(g, "AdapterRAM").unwrap_or(0);
        let vram_str = if vram == 0 {
            "共享系统内存 / 未知".to_string()
        } else if vram >= 4_294_967_296 || vram <= 4_293_918_720 {
            "≥4 GB（WMI 32 位溢出，实际可能更大）".to_string()
        } else {
            fmt_bytes(vram)
        };
        s.push_str(&format!("  - 显存：{vram_str}\n"));
        let dv = v_str(g, "DriverVersion");
        let dd = v_str(g, "DriverDate");
        if !dv.is_empty() || !dd.is_empty() {
            s.push_str(&format!(
                "  - 驱动：{}（日期 {}）\n",
                if dv.is_empty() { "未知".into() } else { dv },
                if dd.is_empty() {
                    "未知".into()
                } else {
                    fmt_wmi_date(&dd)
                }
            ));
        }
        let hw = get_int(g, "CurrentHorizontalResolution").unwrap_or(0);
        let vh = get_int(g, "CurrentVerticalResolution").unwrap_or(0);
        let fr = get_int(g, "CurrentRefreshRate").unwrap_or(0);
        if hw == 0 && vh == 0 && fr == 0 {
            s.push_str("  - 当前输出：无活动显示（虚拟显示适配器，或未接显示器）\n");
        } else {
            s.push_str(&format!("  - 当前输出：{hw} x {vh} @ {fr} Hz\n"));
        }
        let did = v_str(g, "DeviceID");
        if !did.is_empty() {
            s.push_str(&format!("  - DeviceID：{did}\n"));
        }
        // WMI 已列出物理 N 卡（NVIDIA 型号名）→ 标记，稍后 nvidia-smi 只补实时行不重复主卡。
        let nm_lower = nm.to_ascii_lowercase();
        if nm_lower.contains("nvidia")
            && !nm_lower.contains("virtual")
            && !nm_lower.contains("mirar")
        {
            nvidia_via_wmi = true;
        }
    }
    // 物理独显实时通道：WMI 未列出物理 N 卡时，nvidia-smi 型号作为主卡条目输出；
    // WMI 已列出（如 RTX 5090）时只补实时占用/温度/精确显存，避免重复条目。
    match nvidia_gpu_live() {
        Ok(Some(l)) => {
            let mu = l
                .mem_used
                .map(|m| format!("已用 {}", fmt_bytes(m * 1024 * 1024)))
                .unwrap_or_else(|| "已用未知".to_string());
            if !nvidia_via_wmi {
                s.push_str(&format!(
                    "- 显卡 {}：{}（nvidia-smi 物理卡）\n",
                    arr.len() + 1,
                    nvidia_gpu_name().unwrap_or_else(|| "NVIDIA".to_string())
                ));
                s.push_str(&format!(
                    "  - 显存：{}（共 {}）\n",
                    mu,
                    fmt_bytes(l.mem_total * 1024 * 1024)
                ));
            } else {
                s.push_str(&format!(
                    "  - N 卡实时：占用 {}% / 温度 {} ℃ / 显存 {}（共 {}）\n",
                    l.util,
                    l.temp,
                    mu,
                    fmt_bytes(l.mem_total * 1024 * 1024)
                ));
            }
        }
        _ => match amd_adl_live() {
            Ok(Some(list)) if !list.is_empty() => {
                let shown = list
                    .iter()
                    .filter(|a| a.gpu_util > 0 || a.temp_c > 0)
                    .map(|a| {
                        format!(
                            "  - A 卡实时（ADL 适配器 {}）：占用 {}% / 温度 {} ℃",
                            a.index, a.gpu_util, a.temp_c
                        )
                    })
                    .collect::<Vec<_>>();
                for line in shown {
                    s.push_str(&line);
                    s.push('\n');
                }
            }
            _ => {
                s.push_str(
                    "  - 实时（占用/温度）：无 NVIDIA（nvidia-smi）且无 AMD（atiadlxx ADL），跳过\n",
                );
            }
        },
    }
    s.push_str("说明：WMI 列出的适配器可能含虚拟显示适配器（模拟器/远程桌面）；物理独显的\n  型号/显存/温度/占用以 nvidia-smi（N 卡）或 atiadlxx ADL（A 卡）实时通道为准；\n  WMI AdapterRAM 为 32 位，≥4 GB 会回绕，故物理卡显存以 nvidia-smi 为准。");
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_gpu() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

/// 本工具给出 GPU 信息快照 + 说明深度基准未实现，**只读 L0**。
pub fn collect_gpu_snapshot() -> Result<String, String> {
    let gpu = collect_gpu().unwrap_or_else(|e| format!("GPU 信息读不到：{e}"));
    let mut s = String::from("GPU 快照：\n");
    s.push_str(&gpu);
    s.push_str("\n- GPGPU 深度基准（像素填充率/OpenCL）需 GPU 厂商 SDK，本工具集未实现；当前为只读信息快照。");
    Ok(s)
}
