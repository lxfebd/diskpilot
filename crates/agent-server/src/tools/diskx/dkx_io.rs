//! 磁盘 IO 使用率域（从 diskx.rs 拆分，2026-10-12）。
//! DISK_IO_PS + collect_disk_io_usage + fmt_rate（两平台版）。

use crate::ps::{json_array_of, ps_capture};

const DISK_IO_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$rows = @(Get-CimInstance Win32_PerfFormattedData_PerfDisk_PhysicalDisk | Where-Object {
    $_.Name -eq '_Total' -or ($_.Name -match '^\d+ [A-Z]:$')
} | Select-Object Name, DiskReadBytesPerSec, DiskWriteBytesPerSec, PercentDiskTime)
$rows | ConvertTo-Json -Depth 4 -Compress
"#;

/// 磁盘 IO 使用率（只读，实时）。
///
/// 走 CIM `Win32_PerfFormattedData_PerfDisk_PhysicalDisk`，取格式化后的当前速率
/// （单位：字节/秒）。过滤规则：`_Total` = 总计，`<整数> <字母>:` = 真实磁盘（例 `0 C:`），
/// `HarddiskN` / 双前缀副本（例 `2 C:`）等全部丢弃。
///
/// 参数 `sample_ms` 仅作参考提示——PerfFormattedData 提供的是当前速率，不做真实双采样。
#[cfg(windows)]
pub fn collect_disk_io_usage(sample_ms: u64) -> Result<String, String> {
    let raw = ps_capture(DISK_IO_PS)?;
    let arr: Vec<serde_json::Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析磁盘性能计数器失败：{e}"))?;

    if arr.is_empty() {
        return Ok(
            "未读取到磁盘性能计数器（Win32_PerfFormattedData_PerfDisk_PhysicalDisk 无返回）".into(),
        );
    }

    let get_u64 = |v: &serde_json::Value, k: &str| -> u64 {
        v.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0)
    };

    let mut lines: Vec<String> = Vec::with_capacity(arr.len());
    for r in &arr {
        let name = r
            .get("Name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let rb = get_u64(r, "DiskReadBytesPerSec");
        let wb = get_u64(r, "DiskWriteBytesPerSec");
        let pct = get_u64(r, "PercentDiskTime");

        let label = if name == "_Total" {
            "总计".to_string()
        } else {
            // "0 C:" → "C:"（去掉数字前缀）；非标准格式原样返回。
            match name.split_once(' ') {
                Some((_, drive)) => drive.to_string(),
                None => name.clone(),
            }
        };
        lines.push(format!(
            "- {label} · 读 {} · 写 {} · 磁盘占用 {}%",
            fmt_rate(rb),
            fmt_rate(wb),
            pct
        ));
    }

    if lines.is_empty() {
        return Ok("磁盘性能计数器无有效行（过滤规则可能过于严格）".into());
    }

    let mut s = format!("磁盘 IO 实时使用率：\n{}\n", lines.join("\n"));
    s.push_str(&format!(
        "说明：数据来源 Win32_PerfFormattedData_PerfDisk_PhysicalDisk（格式化计数器，当前速率）；\
         sample_ms={sample_ms} 仅作提示，未做真实双采样。"
    ));
    Ok(s)
}

/// 字节/秒 → 人类可读速率（`{v} MB/s` / `{v} KB/s` / `{n} B/s`）。
fn fmt_rate(bps: u64) -> String {
    if bps >= 1_024 * 1_024 {
        format!("{:.*} MB/s", 1, bps as f64 / 1024.0 / 1024.0)
    } else if bps >= 1024 {
        format!("{:.*} KB/s", 0, bps as f64 / 1024.0)
    } else {
        format!("{bps} B/s")
    }
}

#[cfg(not(windows))]
pub fn collect_disk_io_usage(_sample_ms: u64) -> Result<String, String> {
    Ok("当前平台不是 Windows，磁盘 IO 使用率不可用".into())
}
