//! 3) 登录事件 + PowerShell 脚本块日志域（从 sec.rs 拆分，2026-10-12）。

use crate::ps::ps_capture_timeout;
#[cfg(windows)]
use crate::tools::sec::sec_common::{trunc_msg, v_arr};
use crate::tools::v_str;

#[cfg(windows)]
use serde_json::Value;

#[cfg(not(windows))]
use super::NOT_WINDOWS;

// ── 3. 登录事件 + PowerShell 脚本块日志 ────────────────────────────────

/// Security 日志 4624/4625（登录成功/失败）+ PowerShell Operational 4104（脚本块日志）。
/// 两段各带独立 try/catch，静默降级；两者均不可读时给说明性建议。
///
/// 2026-09-16 修复：Get-WinEvent 在无权限查询 Security 时可能阻塞分钟级，
/// 导致整体工具超时（冒烟发现无 id=2 响应且 60s 后进程存活）。改为：
/// 按日志名先锚定「最近事件」——历史日志非空即视为可读，规避冷路径长阻塞；
/// 仍不可读时走既有降级文案。加 -ErrorAction SilentlyContinue 双保险。
const LOGIN_PS_TMPL: &str = r#"
$n = __N__
$secOk = $false
$secErr = ''
$secEvents = @()
try {
  $any = @(Get-WinEvent -ListLog Security -ErrorAction SilentlyContinue)
  if ($any -and $any[0].RecordCount -gt 0 -and $any[0].IsEnabled) {
    $secEvents = @(Get-WinEvent -LogName Security -FilterXPath "*[System[(EventID=4624) or (EventID=4625)]]" -MaxEvents $n -ErrorAction Stop | ForEach-Object {
      [ordered]@{
        TimeCreated = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss')
        Id = $_.Id
        ProviderName = [string]$_.ProviderName
        Message = [string]$_.Message
      }
    })
    $secOk = $true
  } else {
    $secErr = 'Security 日志未启用或记录数为 0（需审计策略开启）'
  }
} catch {
  $secErr = $_.Exception.Message
}
$psOk = $false
$psErr = ''
$psEvents = @()
try {
  $psAny = @(Get-WinEvent -ListLog 'Microsoft-Windows-PowerShell/Operational' -ErrorAction SilentlyContinue)
  if ($psAny -and $psAny[0].RecordCount -gt 0 -and $psAny[0].IsEnabled) {
    $psEvents = @(Get-WinEvent -LogName 'Microsoft-Windows-PowerShell/Operational' -FilterXPath "*[System[EventID=4104]]" -MaxEvents $n -ErrorAction Stop | ForEach-Object {
      [ordered]@{
        TimeCreated = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss')
        Id = $_.Id
        Message = [string]$_.Message
      }
    })
    $psOk = $true
  } else {
    $psErr = 'PowerShell 脚本块日志未启用（需 ScriptBlockLogging GPO）'
  }
} catch {
  $psErr = $_.Exception.Message
}
[ordered]@{
  SecOk = $secOk
  SecErr = $secErr
  SecEvents = $secEvents
  PsOk = $psOk
  PsErr = $psErr
  PsEvents = $psEvents
} | ConvertTo-Json -Depth 5 -Compress
"#;

/// 登录审计：Security 日志 4624/4625 降级尝试 + PowerShell 脚本块日志 4104 尝试读取。
///
/// - Security 日志普通权限不可读 → 输出降级文案 `- [Security] 登录事件：需要管理员权限，无法读取`
/// - PowerShell 4104 Operational 日志普通权限**可能可读**，可读则输出脚本块摘要（截断 200 字符）
/// - 两者都不可读 → 附加 `安全审计受限` 说明与"以管理员身份运行"建议
#[cfg(windows)]
pub fn collect_login_events(max_events: usize) -> Result<String, String> {
    let n = if max_events < 1 {
        20
    } else {
        max_events.min(50)
    };
    let script = LOGIN_PS_TMPL.replace("__N__", &n.to_string());
    // 短查询窗口：安全日志无权限时 Get-WinEvent 仍可能阻塞，超时即降级返回
    //（不再让整个工具吞掉调用者的等待预算）
    let raw = match ps_capture_timeout(&script, std::time::Duration::from_secs(20)) {
        Ok(r) => r,
        Err(e) => return Ok(format!("登录事件查询超时或失败，已降级：{e}\n安全审计受限：Security 日志需管理员权限，建议以管理员身份运行。")),
    };
    let raw = raw.trim().trim_start_matches('\u{feff}');
    if raw.is_empty() {
        return Ok("登录事件不可用（PowerShell 无输出）".into());
    }
    let v: Value =
        serde_json::from_str(raw).map_err(|e| format!("解析登录事件查询结果失败：{e}"))?;

    let mut out = String::new();
    let sec_ok = v.get("SecOk").and_then(Value::as_bool).unwrap_or(false);
    let sec_err = v_str(&v, "SecErr");

    if sec_ok {
        let events = v_arr(&v, "SecEvents");
        if events.is_empty() {
            out.push_str("- [Security] 4624/4625 登录事件：最近无相关事件\n");
        } else {
            out.push_str(&format!(
                "- [Security] 登录事件（4624 成功 / 4625 失败，共 {} 条）：\n",
                events.len()
            ));
            for e in events {
                let ts = v_str(e, "TimeCreated");
                let id = v_str(e, "Id");
                let msg = trunc_msg(&v_str(e, "Message"), 200);
                out.push_str(&format!("- [Security] {ts} · 事件 {id}\n  - 消息：{msg}\n"));
            }
        }
    } else {
        let el = sec_err.to_ascii_lowercase();
        if el.contains("unauthorized") || sec_err.contains("拒绝访问") || sec_err.contains("管理员")
        {
            out.push_str("- [Security] 登录事件：需要管理员权限，无法读取（登录失败审计不可用）\n");
        } else if sec_err.is_empty() || el.contains("does not exist") {
            out.push_str(
                "- [Security] 登录事件：日志不可用（本系统未开启安全审计或未注册该日志）\n",
            );
        } else {
            out.push_str(&format!(
                "- [Security] 登录事件读取失败：{}\n",
                sec_err.chars().take(200).collect::<String>()
            ));
        }
    }

    let ps_ok = v.get("PsOk").and_then(Value::as_bool).unwrap_or(false);
    if ps_ok {
        let events = v_arr(&v, "PsEvents");
        if events.is_empty() {
            out.push_str(
                "- [PowerShell] 脚本块日志 4104：最近无相关事件（或系统未启用脚本块日志，需 `Set-ItemProperty HKLM:\\SOFTWARE\\Microsoft\\PowerShell\\$PSVersion\\ScriptBlockLogging -Name EnableScriptBlockLogging -Value 1` 后重启才产生）\n",
            );
        } else {
            out.push_str(&format!(
                "- [PowerShell] 脚本块日志 4104（{events_len} 条，普通权限可读，脚本攻击链高价值）：\n",
                events_len = events.len()
            ));
            for e in events {
                let ts = v_str(e, "TimeCreated");
                let id = v_str(e, "Id");
                let msg = trunc_msg(&v_str(e, "Message"), 200);
                out.push_str(&format!(
                    "- [PowerShell] {ts} · 事件 {id}\n  - 脚本块：{msg}\n"
                ));
            }
        }
    } else {
        let el = v_str(&v, "PsErr").to_ascii_lowercase();
        if el.contains("unauthorized") || el.contains("拒绝访问") || el.contains("管理员") {
            out.push_str(
                "- [PowerShell] 脚本块日志 4104：需要管理员权限（本环境未开放 Operational 读取）\n",
            );
        } else {
            out.push_str(
                "- [PowerShell] 脚本块日志 4104：不可用（未启用脚本块日志，或 Operational 通道被清除）\n",
            );
        }
    }

    if !sec_ok && !ps_ok {
        out.push_str(
            "\n安全审计受限：Security 日志与 PowerShell 脚本块日志均需管理员权限。\
             \n  建议：以管理员身份运行本工具；或配置 GPO 启用 PowerShell 脚本块日志后重开 Operational 通道。",
        );
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_login_events(_max_events: usize) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}
