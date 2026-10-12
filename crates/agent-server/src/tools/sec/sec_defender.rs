//! 3b) Windows Defender 状态域（从 sec.rs 拆分，2026-10-12）。

// ps_capture / v_str 仅供 cfg(windows) 的 collect_defender_status 使用
//（Linux 仅返回降级文案），随 Windows 实现一起编译。
#[cfg(windows)]
use crate::ps::ps_capture;
#[cfg(windows)]
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
        // 空值必须显式写「未上报」：以前直接拼进模板，实测输出过
        // 「病毒库：（引擎 ）」这种半截句子，读的人无法判断是「没有」还是「没读到」。
        let or_na = |s: String| {
            if s.is_empty() {
                "未上报".to_string()
            } else {
                s
            }
        };
        let sig_part = defender_signature_line(&v_str(&obj, "Signatures"), &v_str(&obj, "Engine"));
        out.push_str(&format!(
            "- 实时保护：{}；{}\n",
            or_na(v_str(&obj, "RealTime")),
            sig_part
        ));
        out.push_str(&format!(
            "- 产品：{}；最近快速扫描：{}\n",
            or_na(v_str(&obj, "Product")),
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

/// 病毒库 / 引擎版本一行。**空值必须显式写「未上报」**：以前直接拼进模板，
/// 实测输出过「病毒库：（引擎 ）」这种半截句子，读的人分不清「没有」还是「没读到」。
/// 纯函数、跨平台，便于单测。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn defender_signature_line(sig: &str, eng: &str) -> String {
    match (sig.is_empty(), eng.is_empty()) {
        (true, true) => "病毒库：未上报".to_string(),
        (false, true) => format!("病毒库：{sig}（引擎版本未上报）"),
        (true, false) => format!("病毒库：未上报（引擎 {eng}）"),
        (false, false) => format!("病毒库：{sig}（引擎 {eng}）"),
    }
}

/// 跑输出「对象」的 PowerShell 脚本（不做数组折叠）。
#[cfg(windows)]
fn json_of_sec(script: &str, what: &str) -> Result<Value, String> {
    let raw = ps_capture(script)?;
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    serde_json::from_str(trimmed).map_err(|e| format!("解析{what}查询结果失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_fields_are_said_out_loudly() {
        // 实测缺陷：这台机器病毒库/引擎版本读不到，输出成「病毒库：（引擎 ）」。
        assert_eq!(defender_signature_line("", ""), "病毒库：未上报");
        assert_eq!(
            defender_signature_line("1.411.15", ""),
            "病毒库：1.411.15（引擎版本未上报）"
        );
        assert_eq!(
            defender_signature_line("", "1.1.24000.0"),
            "病毒库：未上报（引擎 1.1.24000.0）"
        );
    }

    #[test]
    fn both_present_reads_normally() {
        let line = defender_signature_line("1.411.15", "1.1.24000.0");
        assert_eq!(line, "病毒库：1.411.15（引擎 1.1.24000.0）");
        assert!(!line.contains("未上报"), "{line}");
    }
}
