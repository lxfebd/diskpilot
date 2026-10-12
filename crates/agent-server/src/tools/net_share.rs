//! SMB 共享清单 + 会话（Get-SmbShare 普通权限可用，会话降级）。
//!
//! from tools/net.rs 拆分（2026-10-12）。

// Value / json_of / v_str 仅供 cfg(windows) 的 collect_net_share 使用
//（Linux 仅返回降级文案），随 Windows 实现一起编译。
#[cfg(windows)]
use serde_json::Value;

#[cfg(windows)]
use crate::tools::net_util::json_of;
#[cfg(windows)]
use crate::tools::v_str;

// ── 4. 网络共享 ─────────────────────────────────────────────────────────

#[cfg(windows)]
const SHARE_PS: &str = r#"
# 用 @(...) 强制数组语义：否则单个共享会被 ConvertTo-Json 折叠成对象
$ErrorActionPreference='SilentlyContinue'
$shares=@(Get-SmbShare | Select-Object Name,Path,Description,IsSpecial,EnumerateOnly,ScopeName)
$sessions=$null
$sessionsErr=''
try{
    $sessions=@(Get-SmbSession -ErrorAction Stop | Select-Object ClientComputerName,ClientUserName,Dialect)
}catch{
    $sessionsErr=($_.Exception.Message)
}
[ordered]@{
    Shares=$shares
    HasSessions=if($null -ne $sessions){$true}else{$false}
    Sessions=if($null -ne $sessions){$sessions}else{@()}
    SessionsErr=$sessionsErr
} | ConvertTo-Json -Depth 6 -Compress
"#;

/// SMB 共享清单 + 会话。`Get-SmbShare` 普通权限可用；
/// 会话必拒（错误 5），`try/catch` 后降级为中文说明，不把 Err 抛上去。
#[cfg(windows)]
pub fn collect_net_share() -> Result<String, String> {
    let v = json_of(SHARE_PS, "共享文件夹")?;
    let mut out = String::new();

    let shares = v
        .get("Shares")
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    if shares.is_empty() {
        out.push_str("本机没有共享文件夹\n");
    } else {
        out.push_str(&format!("本机共享文件夹（{} 个）：\n", shares.len()));
        for s in shares {
            let name = v_str(s, "Name");
            let path = v_str(s, "Path");
            let desc = v_str(s, "Description");
            let is_special = matches!(s.get("IsSpecial"), Some(Value::Bool(true)));
            let scope = v_str(s, "ScopeName").to_ascii_uppercase();
            let tag = if is_special {
                "（特殊共享，系统保留名）"
            } else {
                "（普通共享）"
            };
            let scope_tag = match scope.as_str() {
                "LOCALHOST" => " · 仅本机访问",
                "ALL" => " · 局域网可见",
                _ => "",
            };
            let desc_part = if desc.is_empty() {
                String::new()
            } else {
                format!(" · 说明：{desc}")
            };
            out.push_str(&format!(
                "- {} → {} · {tag}{scope_tag}{desc_part}\n",
                name,
                if path.is_empty() {
                    "(无路径)"
                } else {
                    path.as_str()
                }
            ));
        }
    }

    let has_sessions = matches!(v.get("HasSessions"), Some(Value::Bool(true)));
    if has_sessions {
        let sessions = v
            .get("Sessions")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        out.push_str(&format!("SMB 会话（{} 个）：\n", sessions.len()));
        for s in sessions {
            out.push_str(&format!(
                "- {} · {} · {}\n",
                v_str(s, "ClientComputerName"),
                v_str(s, "ClientUserName"),
                v_str(s, "Dialect")
            ));
        }
    } else {
        out.push_str(
            "- 会话列表：需要管理员权限（Get-SmbSession 拒绝访问），无法获取当前有哪些远程主机连着这些共享",
        );
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_net_share() -> Result<String, String> {
    Ok("当前平台不是 Windows，网络信息不可用".into())
}
