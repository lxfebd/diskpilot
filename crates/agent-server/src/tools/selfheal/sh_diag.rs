//! 诊断域（从 selfheal.rs 拆分，2026-10-12）。
//! parse_diag（两平台版）+ selfheal_diag（两平台版）。

use serde_json::{json, Value};

// sh_core 全为 Windows 实现，仅 cfg(windows) 分支引用。
#[cfg(windows)]
use super::sh_core::{find_fancmd, run_fancmd};

/// 解析 fancmd diag json，归一化为本工具返回结构。
#[cfg(windows)]
pub(crate) fn parse_diag(raw: &str) -> Value {
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    let v: Value = serde_json::from_str(trimmed).unwrap_or_else(|_| json!({ "parsed": false }));
    if v.get("parsed").and_then(|x| x.as_bool()) == Some(false) {
        return json!({ "parsed": false, "raw": trimmed.chars().take(1200).collect::<String>() });
    }
    let driver_loaded = v
        .get("driver")
        .and_then(|d| d.get("loaded"))
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let sio_found = v
        .get("sio")
        .and_then(|s| s.get("found"))
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let writable = v.get("writable").and_then(|x| x.as_bool()).unwrap_or(false);
    // 可修复点：缺驱动 / 驱动在但端口不通 / 芯片不可识别 / 可写（无事可修）
    let mut fix_points: Vec<String> = Vec::new();
    if !driver_loaded {
        fix_points.push(
            "端口驱动未加载：需以管理员安装 inpoutx64（fan_selfheal_fix action=install_driver）"
                .into(),
        );
    }
    if driver_loaded && v.get("rtc_ok").and_then(|x| x.as_bool()) == Some(false) {
        fix_points.push(
            "驱动已加载但 RTC 端口读写验证失败：驱动版本/位数不匹配，建议换 inpoutx32 或 WinRing0"
                .into(),
        );
    }
    if driver_loaded && !sio_found {
        fix_points.push("SuperIO 芯片不可识别（已试 ITE/Nuvoton/Winbond × 0x2E/0x4E）：主板不兼容或需要更全的芯片表".into());
    }
    if driver_loaded && sio_found && !writable {
        fix_points.push("芯片已识别但写通道不可用：请检查风扇是否支持软件控制".into());
    }
    json!({
        "driver_loaded": driver_loaded,
        "driver_name": v.get("driver").and_then(|d| d.get("name")).and_then(|x| x.as_str()).unwrap_or(""),
        "rtc_ok": v.get("rtc_ok").and_then(|x| x.as_bool()).unwrap_or(false),
        "sio_found": sio_found,
        "sio": v.get("sio").cloned().unwrap_or(json!({})),
        "writable": writable,
        "fans": v.get("fans").cloned().unwrap_or(json!([])),
        "fix_points": json!(fix_points),
        "_raw": v,
    })
}

// 非 Windows 降级版仅供 Windows 侧符号对称编译，Linux 下无调用方（调用方均 cfg(windows)）。
#[cfg(not(windows))]
#[allow(dead_code)]
pub(crate) fn parse_diag(_raw: &str) -> Value {
    json!({ "parsed": false, "note": "非 Windows 平台，风扇自修复不可用" })
}

/// 自修复诊断：跑 fancmd diag json 并结构化返回（只读 L0）。
#[cfg(windows)]
pub fn selfheal_diag() -> Result<String, String> {
    if find_fancmd().is_none() {
        return Ok(json!({
            "fancmd_found": false,
            "note": "未找到 fancmd.exe（磁盘清理工具包未装可写调速桥）。装好 tools/fancmd 后重试。",
            "fix_points": ["安装 fancmd（DiskPilot 风扇写控桥）"]
        })
        .to_string());
    }
    let raw = run_fancmd(&["diag", "json"])?;
    let parsed = parse_diag(&raw);
    if parsed.get("parsed").and_then(|x| x.as_bool()) == Some(false) {
        return Ok(json!({
            "fancmd_found": true,
            "diag_unparsed": true,
            "raw": parsed.get("raw"),
            "note": "fancmd 返回无法解析：可能驱动加载失败或进程异常，见 raw。"
        })
        .to_string());
    }
    let mut fp = parsed
        .get("fix_points")
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(fans) = parsed.get("fans").and_then(|x| x.as_array()) {
        // 附加一条可写通道结论
        if !fp.is_empty() {
            fp.push(
                if parsed.get("writable").and_then(|x| x.as_bool()) == Some(true) {
                    "风扇通道可写：可用 fan_control（主项目 L2）或 fancmd write 调速。"
                } else {
                    "风扇通道当前不可写：按上方 fix_points 修复后重试。"
                },
            );
        }
        let _ = fans;
    }
    Ok(json!({
        "fancmd_found": true,
        "driver_loaded": parsed.get("driver_loaded"),
        "driver_name": parsed.get("driver_name"),
        "rtc_ok": parsed.get("rtc_ok"),
        "sio_found": parsed.get("sio_found"),
        "sio": parsed.get("sio"),
        "writable": parsed.get("writable"),
        "fans": parsed.get("fans"),
        "fix_points": json!(fp),
    })
    .to_string())
}

#[cfg(not(windows))]
pub fn selfheal_diag() -> Result<String, String> {
    Ok(json!({ "fancmd_found": false, "note": "非 Windows 平台，风扇自修复不可用" }).to_string())
}
