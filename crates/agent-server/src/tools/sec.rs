//! 安全信息只读采集：Windows 事件日志、防火墙规则、登录/PowerShell 审计。
//!
//! 纯工具文件，不接路由（由 `tools/mod.rs` 统一接 MCP tool 薄壳）。
//! **全部只读**：不改事件日志、不改防火墙规则、不修改注册表/策略、不删用户文件；
//! 只输出 Windows 事件日志消息文本与防火墙规则的元数据（DisplayName/Port/Protocol/Address）。
//! 全部走 PowerShell（`Get-WinEvent` / `Get-NetFirewallRule` / `Get-NetFirewallPortFilter`
//! / `Get-NetFirewallAddressFilter`）；`ps_capture` 已注入 UTF-8 编码（根治中文乱码，
//! GBK 兜底仍在），本文件负责解析与中文格式化。
//!
//! ## 本文件导出函数清单（供 `tools/mod.rs` 对接）
//! - `pub fn collect_event_logs(logs: Option<String>, hours: u32, max_events: usize) -> Result<String, String>`
//!   —— 事件日志采集（默认 System,Application；Level 2/3 = 错误+警告）。
//! - `pub fn collect_firewall_rules(direction: Option<String>, action: Option<String>, top_n: usize) -> Result<String, String>`
//!   —— 防火墙规则采集（默认 Inbound+Allow 高危组合；联查 PortFilter + AddressFilter）。
//! - `pub fn collect_login_events(max_events: usize) -> Result<String, String>`
//!   —— 登录审计：Security 日志 4624/4625 降级 + PowerShell 脚本块日志 4104 尝试读取。
//!
//! ## 已真机验证的坑（照抄不重新发明）
//! - **Security 日志普通权限不可读**（真机验证）：`Get-WinEvent -LogName Security`
//!   抛 `UnauthorizedAccessException`，`Get-EventLog -List` 也拒。**必须降级**为
//!   `- [Security] 需要管理员权限，无法读取`，绝不整体失败。System / Application 可读。
//! - **`Get-WinEvent.Message` 极长**（单条可占满 AI 上下文）：一律截断 280 字符（登录
//!   脚本块 200 字符），并在截断后附加 `（共 N 字）`；用 `str::chars().take(n)` 走
//!   UTF-8 字符边界（中文/emoji 不会切成半个字）。
//! - **防火墙规则枚举只有 PowerShell**：`Get-NetFirewallRule` 有 889 条（真机量级），
//!   `netsh advfirewall` 输出中文乱码**禁用**。规则只有元数据，端口/地址要联查
//!   `Get-NetFirewallPortFilter`（本地端口/协议）与 `Get-NetFirewallAddressFilter`
//!   （远程地址）。默认只列 Inbound+Allow（高危入站放行），`top_n` 限流。
//! - **无 windows-sys 原生事件日志 API**：`EvtQuery` 未在 windows-sys 0.59 导出，
//!   一律走 PS。
//! - **`ConvertTo-Json` 单元素数组折叠成对象**：本文件所有 PS 模板输出都是 `[ordered]@{ ... }`
//!   对象（非数组），Rust 端直接 `serde_json::from_str::<Value>` 解析，无需 `json_array_of`
//!   兜底。子数组 `Events` / `Rules` / `SecEvents` / `PsEvents` 在 PS 侧用 `@(...)` 强制为数组
//!   后 JSON 序列化不会折叠。
//! - **`Get-WinEvent -FilterHashtable` 权限陷阱**（真机验证）：`-FilterHashtable @{LogName='Security'}`
//!   不抛 `UnauthorizedAccessException`，只返回空（server-side 过滤绕过权限检查）；
//!   必须先用 `-LogName <name> -MaxEvents 1` 做权限探测（会抛 UnauthorizedAccess），
//!   探测通过再跑 FilterHashtable。
//! - **参数注入防护**：日志名走白名单正则（`[A-Za-z._-]`），方向/动作走白名单枚举
//!   （Inbound/Outbound/Any、Allow/Block/Any），其它一律拒绝。`Get-WinEvent -LogName`
//!   本身不接受文件路径（只会命中系统注册的日志），但输入仍做白名单。
//! - **rustc 1.98 拒绝 `{:.1f}`**：本文件全部浮点走 `{:.*}`（当前无浮点格式，保留约束）。
//!
//! ## 隐私红线
//! - 只输出事件日志消息文本（系统日志）与防火墙规则元数据；**绝不读用户个人文件/Temp**。
//! - 不改事件日志配置（`Clear-EventLog` / `Wevtutil` / `Set-LogProperties` 一律禁用）。
//! - 不改防火墙规则（`Set-NetFirewallRule` / `New-NetFirewallRule` / `Enable-NetFirewall`
//!   一律禁用）。

#[cfg(windows)]
use serde_json::Value;

use crate::ps::{ps_capture, ps_capture_timeout};

#[cfg(not(windows))]
#[allow(dead_code)] // 跨平台降级文案：Windows 构建不引用
const NOT_WINDOWS: &str = "当前平台不是 Windows，安全信息不可用";

// ── 通用小工具 ─────────────────────────────────────────────────────────

#[cfg(windows)]
fn v_str(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

#[cfg(windows)]
fn v_arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(Value::as_array).map_or(&[], |a| a)
}

/// 消息截断到 `limit` 个字符（UTF-8 字符边界安全），超长追加 `（共 N 字）`。
///
/// `str::chars().take(n)` 只在字符边界截断，中文/emoji 不会被切半个字。
#[cfg(windows)]
fn trunc_msg(msg: &str, limit: usize) -> String {
    if msg.chars().count() <= limit {
        return msg.to_string();
    }
    let kept: String = msg.chars().take(limit).collect();
    let total = msg.chars().count();
    format!("{kept}…（共{total}字）")
}

/// 白名单校验日志名：只允许 ASCII 字母、点、下划线、连字符、斜杠，长度 ≤ 64。
///
/// 允许列表覆盖常见命名：`System` / `Application` / `Security` / `Setup` /
/// `Application/Server` / `Microsoft-Windows-PowerShell/Operational` 等。
///
/// 拒绝空格与中文/其它 Unicode 字符。`Get-WinEvent -LogName` 只接受已注册日志通道名，
/// 白名单防止通过空格 / 中文 / 通配符绕过。
#[cfg(windows)]
fn validate_log_name(name: &str) -> Result<String, String> {
    let t = name.trim();
    if t.is_empty() || t.len() > 64 {
        return Err(format!("日志名「{name}」非法（需 1..=64 字符）"));
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphabetic() || c == '.' || c == '_' || c == '-' || c == '/')
    {
        return Err(format!(
            "日志名「{name}」含非法字符（仅允许 ASCII 字母、`.`、`_`、`-`、`/`）"
        ));
    }
    Ok(t.to_string())
}

/// 白名单校验 PowerShell 参数值（Direction / Action 等）。
#[cfg(windows)]
fn validate_ps_value(input: &str, allowed: &[&str], what: &str) -> Result<String, String> {
    let t = input.trim().to_string();
    if allowed.iter().any(|a| a.eq_ignore_ascii_case(&t)) {
        Ok(t)
    } else {
        Err(format!(
            "非法{what}「{input}」（允许：{}）",
            allowed.join(", ")
        ))
    }
}

/// 事件 Level 数值 → 中文可读名（`Level` 数值语义固定：2=Error / 3=Warning）。
#[cfg(windows)]
fn level_cn(level: &str) -> &'static str {
    match level {
        "2" => "错误",
        "3" => "警告",
        _ => "未知",
    }
}

// ── 1. 事件日志（System / Application 等普通权限可读）──────────────────

/// 事件日志查询脚本模板：先用 `-LogName` 做权限探测（Security 会抛 UnauthorizedAccess），
/// 探测通过再跑 `-FilterHashtable` 过滤错误/警告。
///
/// **权限陷阱**：`-FilterHashtable @{LogName='Security'}` 不抛异常，只返回空
/// （server-side 过滤绕过权限检查）；所以必须先 `-LogName` 探测。
/// `$err` 收集探测或查询阶段的异常，由 Rust 侧降级。
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

// ── 2. 防火墙规则（元数据：DisplayName / Port / Protocol / Remote）──────

/// 防火墙规则查询脚本模板：联查 `Get-NetFirewallRule` +
/// `Get-NetFirewallPortFilter` + `Get-NetFirewallAddressFilter`，
/// 输出规则元数据（**绝不返回规则内部动作细节**，只输出只读元数据）。
const FIREWALL_PS_TMPL: &str = r#"
$dir = '__DIR__'
$act = '__ACTION__'
$n = __N__
$rules = @()
$err = ''
try {
  $rules = @(Get-NetFirewallRule -Direction $dir -Action $act -Enabled True -ErrorAction Stop | Sort-Object -Property LastModifiedTime -Descending | Select -First $n | ForEach-Object {
    $r = $_
    $p = $r | Get-NetFirewallPortFilter -ErrorAction SilentlyContinue
    $a = $r | Get-NetFirewallAddressFilter -ErrorAction SilentlyContinue
    [ordered]@{
      Name = [string]$r.Name
      DisplayName = [string]$r.DisplayName
      Direction = [string]$r.Direction
      Action = [string]$r.Action
      Enabled = [string]$r.Enabled
      Group = [string]$r.Group
      Profile = [string]$r.Profile
      LocalPort = if ($p) {
        $lp = $p.LocalPort
        if ($null -ne $lp) { [string]$lp } else { '' }
      } else { '' }
      RemotePort = if ($p) {
        $rp = $p.RemotePort
        if ($null -ne $rp) { [string]$rp } else { '' }
      } else { '' }
      Protocol = if ($p) { [string]$p.Protocol } else { '' }
      RemoteAddress = if ($a) { [string]$a.RemoteAddress } else { '' }
    }
  })
} catch {
  $err = $_.Exception.Message
}
[ordered]@{ Rules = $rules; Err = $err } | ConvertTo-Json -Depth 6 -Compress
"#;

/// 防火墙规则采集（**只读元数据**）：默认 Inbound+Allow 高危组合，`top_n` 限流（默认 20，上限 50）。
///
/// 参数走白名单校验：`direction ∈ {Inbound, Outbound, Any}`，`action ∈ {Allow, Block, Any}`。
/// `Get-NetFirewallRule` 需管理员权限（部分 Win10 版本），失败时降级为中文说明。
#[cfg(windows)]
pub fn collect_firewall_rules(
    direction: Option<String>,
    action: Option<String>,
    top_n: usize,
) -> Result<String, String> {
    let dir = match direction.as_deref().map(|s| s.trim().to_string()) {
        Some(d) if !d.is_empty() => validate_ps_value(&d, &["Inbound", "Outbound", "Any"], "方向")?,
        _ => "Inbound".to_string(),
    };
    let act = match action.as_deref().map(|s| s.trim().to_string()) {
        Some(a) if !a.is_empty() => validate_ps_value(&a, &["Allow", "Block", "Any"], "动作")?,
        _ => "Allow".to_string(),
    };
    let n = if top_n < 1 { 20 } else { top_n.min(50) };

    let script = FIREWALL_PS_TMPL
        .replace("__DIR__", &dir)
        .replace("__ACTION__", &act)
        .replace("__N__", &n.to_string());
    let raw = ps_capture(&script)?;
    let raw = raw.trim().trim_start_matches('\u{feff}');
    if raw.is_empty() {
        return Ok("防火墙规则不可用（PowerShell 无输出）".into());
    }
    let v: Value =
        serde_json::from_str(raw).map_err(|e| format!("解析防火墙规则查询结果失败：{e}"))?;
    let err = v_str(&v, "Err");
    if !err.is_empty() {
        let e = err.to_ascii_lowercase();
        if e.contains("access is denied") || err.contains("拒绝访问") || err.contains("管理员")
        {
            return Ok(
                "需管理员权限，无法枚举防火墙规则（请以管理员身份运行以启用防火墙审计）".into(),
            );
        }
        if e.contains("not recognized") || e.contains("does not exist") {
            return Ok(
                "防火墙模块不可用（Get-NetFirewallRule cmdlet 缺失，可能为精简系统镜像）".into(),
            );
        }
        return Ok(format!(
            "防火墙规则查询失败：{}",
            err.chars().take(200).collect::<String>()
        ));
    }
    let rules = v_arr(&v, "Rules");
    if rules.is_empty() {
        return Ok(format!(
            "无匹配规则（方向={dir}，动作={act}，已启用；系统共 {} 条防火墙规则需按需筛选）\n\
             （默认视图为 Inbound+Allow 高危入站放行；可传 direction/action 覆盖）",
            "未知（-Enabled True 过滤后为 0）"
        ));
    }
    let mut out = String::new();
    out.push_str(&format!(
        "防火墙规则（{dir}·{act}，已启用，共 {} 条）：\n",
        rules.len()
    ));
    for r in rules {
        let dir_disp = match v_str(r, "Direction").to_ascii_lowercase().as_str() {
            "inbound" => "入站",
            "outbound" => "出站",
            _ => "N/A",
        };
        let act_disp = match v_str(r, "Action").to_ascii_lowercase().as_str() {
            "allow" => "放行",
            "block" => "阻止",
            _ => "N/A",
        };
        let disp = v_str(r, "DisplayName");
        let nm = v_str(r, "Name");
        let disp_nm = if disp.is_empty() {
            if nm.is_empty() {
                "（无名称）"
            } else {
                nm.as_str()
            }
        } else {
            disp.as_str()
        };
        let port = v_str(r, "LocalPort");
        let remote_port = v_str(r, "RemotePort");
        let proto = v_str(r, "Protocol");
        let remote = v_str(r, "RemoteAddress");
        let group = v_str(r, "Group");
        let profile = v_str(r, "Profile");
        let mut head = format!("- {dir_disp}·{act_disp} · {disp_nm}");
        let mut meta: Vec<String> = Vec::new();
        if !port.is_empty() {
            meta.push(format!("端口 {port}"));
        }
        if !remote_port.is_empty() {
            meta.push(format!("远端端口 {remote_port}"));
        }
        if !proto.is_empty() {
            meta.push(format!("协议 {proto}"));
        }
        if !remote.is_empty() {
            meta.push(format!("远程地址 {remote}"));
        }
        if !group.is_empty() {
            meta.push(format!("组「{group}」"));
        }
        if !profile.is_empty() {
            meta.push(format!("配置文件 {profile}"));
        }
        if meta.is_empty() {
            head.push_str(" · （无端口/协议/地址元数据）");
        } else {
            head.push_str(" · ");
            head.push_str(&meta.join(" · "));
        }
        out.push_str(&head);
        out.push('\n');
    }
    out.push_str(&format!(
        "\n说明：默认仅列 Inbound+Allow 高危入站放行（可传 direction/action 覆盖）；\
         \n  结果按 LastModifiedTime 降序取前 {n} 条；仅展示规则元数据（DisplayName/Port/Protocol/Address），\
         \n  不展示规则内部动作细节；来自 Get-NetFirewallRule 联查 PortFilter/AddressFilter，需管理员权限。"
    ));
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_firewall_rules(
    _direction: Option<String>,
    _action: Option<String>,
    _top_n: usize,
) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

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

// ── 4. 用户账户 ────────────────────────────────────────────────────────

/// 用户账户枚举：Win32_UserAccount（本地 + 域账户，普通用户可见，排除系统账户）。
///
/// 隐私红线：只输出账户名 / 全名 / 是否禁用 / 是否管理员 / SID 尾段，
/// **绝不**输出密码哈希或 token 信息。
#[cfg(windows)]
pub fn collect_user_accounts() -> Result<String, String> {
    let script = r#"
$ErrorActionPreference='Stop'
Get-CimInstance -ClassName Win32_UserAccount | ForEach-Object {
  [ordered]@{
    Name = [string]$_.Name
    FullName = [string]$_.FullName
    Disabled = [bool]$_.Disabled
    LocalAccount = [bool]$_.LocalAccount
    SID = [string]$_.SID
    Lockout = [bool]$_.Lockout
  }
} | ConvertTo-Json -Compress
"#;
    let arr = match json_arr_of_sec(script, "用户账户") {
        Ok(a) => a,
        Err(e) if e.to_ascii_lowercase().contains("access denied") => {
            return Ok("用户账户枚举需要管理员权限（Win32_UserAccount 受限）。".into())
        }
        Err(e) => {
            // 兜底：CIM 查询整体失败时，尝试本地账户即可（无需管理员）
            let fallback = r#"
$ErrorActionPreference='SilentlyContinue'
Get-LocalUser -ErrorAction SilentlyContinue | ForEach-Object {
  [ordered]@{
    Name = [string]$_.Name
    FullName = [string]$_.FullName
    Disabled = -not [bool]$_.Enabled
    LocalAccount = $true
    SID = [string]$_.SID
    Lockout = $false
  }
} | ConvertTo-Json -Compress
"#;
            match json_arr_of_sec(fallback, "本地用户") {
                Ok(a) => a,
                Err(_) => return Err(e),
            }
        }
    };

    if arr.is_empty() {
        return Ok("未枚举到用户账户。".into());
    }
    // 排除常见内置/系统账户，避免噪音；保留 Guest（有价值）
    let skip: [&str; 4] = [
        "defaultuser0",
        "defaultuser",
        "wdagutilityaccount",
        "systemprofile",
    ];
    let mut out = format!("用户账户（{} 个）：\n", arr.len());
    for v in &arr {
        let name = v_str(v, "Name");
        if name.is_empty() || skip.contains(&name.to_ascii_lowercase().as_str()) {
            continue;
        }
        let full = v_str(v, "FullName");
        let local = v
            .get("LocalAccount")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let disabled = v.get("Disabled").and_then(Value::as_bool).unwrap_or(false);
        let lockout = v.get("Lockout").and_then(Value::as_bool).unwrap_or(false);
        let typ = if local { "本地" } else { "域" };
        let mut flag = vec![typ];
        if disabled {
            flag.push("禁用");
        }
        if lockout {
            flag.push("锁定");
        }
        out.push_str(&format!(
            "- {name}{} · {}，{}\n",
            if full.is_empty() {
                String::new()
            } else {
                format!("（{full}）")
            },
            flag.join(" · "),
            sid_tail(&v_str(v, "SID"))
        ));
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_user_accounts() -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

/// 跑输出「数组」的 PowerShell 脚本并解析成 `Vec<Value>`。
#[cfg(windows)]
fn json_arr_of_sec(script: &str, what: &str) -> Result<Vec<Value>, String> {
    let raw = ps_capture(script)?;
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let arr = crate::ps::json_array_of(trimmed);
    serde_json::from_str(&arr).map_err(|e| format!("解析{what}查询结果失败：{e}"))
}

/// SID 尾段（`S-1-5-21-xxxx-xxxx-xxxx-<rid>` → `<rid>`，账户唯一标识）。
#[cfg(windows)]
fn sid_tail(sid: &str) -> String {
    let tail = sid.rsplit('-').next().unwrap_or("").to_string();
    if tail.is_empty() {
        format!("SID {sid}")
    } else {
        format!("RID {tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)] // sid_tail 是 Windows-only 辅助函数
    fn sid_tail_extracts_rid() {
        // S-1-5-21-1234567890-1234567890-1234567890-1001 → RID 1001
        assert_eq!(
            sid_tail("S-1-5-21-1234567890-1234567890-1234567890-1001"),
            "RID 1001"
        );
        // 短 SID → 尾段即最后的段
        assert_eq!(sid_tail("S-1-5"), "RID 5");
        // 空字符串 → 原样返回
        assert_eq!(sid_tail(""), "SID ");
    }

    #[test]
    fn defender_status_non_windows_degrade() {
        let s = collect_defender_status();
        assert!(s.is_ok());
    }

    #[test]
    fn user_accounts_non_windows_degrade() {
        let s = collect_user_accounts();
        assert!(s.is_ok());
    }
}
