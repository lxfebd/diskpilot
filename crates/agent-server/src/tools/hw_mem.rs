//! 内存采集：每根内存条 + 总容量 + XMP/EXPO 诊断 + DRAM 时序。
//!
//! 从 hw.rs 拆分（2026-10-12）。
// 本文件采集实现均为 cfg(windows)（Linux 仅保留返回 Err 的降级 fn），
// 顶层 import 随 Windows 实现一起编译。
#[cfg(windows)]
use crate::tools::fmt_bytes;
#[cfg(windows)]
use crate::tools::hw_common::{get_int, json_arr_of};
#[cfg(windows)]
use crate::tools::v_str;

// ── 3. 内存 ───────────────────────────────────────────────────────────────

/// 内存 PS 脚本仅 Windows 运行，Linux 下无消费方。
#[cfg(windows)]
const MEM_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
# 与 GPU_PS 同理：部分机 Get-CimInstance 对该类抛「调用已取消」返回空，
# 旧版 Get-WmiObject 仍可读 → 失败回退，保证内存条/容量/频率能探明。
$out = $null
try {
  $out = @(Get-CimInstance Win32_PhysicalMemory | Select-Object DeviceLocator,Capacity,Speed,ConfiguredClockSpeed,Manufacturer,PartNumber,MemoryType,BankLabel,FormFactor)
} catch { $out = $null }
if (-not $out -or $out.Count -eq 0) {
  $out = @(Get-WmiObject Win32_PhysicalMemory | Select-Object DeviceLocator,Capacity,Speed,ConfiguredClockSpeed,Manufacturer,PartNumber,MemoryType,BankLabel,FormFactor)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 内存采集：每根内存条 + 总容量 + XMP/EXPO 生效诊断。
#[cfg(windows)]
pub fn collect_memory() -> Result<String, String> {
    let arr = json_arr_of(MEM_PS, "内存")?;
    if arr.is_empty() {
        return Ok("内存信息读不到（Win32_PhysicalMemory 无返回）".into());
    }
    let total: u64 = arr.iter().filter_map(|m| get_int(m, "Capacity")).sum();
    let mut s = String::new();
    s.push_str(&format!(
        "- 总容量：{}（{} 根内存条）\n",
        fmt_bytes(total),
        arr.len()
    ));
    for m in &arr {
        let slot = if v_str(m, "DeviceLocator").is_empty() {
            "未知插槽".to_string()
        } else {
            v_str(m, "DeviceLocator")
        };
        let cap = fmt_bytes(get_int(m, "Capacity").unwrap_or(0));
        let speed = get_int(m, "Speed").unwrap_or(0);
        let conf = get_int(m, "ConfiguredClockSpeed").unwrap_or(0);
        let man = if v_str(m, "Manufacturer").is_empty() {
            "未知品牌".to_string()
        } else {
            v_str(m, "Manufacturer")
        };
        let pn_raw = v_str(m, "PartNumber");
        let pn = if pn_raw.trim().is_empty() {
            "未知型号".to_string()
        } else {
            pn_raw.trim().to_string()
        };
        // Win32_PhysicalMemory.PartNumber 尾部常带 NUL 填充，trim 掉
        let mt = v_str(m, "MemoryType");
        // DDR5 的 Speed 只有实际有效频率的一半；DDR4 及以下不翻倍，别给错提示。
        let is_ddr5 = mt.contains("DDR5") || pn.contains("DDR5");
        s.push_str(&format!(
            "- 内存条「{slot}」：{cap}，标称 {speed} MT/s，运行 {conf} MT/s\n"
        ));
        s.push_str(&format!("  - 品牌 {man} / 型号 {pn}\n"));
        if is_ddr5 {
            s.push_str(
                "  - 说明：标称速率，DDR5 实际有效频率通常为其 2 倍（DDR5-4800 ≈ 等效 9600 MT/s）\n",
            );
        } else {
            s.push_str("  - 说明：速率单位为 MT/s（DDR4 及以下，无 2 倍折算）\n");
        }
    }
    // XMP/EXPO 诊断：运行频率 < 标称 → profile 可能没生效（照搬主项目 hw.rs:1074-1085）。
    let max_speed = arr
        .iter()
        .filter_map(|m| get_int(m, "Speed"))
        .max()
        .unwrap_or(0);
    let max_conf = arr
        .iter()
        .filter_map(|m| get_int(m, "ConfiguredClockSpeed"))
        .max()
        .unwrap_or(0);
    if max_conf > 0 && max_speed > 0 {
        s.push_str(&format!(
            "- 频率诊断：最高标称 {} MT/s，最高运行 {} MT/s（{}）\n",
            max_speed,
            max_conf,
            if max_conf < max_speed {
                "XMP/EXPO 可能未生效"
            } else {
                "XMP/EXPO 已生效"
            }
        ));
    } else if max_speed > 0 {
        s.push_str(
            "- 频率诊断：读不到实际运行频率（ConfiguredClockSpeed 为空），无法判断 XMP 是否生效\n",
        );
    } else {
        s.push_str("- 频率诊断：速率读不到（Speed / ConfiguredClockSpeed 均为空）\n");
    }
    s.push_str("- 时序（CL）：WMI 不提供，需 SPD 读取工具（如 CPU-Z 的 SPD tab、Thaiphoon Burner），本工具不编造");
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_memory() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

// ── 3.5 DRAM 时序（SPD 读取，阶段四）──────────────────────────────────────

/// DRAM 时序 PS 脚本仅 Windows 运行，Linux 下无消费方。
#[cfg(windows)]
const DRAM_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-CimInstance Win32_PhysicalMemory | Select-Object DeviceLocator,Manufacturer,PartNumber,ConfiguredClockSpeed,Speed,Capacity)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// DRAM 时序信息：每根内存条的 SPD 级时序（CL-tRCD-tRP-tRAS）。
///
/// WMI 不直接暴露 SPD 时序，这里用 SPD 读取工具（Thaiphoon Burner 便携版 /
/// RWEverything）探路：有则优先输出，无则基于运行频率给出一致性检查
/// （不编造具体数值）。**只读 L0**。
#[cfg(windows)]
pub fn collect_dram_timings() -> Result<String, String> {
    let arr = json_arr_of(DRAM_PS, "内存时序")?;
    if arr.is_empty() {
        return Ok("内存时序读不到（Win32_PhysicalMemory 无返回）".into());
    }
    let mut s = String::from("DRAM 时序信息：\n");
    for (i, m) in arr.iter().enumerate() {
        let slot = if v_str(m, "DeviceLocator").is_empty() {
            "未知插槽".to_string()
        } else {
            v_str(m, "DeviceLocator")
        };
        let cap = fmt_bytes(get_int(m, "Capacity").unwrap_or(0));
        let speed = get_int(m, "Speed").unwrap_or(0);
        let conf = get_int(m, "ConfiguredClockSpeed").unwrap_or(0);
        let pn = if v_str(m, "PartNumber").trim().is_empty() {
            "未知型号".to_string()
        } else {
            v_str(m, "PartNumber").trim().to_string()
        };
        let man = if v_str(m, "Manufacturer").is_empty() {
            "未知品牌".to_string()
        } else {
            v_str(m, "Manufacturer")
        };
        s.push_str(&format!(
            "- 内存条 {}「{slot}」：{cap}，{man} / {pn}\n",
            i + 1
        ));
        s.push_str(&format!("  - 标称 {speed} MT/s，运行 {conf} MT/s\n"));
    }
    // 一致性检查：运行频率 < 标称 → XMP/EXPO 可能没生效
    let max_speed = arr
        .iter()
        .filter_map(|m| get_int(m, "Speed"))
        .max()
        .unwrap_or(0);
    let max_conf = arr
        .iter()
        .filter_map(|m| get_int(m, "ConfiguredClockSpeed"))
        .max()
        .unwrap_or(0);
    if max_conf > 0 && max_speed > 0 {
        s.push_str(&format!(
            "- 频率一致性：标称 {} MT/s / 运行 {} MT/s（{}）\n",
            max_speed,
            max_conf,
            if max_conf < max_speed {
                "XMP/EXPO 可能未生效"
            } else {
                "XMP/EXPO 已生效"
            }
        ));
    }
    // SPD 读取工具探路（不编造）
    for (exe, tag) in [
        ("ThaiphoonBurner.exe", "Thaiphoon Burner"),
        ("RW.exe", "RWEverything"),
    ] {
        let candidate = std::env::var("ProgramFiles")
            .map(|p| std::path::Path::new(&p).join(exe))
            .unwrap_or_default();
        if candidate.exists() {
            s.push_str(&format!(
                "  - 检测到 {tag}（{exe}），其 SPD 报告可给出完整时序（CL-tRCD-tRP），本工具不解析二进制。\n"
            ));
        }
    }
    s.push_str(
        "- 完整 CL 时序需 SPD 读取（CPU-Z SPD tab / Thaiphoon Burner），本工具不编造具体数值。",
    );
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_dram_timings() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}
