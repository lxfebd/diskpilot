//! 清理预检纯逻辑（从 executor.rs 拆分，2026-10-12）。
//! running_required / preflight_recommended / running_process_names + 单测。

/// 纯函数（单测）：TOML 声明的进程名 ∩ 当前快照。大小写不敏感匹配（快照侧
/// 已小写），结果去重保序，显示保留 TOML 原始大小写。
pub(crate) fn running_required(wanted: &[String], snapshot: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for name in wanted {
        let key = name.trim().to_lowercase();
        if key.is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        if snapshot.contains(&key) {
            out.push(name.trim().to_string());
        }
    }
    out
}

/// 纯函数（单测）：推荐预选 scope id（`recommended_selected == Some(true)`）。
pub(crate) fn preflight_recommended(scaffold: &diskpilot_scaffold::Scaffold) -> Vec<String> {
    scaffold
        .scopes
        .iter()
        .filter(|sc| sc.recommended_selected == Some(true))
        .map(|sc| sc.id.clone())
        .collect()
}

/// 当前在跑的进程名快照（小写）。Windows 用 Toolhelp32（普通权限即可）；
/// 非 Windows 返回空集——preflight 退化为纯建议查询，不阻塞（R10 口径）。
#[cfg(windows)]
pub(crate) fn running_process_names() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return out;
    }
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    unsafe {
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                out.push(String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase());
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
pub(crate) fn running_process_names() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod preflight_tests {
    use super::{preflight_recommended, running_required};

    #[test]
    fn running_required_大小写不敏感且去重保序() {
        let snapshot = vec!["wechat.exe".to_string(), "explorer.exe".to_string()];
        let wanted = vec![
            "WeChat.exe".to_string(),
            "wechat.exe".to_string(),
            " QQ.exe ".to_string(),
            "".to_string(),
        ];
        let out = running_required(&wanted, &snapshot);
        assert_eq!(out, vec!["WeChat.exe".to_string()]);
    }

    #[test]
    fn running_required_快照为空_全不在跑() {
        let wanted = vec!["WeChat.exe".to_string()];
        assert!(running_required(&wanted, &[]).is_empty());
    }

    #[test]
    fn preflight_recommended_只取显式推荐项() {
        let scaffold: diskpilot_scaffold::Scaffold = toml::from_str(
            r#"
id = "t"
name = "T"
risk = "low"
disclaimer = "d"
detect = []
[[scope]]
id = "a"
label = "A"
glob = "**/a"
mode = "recycle"
recommended_selected = true
[[scope]]
id = "b"
label = "B"
glob = "**/b"
mode = "recycle"
required_stopped_processes = ["WeChat.exe"]
"#,
        )
        .unwrap();
        assert_eq!(preflight_recommended(&scaffold), vec!["a".to_string()]);
        // 反序列化侧：required_stopped_processes 也正确落到 scope 上
        assert_eq!(
            scaffold.scopes[1].required_stopped_processes.as_deref(),
            Some(&["WeChat.exe".to_string()][..])
        );
    }
}
