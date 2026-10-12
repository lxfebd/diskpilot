//! 驱动枚举域（从 sys.rs 拆分，2026-10-12）。
//! collect_drivers：Win32_PnPSignedDriver + 微软日期失真标注。

#[cfg(windows)]
use serde_json::Value;

#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use crate::ps::{json_array_of, ps_capture_timeout};

#[cfg(windows)]
use crate::tools::v_str;

#[cfg(not(windows))]
use super::NOT_WINDOWS;

// ── 2. 驱动枚举 ─────────────────────────────────────────────────────────

#[cfg(windows)]
const DRIVERS_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-CimInstance Win32_PnPSignedDriver | Where-Object { $_.DeviceName -and $_.DriverVersion } | Sort-Object DeviceName -Unique | Select-Object DeviceName,DeviceClass,DriverVersion,DriverDate,Manufacturer)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// `DriverDate` 的 CIM DateTime（如 `20260410000000.000000-000`）→ `YYYY-MM-DD`。
/// 只取前 8 位数字，安全解析；失败原样透传。
fn fmt_driver_date(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    let digits: Vec<char> = t.chars().take(8).collect();
    if digits.len() == 8 && digits.iter().all(|c| c.is_ascii_digit()) {
        let ds: String = digits.into_iter().collect();
        let y: u32 = ds[0..4].parse().unwrap_or(0);
        let m: u32 = ds[4..6].parse().unwrap_or(0);
        let d: u32 = ds[6..8].parse().unwrap_or(0);
        if (1900..=2099).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d) {
            return format!("{y:04}-{m:02}-{d:02}");
        }
    }
    t.to_string()
}

/// `DeviceClass` 短码 → 中文。未命中返回原字符串。
fn device_class_zh(s: &str) -> String {
    match s {
        "NET" | "Net" => "网络适配器".to_string(),
        "Display" => "显示".to_string(),
        "USB" => "USB".to_string(),
        "Media" => "多媒体".to_string(),
        "Image" => "成像".to_string(),
        "Audio" => "音频".to_string(),
        "DiskDrive" => "磁盘".to_string(),
        "System" => "系统".to_string(),
        "Bluetooth" => "蓝牙".to_string(),
        "SCSIAdapter" => "SCSI".to_string(),
        "" => "未知".to_string(),
        other => other.to_string(),
    }
}

/// 微软驱动日期失真检测：年份 < 2020 且版本号形如 `10.0.*`。
fn is_microsoft_date_suspect(version: &str, year: u32) -> bool {
    year > 0 && year < 2020 && version.starts_with("10.0.")
}

/// 驱动枚举：PowerShell `Win32_PnPSignedDriver`（真机 137 条跑通）。
/// 用 `ps_capture_timeout` 放宽到 60s——137 条 CIM 查询需要数秒。
#[cfg(windows)]
pub fn collect_drivers(keyword: Option<String>, top_n: usize) -> Result<String, String> {
    let top_n = top_n.clamp(1, 200);
    let kw: Option<String> = keyword
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| !s.is_empty());

    let raw = ps_capture_timeout(DRIVERS_PS, Duration::from_secs(60))?;
    let arr_str = json_array_of(raw.trim().trim_start_matches('\u{feff}'));
    let arr: Vec<Value> =
        serde_json::from_str(&arr_str).map_err(|e| format!("解析驱动列表失败：{e}"))?;

    if arr.is_empty() {
        return Ok("驱动列表为空（Win32_PnPSignedDriver 无返回）".into());
    }

    let mut rows: Vec<Value> = arr;
    if let Some(k) = &kw {
        rows.retain(|v| {
            v_str(v, "DeviceName").to_ascii_lowercase().contains(k)
                || v_str(v, "Manufacturer").to_ascii_lowercase().contains(k)
        });
    }
    // 排序：按 DeviceName 字母序（PS 已 -Unique 排过，这里保底）。
    rows.sort_by(|a, b| {
        v_str(a, "DeviceName")
            .to_ascii_lowercase()
            .cmp(&v_str(b, "DeviceName").to_ascii_lowercase())
    });
    let total_before = rows.len();
    rows.truncate(top_n);

    if rows.is_empty() {
        return Ok(format!(
            "没有 DeviceName/Manufacturer 匹配「{}」的驱动",
            kw.unwrap_or_default()
        ));
    }

    let mut s = String::new();
    s.push_str(&format!(
        "驱动列表（共 {} 条{}）：\n",
        total_before,
        if total_before > rows.len() {
            format!("，只显示前 {}", rows.len())
        } else {
            String::new()
        }
    ));
    for v in &rows {
        let name = if v_str(v, "DeviceName").is_empty() {
            "未知设备".to_string()
        } else {
            v_str(v, "DeviceName")
        };
        let cls = device_class_zh(&v_str(v, "DeviceClass"));
        let ver = if v_str(v, "DriverVersion").is_empty() {
            "未知".to_string()
        } else {
            v_str(v, "DriverVersion")
        };
        let date_raw = v_str(v, "DriverDate");
        let date_fmt = fmt_driver_date(&date_raw);
        // 年份解析用于微软驱动假日期检测。
        let year: u32 = date_fmt
            .chars()
            .take(4)
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        let microsoft_suspect = is_microsoft_date_suspect(&ver, year);
        let man = if v_str(v, "Manufacturer").is_empty() {
            "未知厂商".to_string()
        } else {
            v_str(v, "Manufacturer")
        };
        s.push_str(&format!(
            "- {} · {} · {} · {} · {}\n",
            name,
            cls,
            ver,
            if date_fmt.is_empty() {
                "未知"
            } else {
                &date_fmt
            },
            man
        ));
        if microsoft_suspect {
            s.push_str("  - （微软驱动日期可能失真：版本号 10.0.* 但日期字段异常旧，Windows 累积更新会重置该字段）\n");
        }
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_drivers(_keyword: Option<String>, _top_n: usize) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}
