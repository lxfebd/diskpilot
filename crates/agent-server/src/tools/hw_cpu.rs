//! CPU 采集：型号/核心/线程/频率/缓存 + CPUID 指令集探测。
//!
//! 从 hw.rs 拆分（2026-10-12）。
// 本文件采集实现均为 cfg(windows)（Linux 仅保留返回降级文案的 fn），
// 顶层 import 随 Windows 实现一起编译。
#[cfg(windows)]
use crate::tools::hw_common::{get_f64, get_int, json_arr_of, json_of, v_arr};
#[cfg(windows)]
use crate::tools::v_str;

// ── 1. CPU ────────────────────────────────────────────────────────────────

/// CPU PS 脚本仅 Windows 运行，Linux 下无消费方。
#[cfg(windows)]
const CPU_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$cpu=@(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors,MaxClockSpeed,CurrentClockSpeed,L2CacheSize,L3CacheSize,Manufacturer,VirtualizationFirmwareEnabled,LoadPercentage)
$perf=Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor | Where-Object { $_.Name -eq '_Total' } | Select-Object -First 1
$out=[ordered]@{ cpu=$cpu; total=[int]$cpu.Count; usage=if($perf){[double]$perf.PercentProcessorTime}else{$null} }
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// CPU 采集：型号/核心/线程/频率/缓存/实时占用。
#[cfg(windows)]
pub fn collect_cpu() -> Result<String, String> {
    let v = json_of(CPU_PS, "CPU")?;
    let arr = v_arr(&v, "cpu");
    if arr.is_empty() {
        return Ok("CPU 信息读不到（Win32_Processor 无返回）".into());
    }
    let first = &arr[0];
    let name = if v_str(first, "Name").is_empty() {
        "未知型号".to_string()
    } else {
        v_str(first, "Name")
    };
    let cores = get_int(first, "NumberOfCores").unwrap_or(0);
    let threads = get_int(first, "NumberOfLogicalProcessors").unwrap_or(0);
    let max_hz = get_int(first, "MaxClockSpeed").unwrap_or(0);
    let cur_hz = get_int(first, "CurrentClockSpeed").unwrap_or(0);
    // L2CacheSize 的 WMI bug：垃圾值（3 之类的）一律当未知，绝不显示 3 KB。
    let l2 = get_int(first, "L2CacheSize").unwrap_or(0);
    let l3 = get_int(first, "L3CacheSize").unwrap_or(0);
    let cache = |kb: u64| -> String {
        if kb < 16 {
            // WMI bug：常返回 3 之类的垃圾值，按未知处理
            "未知".to_string()
        } else if kb >= 1024 {
            format!("{:.*} MB", 1, kb as f64 / 1024.0)
        } else {
            format!("{kb} KB")
        }
    };
    // Win32_Processor.VirtualizationFirmwareEnabled 是 bool，JSON 里是小写 true/false
    let virt = match v_str(first, "VirtualizationFirmwareEnabled")
        .to_ascii_lowercase()
        .as_str()
    {
        "true" => "已开启",
        "false" => "未开启",
        _ => "未知",
    };
    let usage = get_f64(&v, "usage").unwrap_or(-1.0);
    let usage_str = if usage < 0.0 {
        "读不到".to_string()
    } else {
        format!("{:.*}%", 1, usage)
    };
    let total = get_int(&v, "total").unwrap_or(arr.len() as u64);
    let mut s = String::new();
    if total > 1 {
        s.push_str(&format!(
            "- CPU（共 {total} 路物理 CPU，以下为第 1 路）：\n"
        ));
    } else {
        s.push_str("- CPU：\n");
    }
    s.push_str(&format!("  - 型号：{name}\n"));
    s.push_str(&format!("  - 核心 / 线程：{cores} / {threads}\n"));
    if max_hz > 0 {
        s.push_str(&format!(
            "  - 标称频率：{} MHz（约 {:.*} GHz）\n",
            max_hz,
            2,
            max_hz as f64 / 1000.0
        ));
    }
    if cur_hz > 0 {
        s.push_str(&format!(
            "  - 实时频率：{} MHz（空闲时通常远低于标称，Turbo 下可更高）\n",
            cur_hz
        ));
    }
    s.push_str(&format!(
        "  - 缓存：L2 {} / L3 {}（WMI 的 L2CacheSize 常有 bug，过小值按未知处理）\n",
        cache(l2),
        cache(l3)
    ));
    let man = v_str(first, "Manufacturer");
    if !man.is_empty() {
        s.push_str(&format!("  - 厂商：{man}\n"));
    }
    s.push_str(&format!("  - 硬件虚拟化：{virt}\n"));
    s.push_str(&format!(
        "  - 实时占用：{usage_str}（PercentProcessorTime 瞬时采样，非平均值）"
    ));
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_cpu() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

/// CPUID 指令集探测 PS 脚本仅 Windows 运行，Linux 下无消费方。
#[cfg(windows)]
const CPUID_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfLogicalProcessors,VirtualizationFirmwareEnabled,SecondLevelAddressTranslationExtensions,VMMonitorModeExtensions,PowerManagementSupported)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// CPU 指令集特性：虚拟化/二级地址翻译（SLAT）/VM 监控模式扩展等 CPUID 位。
///
/// 读取 CPUID 指令集位（SSE/AVX/AVX2/FMA 等）需要 Ring0 或 cpuid 内联汇编，
/// WMI 只给到虚拟化相关位。这里输出 WMI 能确认的位 + 用 Rust 运行时探测
/// `is_x86_feature_detected!` 确认 AVX2/FMA 等用户态指令集（跨平台安全）。
/// **只读 L0**。
#[cfg(windows)]
pub fn collect_cpu_features() -> Result<String, String> {
    let arr = json_arr_of(CPUID_PS, "CPU 指令集")?;
    let first = arr.first().cloned().unwrap_or_default();
    let name = if v_str(&first, "Name").is_empty() {
        "未知型号".to_string()
    } else {
        v_str(&first, "Name")
    };
    let mut s = String::from("CPU 指令集与特性：\n");
    s.push_str(&format!("- 型号：{name}\n"));

    // 虚拟化位
    let virt = match v_str(&first, "VirtualizationFirmwareEnabled")
        .to_ascii_lowercase()
        .as_str()
    {
        "true" => "已开启",
        "false" => "未开启",
        _ => "未知",
    };
    s.push_str(&format!("- 硬件虚拟化（VT-x/AMD-V）：{virt}\n"));
    let slat = v_str(&first, "SecondLevelAddressTranslationExtensions").to_ascii_lowercase();
    let vmm = v_str(&first, "VMMonitorModeExtensions").to_ascii_lowercase();
    s.push_str(&format!(
        "- 二级地址翻译 SLAT（EPT/NPT）：{}\n",
        if slat == "true" {
            "支持"
        } else {
            "未知/不支持"
        }
    ));
    s.push_str(&format!(
        "- VM 监控模式扩展（VMX/SVM）：{}\n",
        if vmm == "true" {
            "支持"
        } else {
            "未知/不支持"
        }
    ));

    // 用户态指令集（运行时探测，跨平台安全）
    let mut features: Vec<&str> = Vec::new();
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("sse2") {
            features.push("SSE2");
        }
        if std::is_x86_feature_detected!("sse4.2") {
            features.push("SSE4.2");
        }
        if std::is_x86_feature_detected!("avx") {
            features.push("AVX");
        }
        if std::is_x86_feature_detected!("avx2") {
            features.push("AVX2");
        }
        if std::is_x86_feature_detected!("fma") {
            features.push("FMA");
        }
        if std::is_x86_feature_detected!("aes") {
            features.push("AES-NI");
        }
    }
    if features.is_empty() {
        s.push_str("- 指令集探测：当前平台无法用运行时探测确认（非 x86 或编译器不暴露），给出 WMI 虚拟化位即可。\n");
    } else {
        s.push_str(&format!("- 已确认指令集：{}\n", features.join(" / ")));
        s.push_str("- 说明：AVX2/FMA/AES-NI 由运行时 CPUID 探测（Rust is_x86_feature_detected），未列出的（AVX-512 等）不代表不支持。\n");
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_cpu_features() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

// ── 3.7 显示器信息（阶段四）───────────────────────────────────────────────
