//! 电池采集：电量/健康度/循环次数（只读，普通权限）。
//!
//! 从 hw.rs 拆分（2026-10-12）。
// 本文件采集实现均为 cfg(windows)（Linux 仅保留返回 Err 的 collect_battery），
// 顶层 import 随 Windows 实现一起编译。
#[cfg(windows)]
use crate::tools::fmt_bytes;
#[cfg(windows)]
use crate::tools::hw_common::{get_int, json_of, v_arr};
#[cfg(windows)]
use crate::tools::v_str;
#[cfg(windows)]
use serde_json::Value;

// ── 6. 电池 ───────────────────────────────────────────────────────────────

/// 电池 PS 脚本仅 Windows 运行，Linux 下无消费方。
#[cfg(windows)]
const BATTERY_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$bats=@(Get-CimInstance Win32_Battery | Select-Object DeviceID,Name,ManufacturerName,SerialNumber,DesignCapacity,FullChargeCapacity,EstimatedChargeRemaining,EstimatedRunTime,BatteryStatus,Temperature,Chemistry)
$sysb=Get-CimInstance Win32_SystemBattery
$bstat=Get-CimInstance Win32_BatteryStatus
$cycles=null
if($sysb){ $cycles=@($sysb | ForEach-Object { @($_.SystemBatteryInformation | Select -Expand CycleCount) -join "," } ) }
$out=[ordered]@{
  battery=$bats
  cycles=if($cycles){$cycles}else{[ordered]@{}}
  power_online=if($bstat){[bool]$bstat.PowerOnline}else{$null}
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 电池采集：电量/剩余时间/健康度/循环次数。台式机无电池时返回说明而非 `Err`。
#[cfg(windows)]
pub fn collect_battery() -> Result<String, String> {
    let v = json_of(BATTERY_PS, "电池")?;
    let arr = v_arr(&v, "battery");
    if arr.is_empty() {
        return Ok("未检测到电池（台式机或电池不可用）".into());
    }
    let b = &arr[0];
    let mut s = String::new();
    s.push_str("- 电池：");
    if v_str(b, "Name").is_empty() {
        s.push_str("型号未提供");
    } else {
        s.push_str(&v_str(b, "Name"));
    }
    if !v_str(b, "DeviceID").is_empty() {
        s.push_str(&format!("（{0}）", v_str(b, "DeviceID")));
    }
    s.push('\n');
    let pct = get_int(b, "EstimatedChargeRemaining").unwrap_or(0);
    s.push_str(&format!("  - 当前电量：{pct}%\n"));
    let mins = get_int(b, "EstimatedRunTime").unwrap_or(0);
    if mins > 0 {
        s.push_str(&format!(
            "  - 预估剩余：{mins} 分钟（约 {:.*} 小时）\n",
            1,
            mins as f64 / 60.0
        ));
    } else {
        s.push_str("  - 预估剩余：读不到（未供电或 WMI 无估算）\n");
    }
    let code = get_int(b, "BatteryStatus").unwrap_or(0);
    let status = match code {
        1 => "使用电池（放电中）",
        2 => "正在充电",
        3 => "已充满",
        4 => "过热",
        5 => "电池错误",
        6 => "需要更换",
        7 => "电池不存在",
        8 => "需要冷却",
        9 => "电池已耗尽",
        10 => "安全错误",
        0 => "未知",
        _ => {
            s.push_str(&format!(
                "  - 电池状态码：未知数值 {code}（Win32_Battery.BatteryStatus）\n"
            ));
            "未知"
        }
    };
    let po = v.get("power_online").and_then(Value::as_bool);
    let po_str = match po {
        Some(true) => "（已插电源）",
        Some(false) => "（未插电源）",
        None => "（PowerOnline 未提供）",
    };
    s.push_str(&format!("  - 状态：{status}{po_str}\n"));
    let design = get_int(b, "DesignCapacity").unwrap_or(0);
    let full = get_int(b, "FullChargeCapacity").unwrap_or(0);
    s.push_str(&format!(
        "  - 设计容量：{}（{} mAh）\n",
        if design > 0 {
            fmt_bytes(design * 1000)
        } else {
            "未知".into()
        },
        design
    ));
    s.push_str(&format!(
        "  - 满充容量：{}（{} mAh）\n",
        if full > 0 {
            fmt_bytes(full * 1000)
        } else {
            "未知".into()
        },
        full
    ));
    if design > 0 && full > 0 {
        let h = full as f64 / design as f64 * 100.0;
        let tag = if h < 80.0 {
            "，健康度低于 80%，建议更换电池"
        } else if h < 90.0 {
            "，有可感知衰减"
        } else {
            ""
        };
        s.push_str(&format!(
            "  - 健康度：{:.*}%（满充容量 / 设计容量）{tag}\n",
            1, h
        ));
    } else {
        s.push_str("  - 健康度：无法计算（设计容量或满充容量读不到）\n");
    }
    // Win32_SystemBattery.CycleCount 是嵌套数组；脚本里已拼成逗号分隔字符串。
    let mut cycles: Vec<u64> = Vec::new();
    for cv in v_arr(&v, "cycles") {
        match cv {
            Value::String(s) => {
                for part in s.split(',') {
                    if let Ok(n) = part.trim().parse::<u64>() {
                        cycles.push(n);
                    }
                }
            }
            Value::Null => cycles.push(0),
            _ => {}
        }
    }
    if cycles.is_empty() {
        s.push_str("  - 循环次数：未提供（Win32_SystemBattery 无 CycleCount）\n");
    } else if cycles.iter().all(|&n| n == 0) {
        s.push_str("  - 循环次数：未提供（CycleCount 为 0 或空）\n");
    } else {
        let cs = cycles
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("、");
        s.push_str(&format!("  - 循环次数：{cs}\n"));
    }
    let mf = v_str(b, "ManufacturerName");
    if !mf.is_empty() {
        s.push_str(&format!("  - 电池厂商：{mf}\n"));
    }
    let chem = v_str(b, "Chemistry");
    if !chem.is_empty() {
        s.push_str(&format!("  - 化学体系：{chem}\n"));
    }
    let sn = v_str(b, "SerialNumber");
    if !sn.is_empty() {
        s.push_str(&format!("  - 序列号：{sn}\n"));
    }
    let temp = get_int(b, "Temperature").unwrap_or(0);
    // Win32_Battery.Temperature 单位是 0.1 ℃
    if temp > 0 {
        s.push_str(&format!(
            "  - 电池温度：{:.*} ℃（原始 {temp}，单位 0.1 ℃）",
            1,
            temp as f64 / 10.0
        ));
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_battery() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}
