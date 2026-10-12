//! 事件日志域（从 sec.rs 拆分，2026-10-12）。
//! collect_event_logs：System / Application 等普通权限可读的系统事件。

// ps_capture / v_str 仅供 cfg(windows) 的 collect_event_logs 使用
//（Linux 仅返回降级文案），随 Windows 实现一起编译。
#[cfg(windows)]
use crate::ps::ps_capture;
#[cfg(windows)]
use crate::tools::sec::sec_common::{level_cn, trunc_msg, v_arr, validate_log_name};
#[cfg(windows)]
use crate::tools::v_str;

#[cfg(windows)]
use serde_json::Value;

#[cfg(not(windows))]
use super::NOT_WINDOWS;

// ── 1. 事件日志（System / Application 等普通权限可读）──────────────────

/// 事件日志查询脚本模板：先用 `-LogName` 做权限探测（Security 会抛 UnauthorizedAccess），
/// 探测通过再跑 `-FilterHashtable` 过滤错误/警告。
///
/// **权限陷阱**：`-FilterHashtable @{LogName='Security'}` 不抛异常，只返回空
/// （server-side 过滤绕过权限检查）；所以必须先 `-LogName` 探测。
/// `$err` 收集探测或查询阶段的异常，由 Rust 侧降级。
/// 仅 PowerShell 查询使用（Linux 下无消费方）。
#[cfg(windows)]
const EVENT_LOG_PS_TMPL: &str = r#"
$log = '__LOG__'
$hours = __HOURS__
$n = __MAX__
$err = ''
$events = @()
$probeOk = $false
$probeErr = ''
# 先做权限探测：直接按日志名拿一条，Security 会抛 UnauthorizedAccessException；
# FilterHashtable 走 server-side 过滤反而不抛，只返回空。所以先用 -LogName 探测。
try {
  $null = @(Get-WinEvent -LogName $log -MaxEvents 1 -ErrorAction Stop | ForEach-Object { $_.Id })
  $probeOk = $true
} catch {
  $probeErr = $_.Exception.Message
}
if ($probeOk) {
  try {
    $events = @(Get-WinEvent -FilterHashtable @{LogName=$log; Level=2,3; StartTime=(Get-Date).AddHours(-$hours)} -MaxEvents $n -ErrorAction Stop | ForEach-Object {
      [ordered]@{
        TimeCreated = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss')
        Id = $_.Id
        Level = $_.Level
        ProviderName = [string]$_.ProviderName
        Message = [string]$_.Message
      }
    })
  } catch {
    $err = $_.Exception.Message
  }
} else {
  $err = $probeErr
}
[ordered]@{
  LogName = $log
  Events = $events
  Err = $err
  Success = if ($err -eq '') { $true } else { $false }
} | ConvertTo-Json -Depth 5 -Compress
"#;

/// 事件日志采集：逗号分隔的日志名列表（默认 `System,Application`），
/// 时间窗 `hours` 钳 1..=720，每日志最多 `max_events` 条（钳 1..=50）。
///
/// **降级**：Security 等需要管理员权限的日志不整体失败，只输出
/// `- [Security] 需要管理员权限，无法读取` 并继续下一个日志。
#[cfg(windows)]
pub fn collect_event_logs(
    logs: Option<String>,
    hours: u32,
    max_events: usize,
) -> Result<String, String> {
    let logs_list: Vec<String> = match logs {
        Some(s) if !s.trim().is_empty() => s
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => vec!["System".to_string(), "Application".to_string()],
    };
    if logs_list.is_empty() {
        return Ok("未指定要查询的日志（logs 为空或只含分隔符）".into());
    }
    let hours = if hours < 1 { 24 } else { hours.min(720) };
    let max_events = if max_events < 1 {
        20
    } else {
        max_events.min(50)
    };

    let mut out = String::new();
    let mut any_success = false;
    for raw_name in &logs_list {
        let log = match validate_log_name(raw_name) {
            Ok(l) => l,
            Err(e) => {
                out.push_str(&format!("- {e}\n"));
                continue;
            }
        };
        let script = EVENT_LOG_PS_TMPL
            .replace("__LOG__", &log)
            .replace("__HOURS__", &hours.to_string())
            .replace("__MAX__", &max_events.to_string());
        let raw = match ps_capture(&script) {
            Ok(r) => r,
            Err(e) => {
                out.push_str(&format!("- [{log}] 查询失败：{e}\n",));
                continue;
            }
        };
        let raw = raw.trim().trim_start_matches('\u{feff}');
        let v: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                out.push_str(&format!("- [{log}] 解析失败：{e}\n"));
                continue;
            }
        };
        let err = v_str(&v, "Err");
        if !v.get("Success").and_then(Value::as_bool).unwrap_or(false) || !err.is_empty() {
            if err.to_ascii_lowercase().contains("unauthorized")
                || err.contains("拒绝访问")
                || err.contains("需要管理员")
            {
                out.push_str(&format!(
                    "- [{log}] 需要管理员权限，无法读取（登录失败审计不可用）\n"
                ));
            } else if err.to_ascii_lowercase().contains("log does not exist")
                || err.to_ascii_lowercase().contains("does not exist")
            {
                out.push_str(&format!("- [{log}] 日志不存在（本系统未注册该日志）\n"));
            } else {
                let err_msg = if err.is_empty() {
                    "未知错误".to_string()
                } else {
                    err.chars().take(200).collect()
                };
                out.push_str(&format!("- [{log}] 无法读取：{err_msg}\n"));
            }
            continue;
        }
        any_success = true;
        let events = v_arr(&v, "Events");
        if events.is_empty() {
            out.push_str(&format!("- {log} 日志最近 {hours} 小时无错误/警告\n"));
            continue;
        }
        for e in events {
            let ts = v_str(e, "TimeCreated");
            let id = v_str(e, "Id");
            let level = v_str(e, "Level");
            let provider = v_str(e, "ProviderName");
            let msg = v_str(e, "Message");
            let msg = trunc_msg(&msg, 280);
            let provider_part = if provider.is_empty() {
                String::new()
            } else {
                format!(" · {provider}")
            };
            let level_disp = level_cn(&level);
            out.push_str(&format!(
                "- [{log}] {ts} · {level_disp}（{level}） · 事件 {id}{provider_part}\n  - 消息：{msg}\n"
            ));
        }
    }
    if !any_success {
        out.push_str("\n所有日志均未读取成功（多为权限受限，请以管理员身份运行以启用安全审计）");
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_event_logs(
    _logs: Option<String>,
    _hours: u32,
    _max_events: usize,
) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}
