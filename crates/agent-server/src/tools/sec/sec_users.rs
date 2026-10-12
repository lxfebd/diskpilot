//! 用户账户域（从 sec.rs 拆分，2026-10-12）。
//! collect_user_accounts + json_arr_of_sec + sid_tail。

// ps_capture / v_str / sid_tail 仅供 cfg(windows) 的 collect_user_accounts 使用
//（Linux 仅返回降级文案），随 Windows 实现一起编译。
#[cfg(windows)]
use crate::ps::ps_capture;
#[cfg(windows)]
use crate::tools::v_str;

#[cfg(windows)]
use serde_json::Value;

#[cfg(not(windows))]
use super::NOT_WINDOWS;

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
pub(crate) fn sid_tail(sid: &str) -> String {
    let tail = sid.rsplit('-').next().unwrap_or("").to_string();
    if tail.is_empty() {
        format!("SID {sid}")
    } else {
        format!("RID {tail}")
    }
}
