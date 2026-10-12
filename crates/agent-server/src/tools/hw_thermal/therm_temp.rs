//! 温度采集主通道（从 hw_thermal.rs 拆分，2026-10-12）。
//! THERMAL_PS + collect_temperature：HWiNFO 共享内存 → ACPI 热区降级。

use crate::ps::{json_array_of, ps_capture};
use crate::tools::hw_common::{amd_adl_temp, get_f64, nvidia_gpu_temp, thermal_ready};
use crate::tools::hw_disk::DISK_SMART_PS;
use crate::tools::v_str;
use serde_json::Value;

use super::therm_sensors::{extract_cpu_temps_from_sensor_json, run_fancmd_sensors};

pub(crate) const THERMAL_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature | Select-Object InstanceName,CurrentTemperature)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 温度采集：优先 HWiNFO 共享内存（CPU/GPU/主板/磁盘全温度，普通权限可读），
/// 失败回退 ACPI 机箱级热区 + GPU 温度。
///
/// ACPI 热区是「十分之一开尔文」，且**未就绪时会报 2731（=27.31 ℃ 的假值）或 0**，
/// 必须用合理区间过滤；全部被过滤掉时返回降级说明，绝不假装读到了。
#[cfg(windows)]
pub fn collect_temperature() -> Result<String, String> {
    // 1) 优先 HWiNFO 共享内存：HWiNFO 后台运行时以**普通权限**发布全部温度传感器，
    //    非管理员也能读到 CPU/GPU/主板/磁盘温度（根治「AI 看不到 CPU 温度」）。
    //    读数可能多达几十条，截断到 60 条防止 AI 上下文爆炸。
    if let Ok(lines) = crate::tools::hwinfo::read_temperature_lines() {
        let mut out: Vec<String> = Vec::new();
        for (nm, v) in lines.iter().take(60) {
            out.push(format!("- {nm}：{:.*} ℃", 1, v));
        }
        if lines.len() > 60 {
            out.push(format!("- ……（其余 {} 条温度省略）", lines.len() - 60));
        }
        let mut s = format!(
            "温度传感器（HWiNFO 共享内存，共 {} 条）：\n{}",
            lines.len(),
            out.join("\n")
        );
        s.push_str(
            "\n说明：HWiNFO 以普通权限后台发布共享内存，读到的温度与 HWiNFO 界面一致；\n  \
             HWiNFO 未运行时自动降级到 ACPI 热区 + GPU + SMART 通道。",
        );
        return Ok(s);
    }
    // 1.5) 自研 SuperIO 直读温度：HWiNFO 没运行时，inpoutx64 驱动（已装则普通权限可用）
    //      直读 ITE/Nuvoton/Winbond 环境寄存器（主板/CPU/辅助温度，0x29-0x2B）。
    if let Ok(lines) = crate::tools::superio::read_temperatures() {
        let mut out: Vec<String> = Vec::new();
        for (label, v) in lines.iter().take(10) {
            out.push(format!("- {label}：{:.*} ℃", 1, v));
        }
        let mut s = format!(
            "温度传感器（自研 SuperIO 直读，共 {} 条）：\n{}",
            lines.len(),
            out.join("\n")
        );
        s.push_str(
            "\n说明：直读主板 SuperIO 环境寄存器（inpoutx64 驱动，普通权限），无 HWiNFO/LHM 依赖。",
        );
        return Ok(s);
    }
    let raw = ps_capture(THERMAL_PS)?;
    let arr: Vec<Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析温度查询结果失败：{e}"))?;
    let mut out: Vec<String> = Vec::new();
    for t in &arr {
        let raw = get_f64(t, "CurrentTemperature").unwrap_or(0.0);
        let c = raw / 10.0 - 273.15;
        // 热区未就绪时 MSACPI 报标记值 2731（= 27.31 ℃）或 0，落在 0~120 区间内会被当成
        // 真实温度——这正是「电脑温度=室温」假象的来源。`thermal_ready` 统一排除。
        if thermal_ready(c) {
            out.push(format!(
                "- {}：{:.*} ℃",
                if v_str(t, "InstanceName").is_empty() {
                    "热区".into()
                } else {
                    v_str(t, "InstanceName")
                },
                0,
                c
            ));
        }
    }
    // CPU 温度：fancmd（LHM 内核）读 CPU 核心温度需 Ring0 驱动 + 管理员权限。
    // 非管理员下 CPU 温度必为 null，白等 fancmd 枚举 4-8s 毫无意义 → 直接跳过。
    // 管理员（DiskPilot 生产模式）下 fancmd 能读到 Core Max，并入输出；读不到静默跳过。
    if crate::tools::selfheal::is_admin() {
        if let Some(exe) = crate::tools::selfheal::find_fancmd() {
            if let Ok(raw) = run_fancmd_sensors(&exe) {
                if let Some(cpus) = extract_cpu_temps_from_sensor_json(&raw) {
                    for (nm, v) in cpus {
                        out.push(format!(
                            "- CPU（{nm}）：{:.*} ℃（LibreHardwareMonitor）",
                            1, v
                        ));
                    }
                }
            }
        }
    }
    match nvidia_gpu_temp() {
        Some(gt) => out.push(format!("- GPU（NVIDIA）：{gt}（nvidia-smi 实时）")),
        None => match amd_adl_temp() {
            Some(gt) => out.push(format!("- GPU（AMD）：{gt}（atiadlxx ADL 实时）")),
            None => out.push("- GPU：非 NVIDIA/AMD 或驱动未装，WMI 不提供独立显卡温度".into()),
        },
    }
    // 磁盘温度：SMART 控制器上报时是真实物理盘温度（NVMe/SSD），与热区无关，无条件并入。
    if let Ok(raw) = ps_capture(DISK_SMART_PS) {
        let trimmed = raw.trim().trim_start_matches('\u{feff}');
        if let Ok(arr) = serde_json::from_str::<Vec<Value>>(&json_array_of(trimmed)) {
            for d in arr
                .iter()
                .filter(|d| get_f64(d, "temperature").unwrap_or(0.0) != 0.0)
            {
                let nm = v_str(d, "friendly");
                let t = get_f64(d, "temperature").unwrap_or(0.0);
                // 有些控制器以 0.1℃ 上报、有些直接摄氏——按取值落在合理区间内的解释输出
                let c = if t > 120.0 { t / 10.0 } else { t };
                if !(0.0..=120.0).contains(&c) {
                    continue;
                }
                out.push(format!("- 磁盘（{nm}）：{:.*} ℃（SMART）", 0, c));
            }
        }
    }
    // 热区 / GPU / 磁盘温度至少一个有效才算「读到了」；全空 → 降级文案，绝不假装。
    let has_any = out
        .iter()
        .any(|l| l.contains("℃") && !l.contains("驱动未装"));
    if !has_any {
        Ok(
            "温度传感器读不到（ACPI 热区 + GPU 温度 + 磁盘 SMART 均不可用），只能给通用建议\n\
            - 常见原因：主板固件未实现 ACPI 热区，或当前没有就绪的热区\n\
            - 通用建议：留意机身/出风口触感，跑重负载时若外壳烫手可考虑清灰或垫高散热\n\
            - CPU 核心温度需厂商工具（如 Ryzen Master / IAUWMI / Intel DTT），WMI 只有机箱级热区"
                .into(),
        )
    } else {
        let mut s = format!("温度传感器：\n{}", out.join("\n"));
        s.push_str(
            "\n说明：ACPI 热区单位为十分之一开尔文（摄氏 = raw/10 - 273.15），\n  \
             已过滤未就绪的标记值（2731 = 27.31 ℃ 的假值）；GPU 温度 NVIDIA 取 nvidia-smi 实时读数、\n  \
             AMD 取 atiadlxx.dll ADL 实时读数（两通道都不可用时该行标注「驱动未装」）；\n  \
             磁盘取 SMART 控制器上报值；CPU 核心温度需厂商工具（如 Ryzen Master / IAUWMI）。",
        );
        Ok(s)
    }
}

#[cfg(not(windows))]
pub fn collect_temperature() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}
