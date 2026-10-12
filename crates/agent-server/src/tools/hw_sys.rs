//! 系统信息采集：显示器 / ACPI 热区 / IPMI 探测 / 主板 BIOS。
//!
//! 从 hw.rs 拆分（2026-10-12）。
// 本文件采集实现均为 cfg(windows)（Linux 仅保留返回降级文案的 fn），
// 顶层 import 随 Windows 实现一起编译。
#[cfg(windows)]
use crate::tools::hw_common::{fmt_wmi_date, get_int, json_arr_of, json_of, v_arr};
#[cfg(windows)]
use crate::tools::v_str;
#[cfg(windows)]
use serde_json::Value;

/// 显示器 PS 脚本仅 Windows 运行，Linux 下无消费方。
#[cfg(windows)]
const DISPLAY_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorBasicDisplayParams | Select-Object InstanceName,VideoInputType,MaxHorizontalImageSize,MaxVerticalImageSize,DisplayTransferCharacteristic,Active)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 显示器信息：WMI 基本显示参数（物理尺寸/输入类型/激活状态）。
///
/// 完整 EDID（厂商/序列号/生产周/色彩位深）需要解析 EDID 二进制，WMI 给到
/// 物理尺寸 + 输入类型；分辨率/刷新率已在 collect_gpu 输出。**只读 L0**。
#[cfg(windows)]
pub fn collect_displays() -> Result<String, String> {
    let arr = json_arr_of(DISPLAY_PS, "显示器")?;
    if arr.is_empty() {
        return Ok(
            "显示器信息读不到（WmiMonitorBasicDisplayParams 无返回，可能无外接显示器或驱动未暴露）"
                .into(),
        );
    }
    let mut s = String::from("显示器信息：\n");
    for (i, d) in arr.iter().enumerate() {
        let instance = v_str(d, "InstanceName");
        let input = v_str(d, "VideoInputType");
        let w = get_int(d, "MaxHorizontalImageSize").unwrap_or(0);
        let h = get_int(d, "MaxVerticalImageSize").unwrap_or(0);
        let active = v_str(d, "Active").to_ascii_lowercase();
        s.push_str(&format!("- 显示器 {}：\n", i + 1));
        if !instance.is_empty() {
            // 形如 DISPLAY\DEL40C4\5&... ，截短成友好串
            let short = instance.split('\\').nth(1).unwrap_or(&instance);
            s.push_str(&format!("  - 设备：{short}\n"));
        }
        s.push_str(&format!(
            "  - 物理尺寸：{} cm × {} cm（约 {:.1} 英寸）\n",
            w,
            h,
            ((w as f64).powi(2) + (h as f64).powi(2)).sqrt() / 2.54
        ));
        s.push_str(&format!(
            "  - 输入类型：{}\n",
            if input.is_empty() {
                "未知".to_string()
            } else {
                input
            }
        ));
        s.push_str(&format!(
            "  - 状态：{}\n",
            if active == "true" {
                "激活"
            } else {
                "非激活"
            }
        ));
    }
    s.push_str("- 分辨率/刷新率见 collect_gpu 输出；完整 EDID（序列号/生产周）需专用工具解析。");
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_displays() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

// ── 3.8 ACPI 固件/电源信息（阶段五·G17 简化版）────────────────────────
// WMI Win32_BIOS + Win32_ComputerSystem 可读固件版本/序列号/唤醒类型 + 电源状态；
// 完整 ACPI 表浏览（FADT/DSDT 反汇编）需 Ring0/bios 工具，这里给到可读信息。

/// ACPI 热区 PS 脚本仅 Windows 运行，Linux 下无消费方。
#[cfg(windows)]
const ACPI_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=[ordered]@{
  bios=@(Get-CimInstance Win32_BIOS | Select-Object Manufacturer,SMBIOSBIOSVersion,SerialNumber,Version,ReleaseDate)
  sys=@(Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer,Model,SystemType,TotalPhysicalMemory)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// ACPI / 固件信息（只读）：BIOS 厂商/版本/序列号/发布日期 + 机箱厂商/型号/系统类型。
///
/// AIDA64「ACPI Browser」可浏览 FADT/DSDT 等表（需 Ring0），本工具给到 WMI 可读的
/// 固件与电源顶层信息；完整反汇编不在此列（专业向，无安全收益）。**只读 L0**。
#[cfg(windows)]
pub fn collect_acpi() -> Result<String, String> {
    let v = json_of(ACPI_PS, "ACPI/固件")?;
    let mut s = String::from("ACPI 固件与电源信息：\n");
    let bins = v_arr(&v, "bios");
    if let Some(b) = bins.first() {
        let mfr = v_str(b, "Manufacturer");
        let ver = v_str(b, "SMBIOSBIOSVersion");
        let sn = v_str(b, "SerialNumber");
        let rel = v_str(b, "ReleaseDate");
        if !mfr.is_empty() || !ver.is_empty() {
            s.push_str(&format!("- BIOS：{} {}\n", mfr, ver));
        }
        if !rel.is_empty() {
            s.push_str(&format!("- BIOS 发布日期：{rel}\n"));
        }
        if !sn.is_empty() && sn != "To be filled by O.E.M." {
            s.push_str(&format!("- BIOS 序列号：{sn}\n"));
        }
    }
    let systs = v_arr(&v, "sys");
    if let Some(sys) = systs.first() {
        let mfr = v_str(sys, "Manufacturer");
        let model = v_str(sys, "Model");
        let st = v_str(sys, "SystemType");
        if !mfr.is_empty() || !model.is_empty() {
            s.push_str(&format!("- 机箱：{mfr} {model}\n"));
        }
        if !st.is_empty() {
            s.push_str(&format!("- 系统类型（ACPI OEM）：{st}\n"));
        }
    }
    s.push_str("- 说明：完整 ACPI 表浏览（FADT/DSDT 反汇编）需 Ring0 工具，属专业向；这里给到固件/电源顶层信息。");
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_acpi() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

// ── 3.9 IPMI 服务器接口探测（阶段五·G14）────────────────────────────

/// IPMI 探测 PS 脚本仅 Windows 运行，Linux 下无消费方。
#[cfg(windows)]
const IPMI_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=[ordered]@{
  ipmi=@(Get-CimInstance -Namespace root\wmi -ClassName IPMI_Interface 2>$null | Select-Object -First 5)
  baseboard=@(Get-CimInstance Win32_BaseBoard | Select-Object Manufacturer,Product)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// IPMI 服务器接口探测（只读）：列出 IPMI 接口类实例（BMC 存在性）。
///
/// 家用台式机一般**无 BMC/IPMI**（HEDT/服务器板才有），无返回时如实提示「未检测到
/// IPMI」，不编造传感器/日志。服务器场景 AIDA64 会读 BMC 传感器/系统事件日志（SEL），
/// 这里探测到接口后给引导（IPMI 工具不在本工具集）。**只读 L0**。
#[cfg(windows)]
pub fn collect_ipmi() -> Result<String, String> {
    let v = json_of(IPMI_PS, "IPMI")?;
    let arr = v_arr(&v, "ipmi");
    if arr.is_empty() {
        return Ok("IPMI：未检测到 BMC/IPMI 接口（家用台式机通常无；服务器/HEDT 板才带）。如实返回，不编造传感器。".into());
    }
    let base = v_arr(&v, "baseboard").first().cloned().unwrap_or_default();
    let board = if v_str(&base, "Manufacturer").is_empty() {
        String::new()
    } else {
        format!(
            "{} {}",
            v_str(&base, "Manufacturer"),
            v_str(&base, "Product")
        )
    };
    let mut s = String::from("IPMI 服务器接口：\n");
    if !board.is_empty() {
        s.push_str(&format!("- 主板：{board}\n"));
    }
    s.push_str(&format!("- 检测到 IPMI/BMC 接口实例：{} 个\n", arr.len()));
    for (i, it) in arr.iter().enumerate() {
        s.push_str(&format!(
            "  - 接口 {}：{}\n",
            i + 1,
            v_str(it, "InstanceName")
        ));
    }
    s.push_str("- BMC 传感器/系统事件日志（SEL）需 IPMI 工具（ipmitool）读取，本工具集不含。");
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_ipmi() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

/// 主板 BIOS PS 脚本仅 Windows 运行，Linux 下无消费方。
#[cfg(windows)]
const BOARD_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=[ordered]@{
  board=@(Get-CimInstance Win32_BaseBoard | Select-Object Manufacturer,Product,SerialNumber)
  bios=@(Get-CimInstance Win32_BIOS | Select-Object Manufacturer,SMBIOSBIOSVersion,ReleaseDate)
  sys=@(Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer,Model)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 主板 / BIOS / 整机品牌采集。
#[cfg(windows)]
pub fn collect_motherboard() -> Result<String, String> {
    let v = json_of(BOARD_PS, "主板")?;
    // PS 里已用 @(...) 包成数组；这里同时兼容「是数组」和「退化成了单个对象」两种形态。
    let pick = |k: &str| -> Value {
        match v.get(k) {
            Some(Value::Array(a)) if !a.is_empty() => a[0].clone(),
            Some(o @ Value::Object(_)) => o.clone(),
            _ => Value::default(),
        }
    };
    let board = pick("board");
    let bios = pick("bios");
    let sys = pick("sys");
    // 厂商没填时 WMI 会塞占位串：经典的是 "To Be Filled By O.E.M."，
    // 还有 "Default string"（这台机器的 Gigabyte 主板就是这样）。都按未知处理。
    let placeholder = |s: &str| -> String {
        let t = s.trim();
        if t.is_empty() {
            "未知".to_string()
        } else if t.eq_ignore_ascii_case("To Be Filled By O.E.M.")
            || t.eq_ignore_ascii_case("Default string")
            || t.eq_ignore_ascii_case("Unknown")
        {
            format!("未知（原始占位串「{t}」）")
        } else {
            t.to_string()
        }
    };
    let mut s = String::new();
    s.push_str(&format!(
        "- 主板厂商：{}\n",
        placeholder(&v_str(&board, "Manufacturer"))
    ));
    s.push_str(&format!(
        "- 主板型号：{}\n",
        placeholder(&v_str(&board, "Product"))
    ));
    s.push_str(&format!(
        "- 主板序列号：{}\n",
        placeholder(&v_str(&board, "SerialNumber"))
    ));
    s.push_str(&format!(
        "- BIOS 厂商：{}\n",
        placeholder(&v_str(&bios, "Manufacturer"))
    ));
    s.push_str(&format!(
        "- BIOS 版本：{}\n",
        placeholder(&v_str(&bios, "SMBIOSBIOSVersion"))
    ));
    let rd = v_str(&bios, "ReleaseDate");
    if rd.trim().is_empty() {
        s.push_str("- BIOS 发布日期：未知\n");
    } else {
        s.push_str(&format!("- BIOS 发布日期：{}\n", fmt_wmi_date(&rd)));
    }
    s.push_str(&format!(
        "- 整机品牌：{}\n",
        placeholder(&v_str(&sys, "Manufacturer"))
    ));
    s.push_str(&format!(
        "- 整机机型：{}",
        placeholder(&v_str(&sys, "Model"))
    ));
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_motherboard() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

// ── 5. 温度 ───────────────────────────────────────────────────────────────
