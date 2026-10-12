//! 安全域通用小工具（从 sec.rs 拆分，2026-10-12）。
//! v_arr / trunc_msg / validate_log_name / validate_ps_value / level_cn。

#[cfg(windows)]
use serde_json::Value;

// ── 通用小工具 ─────────────────────────────────────────────────────────

#[cfg(windows)]
pub(crate) fn v_arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(Value::as_array).map_or(&[], |a| a)
}

/// 消息截断到 `limit` 个字符（UTF-8 字符边界安全），超长追加 `（共 N 字）`。
///
/// `str::chars().take(n)` 只在字符边界截断，中文/emoji 不会被切半个字。
#[cfg(windows)]
pub(crate) fn trunc_msg(msg: &str, limit: usize) -> String {
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
pub(crate) fn validate_log_name(name: &str) -> Result<String, String> {
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
pub(crate) fn validate_ps_value(
    input: &str,
    allowed: &[&str],
    what: &str,
) -> Result<String, String> {
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
pub(crate) fn level_cn(level: &str) -> &'static str {
    match level {
        "2" => "错误",
        "3" => "警告",
        _ => "未知",
    }
}
