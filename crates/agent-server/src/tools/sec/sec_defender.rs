//! 3b) Windows Defender 状态域（从 sec.rs 拆分，2026-10-12）。

use crate::ps::ps_capture;
use crate::tools::v_str;

#[cfg(windows)]
use serde_json::Value;

#[cfg(not(windows))]
use super::NOT_WINDOWS;

// ── 3. Windows Defender 状态 ────────────────────────────────────────────

/// Defender 状态采集：服务状态 + 实时保护 + 病毒库版本 + 最近扫描。
///
/// 全部只读；`Get-MpComputerStatus` 普通用户可读（winmgmt），无需管理员。
/// 若系统用第三方杀软（Defender 被替换），如实返回「未启用」而非编造。
#[cfg(windows)]
pub fn collect_defender_status() -> Result<String, String> {
    let script = r#"
$ErrorActionPreference='SilentlyContinue'
$svc = Get-Service -Name WinDefend
$svcStatus = ''
$svcStart = ''
if ($null -ne $svc) { $svcStatus = [string]$svc.Status; $svcStart = [string]$svc.StartType }
$mpOk = $false
$mpErr = ''
$realTime = ''
$sig = ''
$engine = ''
$product = ''
$lastScan = ''
try {
  $s = Get-MpComputerStatus -ErrorAction Stop
  $mpOk = $true
  $realTime = if ($s.RealTimeProtectionEnabled) { '已启用' } else { '未启用' }
  $sig = [string]$s.AntivirusSignatureVersion
  $engine = [string]$s.AntivirusEngineVersion
  $product = [string]$s.AMProductVersion
  $ls = $s.QuickScanEndTime
  $lastScan = if ($null -ne $ls) { $ls.ToString('yyyy-MM-dd HH:mm') } else { '' }
} catch {
  $mpErr = $_.Exception.Message
}
[ordered]@{
  ServiceOk = $null -ne $svc
  ServiceStatus = $svcStatus
  ServiceStartType = $svcStart
  MpOk = $mpOk
  MpError = $mpErr
  RealTime = $realTime
  Signatures = $sig
  Engine = $engine
  Product = $product
  LastScan = $lastScan
} | ConvertTo-Json -Compress
"#;
    let obj = json_of_sec(script, "Defender 状态")?;
    let svc_ok = obj
        .get("ServiceOk")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let svc_status = v_str(&obj, "ServiceStatus");
    let svc_start = v_str(&obj, "ServiceStartType");

    let mut out = String::new();
    if svc_ok {
        out.push_str(&format!(
            "- WinDefend 服务：{svc_status}（启动类型 {svc_start}）\n",
            svc_start = svc_start.to_ascii_lowercase()
        ));
    } else {
        out.push_str("- WinDefend 服务：未安装或不可用\n");
    }

    let mp_ok = obj.get("MpOk").and_then(Value::as_bool).unwrap_or(false);
    if mp_ok {
        out.push_str(&format!(
            "- 实时保护：{}；病毒库：{}（引擎 {}）\n",
            v_str(&obj, "RealTime"),
            v_str(&obj, "Signatures"),
            v_str(&obj, "Engine")
        ));
        out.push_str(&format!(
            "- 产品：{}；最近快速扫描：{}\n",
            v_str(&obj, "Product"),
            if v_str(&obj, "LastScan").is_empty() {
                "从未扫描".to_string()
            } else {
                v_str(&obj, "LastScan")
            }
        ));
    } else {
        let err = v_str(&obj, "MpError");
        if err.is_empty() {
            out.push_str(
                "- 实时保护：无法读取（Get-MpComputerStatus 不可用，通常为第三方杀软占位）\n",
            );
        } else {
            out.push_str(&format!(
                "- 实时保护读取失败：{}（通常为第三方杀软占位）\n",
                err.chars().take(120).collect::<String>()
            ));
        }
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_defender_status() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

/// 跑输出「对象」的 PowerShell 脚本（不做数组折叠）。
#[cfg(windows)]
fn json_of_sec(script: &str, what: &str) -> Result<Value, String> {
    let raw = ps_capture(script)?;
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    serde_json::from_str(trimmed).map_err(|e| format!("解析{what}查询结果失败：{e}"))
}
