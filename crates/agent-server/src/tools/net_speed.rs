//! 每张网卡实时下载/上传速率（PS 双采样）。
//!
//! from tools/net.rs 拆分（2026-10-12）。

use serde_json::Value;

use crate::tools::fmt_bytes;
use crate::tools::net_util::{fmt_rate, get_f64, get_int, json_arr_of};
use crate::tools::v_str;

// ── 3. 实时上下行速率 ──────────────────────────────────────────────────

/// 每张网卡实时下载/上传速率。单次 PS 内双采样（`Get-NetAdapterStatistics` 字段是
/// `ReceivedBytes` / `SentBytes`），`interval_ms` 钳到 1000..=10000。
#[cfg(windows)]
pub fn collect_net_speed(interval_ms: u64) -> Result<String, String> {
    let ms = interval_ms.clamp(1000, 10000);
    let template = r#"
$ErrorActionPreference='SilentlyContinue'
$before=@{}
foreach($s in (Get-NetAdapterStatistics -ErrorAction SilentlyContinue)){
    if($s.Name){ $before[[string]$s.Name]=@([uint64]$s.ReceivedBytes,[uint64]$s.SentBytes) }
}
$start=Get-Date
Start-Sleep -Milliseconds __MS__
$elapsed=(Get-Date).Subtract($start).TotalMilliseconds/1000.0
$out=@(foreach($s in (Get-NetAdapterStatistics -ErrorAction SilentlyContinue)){
    $name=[string]$s.Name
    if(-not $name){ continue }
    $b=$before[[string]$s.Name]
    $dl=if($elapsed -gt 0){[math]::Max(0.0,[double]([uint64]$s.ReceivedBytes-[uint64]$b[0])/$elapsed)}else{0}
    $ul=if($elapsed -gt 0){[math]::Max(0.0,[double]([uint64]$s.SentBytes-[uint64]$b[1])/$elapsed)}else{0}
    [ordered]@{
        Name=$name
        Description=[string]$s.Description
        DownloadBps=$dl
        UploadBps=$ul
        TotalRecv=[uint64]$s.ReceivedBytes
        TotalSent=[uint64]$s.SentBytes
        IsVirtual=($name -match 'Hyper-V|VMware|Bluetooth|Loopback|vEthernet|Virtual|VirtualBox|VPN|TAP-Win|WSL|Tunnel' -or $s.Description -match 'Hyper-V|VMware|Bluetooth|Loopback|vEthernet|Virtual|VirtualBox|VPN|TAP-Win|WSL|Tunnel')
    }
})
$out | ConvertTo-Json -Depth 5 -Compress
"#;
    let script = template.replace("__MS__", &ms.to_string());
    let arr = json_arr_of(&script, "网卡速率")?;
    if arr.is_empty() {
        return Ok("未检测到网卡或无法读取流量计数器".into());
    }
    let mut out = format!("网卡实时速率（{} ms 采样）：\n", ms);
    let mut total_dl = 0.0f64;
    let mut total_ul = 0.0f64;
    let mut real_count = 0usize;
    for v in &arr {
        let name = v_str(v, "Name");
        if name.is_empty() {
            continue;
        }
        let desc = v_str(v, "Description");
        let label = if !desc.is_empty() && desc != name {
            format!("{}（{}）", name, desc)
        } else {
            name.clone()
        };
        if matches!(v.get("IsVirtual"), Some(Value::Bool(true))) {
            out.push_str(&format!("- {}（虚拟，无真实流量）\n", label));
            continue;
        }
        real_count += 1;
        let dl = get_f64(v, "DownloadBps").unwrap_or(0.0);
        let ul = get_f64(v, "UploadBps").unwrap_or(0.0);
        let recv = get_int(v, "TotalRecv").unwrap_or(0) as u64;
        let sent = get_int(v, "TotalSent").unwrap_or(0) as u64;
        total_dl += dl;
        total_ul += ul;
        out.push_str(&format!(
            "- {} · 下载 {} · 上传 {} · 累计收 {} / 发 {}\n",
            label,
            fmt_rate(dl),
            fmt_rate(ul),
            fmt_bytes(recv),
            fmt_bytes(sent)
        ));
    }
    if real_count > 1 {
        out.push_str(&format!(
            "- 合计 · 下载 {} · 上传 {}\n",
            fmt_rate(total_dl),
            fmt_rate(total_ul)
        ));
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_net_speed(_interval_ms: u64) -> Result<String, String> {
    Ok("当前平台不是 Windows，网络信息不可用".into())
}
