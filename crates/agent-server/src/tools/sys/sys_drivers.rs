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

/// `DriverDate` → `YYYY-MM-DD`；解析失败原样透传，绝不猜。
/// 仅供 cfg(windows) 的 collect_drivers 使用。
///
/// 两种真实形态都要认：
/// - PowerShell `ConvertTo-Json` 把 CIM DateTime 序列化成 `/Date(1150848000000)/`
///   （有时带时区偏移）——实测驱动列表 138 条全是这个形态，以前只认下面那种，
///   于是界面上整列都是 `/Date(1150848000000)/` 这种没人看得懂的串；
/// - CIM DateTime 数字串 `20260410000000.000000-000`。
#[cfg(windows)]
fn fmt_driver_date(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.starts_with("/Date(") {
        return crate::tools::hw_common::fmt_wmi_date(t);
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

/// `DeviceClass` 短码 → 中文。未命中返回原字符串。仅供 cfg(windows) 的 collect_drivers 使用。
#[cfg(windows)]
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

/// 驱动日期字段不可信的情形。仅供 cfg(windows) 的 collect_drivers 使用。
///
/// - 版本号 `10.0.*` 且年份 < 2020：Windows 内置驱动，日期会被累积更新重置（实测 2006-06-21）；
/// - 年份 < 1995：WMI 零值/溢出产物（实测 `10.1.49.9` 的 Intel 驱动报成 1968-07-18），
///   任何 Windows 驱动都不可能来自 1968 年。
#[cfg(windows)]
fn is_microsoft_date_suspect(version: &str, year: u32) -> bool {
    if year == 0 {
        return false;
    }
    year < 1995 || (year < 2020 && version.starts_with("10.0."))
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
    s.push_str(&render_driver_rows(&rows));
    Ok(s)
}

/// 纯渲染（可单测）：驱动明细行 + 日期失真标注。
#[cfg(windows)]
fn render_driver_rows(rows: &[Value]) -> String {
    let mut s = String::new();
    let mut lines: Vec<(String, bool)> = Vec::new();
    for v in rows {
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
        lines.push((
            format!(
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
            ),
            microsoft_suspect,
        ));
    }
    // 命中 3 条以上就折叠成一条汇总：实测日期解析修好后几乎每条内置驱动都命中，
    // 逐行挂注释会让 50 行输出变成 100 行，等于没在说事情。
    let suspects = lines.iter().filter(|(_, sus)| *sus).count();
    if suspects >= 3 {
        let sample = lines
            .iter()
            .find(|(_, sus)| *sus)
            .map(|(l, _)| l.split(" · ").nth(3).unwrap_or("?").to_string())
            .unwrap_or_default();
        for (l, _) in &lines {
            s.push_str(l);
        }
        s.push_str(&format!(
            "  - 其中 {suspects} 条日期字段不可信（例如 {sample}）：Windows 内置驱动的日期会被累积更新重置，个别是 WMI 零值/溢出，\n    \
             这不代表驱动很旧；判断驱动新旧请看版本号。\n"
        ));
    } else {
        for (l, sus) in &lines {
            s.push_str(l);
            if *sus {
                s.push_str("  - （日期字段不可信：Windows 内置驱动日期会被累积更新重置，或为 WMI 零值/溢出）\n");
            }
        }
    }
    s
}

#[cfg(not(windows))]
pub fn collect_drivers(_keyword: Option<String>, _top_n: usize) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn driver_date_parses_the_json_com_form() {
        // 实测驱动列表 138 条全是这个形态，以前只认 CIM 数字串，
        // 于是整列输出成 /Date(1150848000000)/。
        assert_eq!(fmt_driver_date("/Date(1150848000000)/"), "2006-06-21");
        assert_eq!(fmt_driver_date("/Date(1150848000000+0800)/"), "2006-06-21");
    }

    #[test]
    fn driver_date_parses_the_cim_numeric_form() {
        assert_eq!(fmt_driver_date("20260410000000.000000-000"), "2026-04-10");
    }

    #[test]
    fn driver_date_never_invents_a_date() {
        assert_eq!(fmt_driver_date(""), "");
        assert_eq!(fmt_driver_date("unknown"), "unknown");
        assert_eq!(fmt_driver_date("/Date(zzz)/"), "/Date(zzz)/");
    }

    #[test]
    fn parsed_date_enables_the_microsoft_date_suspect_flag() {
        // 日期解析失败时年份读成 0，这个标注以前对 /Date(...) 永远不触发。
        assert!(is_microsoft_date_suspect("10.0.26100.1150", 2006));
        assert!(!is_microsoft_date_suspect("10.0.26100.1150", 2024));
        assert!(!is_microsoft_date_suspect("1.7.16.218", 2006));
        // 实测 10.1.49.9 的 Intel 驱动报成 1968-07-18：任何 Windows 驱动都不可能来自 1968。
        assert!(is_microsoft_date_suspect("10.1.49.9", 1968));
        assert!(!is_microsoft_date_suspect("10.1.49.9", 2023));
    }

    fn drv(name: &str, ver: &str, date: &str) -> Value {
        serde_json::json!({
            "DeviceName": name, "DeviceClass": "System",
            "DriverVersion": ver, "DriverDate": date, "Manufacturer": "Microsoft"
        })
    }

    #[test]
    fn repeated_date_suspects_collapse_into_one_summary() {
        // 实测：日期解析修好后几乎每条内置驱动都命中，逐行挂注释把 50 行变成 100 行。
        let rows = vec![
            drv("ACPI Fan", "10.0.26100.1150", "/Date(1150848000000)/"),
            drv("ACPI Button", "10.0.26100.1150", "/Date(1150848000000)/"),
            drv("ACPI Sleep", "10.0.26100.1150", "/Date(1150848000000)/"),
            drv("AMD Tool", "1.7.16.218", "/Date(1590537600000)/"),
        ];
        let out = render_driver_rows(&rows);
        assert!(
            out.contains("其中 3 条日期字段不可信（例如 2006-06-21）"),
            "{out}"
        );
        assert!(!out.contains("（日期字段不可信"), "{out}");
        assert_eq!(
            out.lines().filter(|l| l.starts_with("- ")).count(),
            4,
            "{out}"
        );
    }

    #[test]
    fn few_date_suspects_keep_the_per_line_note() {
        let rows = vec![
            drv("ACPI Fan", "10.0.26100.1150", "/Date(1150848000000)/"),
            drv("Vendor Driver", "12.3.4", "/Date(1700000000000)/"),
        ];
        let out = render_driver_rows(&rows);
        assert!(out.contains("（日期字段不可信"), "{out}");
        assert!(!out.contains("其中"), "{out}");
    }
}
