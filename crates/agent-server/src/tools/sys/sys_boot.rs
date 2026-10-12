//! 开机自启项枚举域（从 sys.rs 拆分，2026-10-12）。
//! collect_boot_items / manage_scheduled_task：Run 键 + 启动文件夹 + 计划任务。

#[cfg(windows)]
use serde_json::Value;

#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use crate::ps::{json_array_of, ps_capture, ps_capture_timeout};

#[cfg(windows)]
use crate::tools::v_str;

#[cfg(windows)]
pub(crate) use diskpilot_psutil::escape_ps_single_quoted;

#[cfg(not(windows))]
use super::NOT_WINDOWS;

// ── 3. 开机自启项枚举 ────────────────────────────────────────────────────

#[cfg(windows)]
const SCHEDULED_TASK_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-ScheduledTask | Where-Object { $_.State -ne 'Disabled' } | Select-Object TaskName,TaskPath,State | Select-Object -First 100)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

mod registry;
pub(crate) use registry::{enum_run_key, enum_startup_folder, RunEntry};

/// 开机自启项枚举：5 个 Run 键 + 2 个 Startup 文件夹 + 可选计划任务。**只读**，绝不修改/禁用/删除。
#[cfg(windows)]
pub fn collect_boot_items(include_scheduled: bool) -> Result<String, String> {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };

    let mut entries: Vec<RunEntry> = Vec::new();

    // ① HKCU\Run（当前用户）
    enum_run_key(
        HKEY_CURRENT_USER,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        0,
        "[HKCU\\Run]",
        &mut entries,
    );

    // ② HKLM\Run（机器级 64 位）
    enum_run_key(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        KEY_WOW64_64KEY,
        "[HKLM\\Run]",
        &mut entries,
    );

    // ③ HKCU Policies\Explorer\Run（策略强制，任务管理器禁不掉——高价值）
    enum_run_key(
        HKEY_CURRENT_USER,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer\Run",
        0,
        "[策略\\Run]",
        &mut entries,
    );

    // ④ HKLM Policies\Explorer\Run（机器级策略强制）
    enum_run_key(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer\Run",
        KEY_WOW64_64KEY,
        "[策略\\Run]",
        &mut entries,
    );

    // ⑤ HKLM WOW6432Node\Run（32 位视图）
    enum_run_key(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
        KEY_WOW64_32KEY,
        "[WOW64\\Run]",
        &mut entries,
    );

    // ⑥ 启动文件夹：用户 + 机器
    // 用户 Startup（`%USERPROFILE%\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup`）
    let user_profile = std::env::var("USERPROFILE").unwrap_or_default();
    if !user_profile.is_empty() {
        let user_startup = std::path::PathBuf::from(&user_profile)
            .join("AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup");
        enum_startup_folder(&user_startup, &mut entries);
    }
    // 机器级 Startup（Win10+ 在 ProgramData 下）
    let machine_startup =
        std::path::PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\StartUp");
    enum_startup_folder(&machine_startup, &mut entries);

    let registry_count = entries
        .iter()
        .filter(|e| !e.source.starts_with("[启动"))
        .count();
    let folder_count = entries
        .iter()
        .filter(|e| e.source.starts_with("[启动"))
        .count();

    // ⑦ 计划任务（默认跳过，199 条非常慢）
    let mut scheduled_count = 0usize;
    let mut scheduled_lines: Vec<String> = Vec::new();
    if include_scheduled {
        match ps_capture_timeout(SCHEDULED_TASK_PS, Duration::from_secs(60)) {
            Ok(raw) => {
                let arr_str = json_array_of(raw.trim().trim_start_matches('\u{feff}'));
                if let Ok(arr) = serde_json::from_str::<Vec<Value>>(&arr_str) {
                    for v in &arr {
                        let tn = v_str(v, "TaskName");
                        let tp = v_str(v, "TaskPath");
                        let st = v_str(v, "State");
                        if tn.is_empty() {
                            continue;
                        }
                        let state_zh = match st.as_str() {
                            "Ready" => "就绪",
                            "Running" => "运行中",
                            "Paused" => "已暂停",
                            "Disabled" => "已禁用",
                            _ => {
                                if st.is_empty() {
                                    "未知"
                                } else {
                                    "状态未知"
                                }
                            }
                        };
                        let full = if tp.is_empty() {
                            format!("\\{tn}")
                        } else {
                            format!("{tp}{tn}")
                        };
                        scheduled_lines.push(format!("- [计划任务] {full} · {state_zh}"));
                        scheduled_count += 1;
                    }
                }
            }
            Err(_) => {
                scheduled_lines
                    .push("- [计划任务] （查询失败：Get-ScheduledTask 超时或无返回）".to_string());
            }
        }
    }

    // 输出
    let total = entries.len() + scheduled_count;
    if total == 0 && !include_scheduled {
        return Ok("未发现开机自启项（注册表 5 键 + 用户/机器 Startup 文件夹均空）".into());
    }

    let missing = entries.iter().filter(|e| !e.exe_exists).count();
    let mut s = String::new();
    s.push_str("开机自启项（只读枚举，绝不修改/禁用/删除）：\n");
    for e in &entries {
        let exists_tag = if e.exe_exists {
            "· 存在".to_string()
        } else {
            "· （指向不存在文件——僵尸启动项）".to_string()
        };
        s.push_str(&format!(
            "- {} {} → {} {}\n",
            e.source, e.name, e.command, exists_tag
        ));
    }
    for line in &scheduled_lines {
        s.push_str(line);
        s.push('\n');
    }
    s.push_str(&format!(
        "共 {} 项（注册表 {} / 启动文件夹 {} / 计划任务 {}），其中 {} 项指向不存在的文件",
        total, registry_count, folder_count, scheduled_count, missing
    ));
    if !include_scheduled {
        s.push_str("（如需查看计划任务请传 include_scheduled=true）");
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_boot_items(_include_scheduled: bool) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}

/// 计划任务管理（写操作，仅可逆动作）。**由主项目 agent.rs 桥接层做 confirmed
/// 确认门**，本函数只负责「能否安全操作 + 执行」。守卫（违反即拒绝）：
/// - action 只允许 enable（启用）/ disable（禁用）/ query（只读查询）——
///   **没有 delete / run**：任务删除不可逆，绝不开放给 AI；
/// - 系统任务黑名单：任务路径以 `\Microsoft\` / `\Windows\` / `\System32\` 开头
///   的任务（Windows 内置维护任务）一律拒绝修改；
/// - 任务名不能为空、不能是通配 `\*`；目标任务不存在 → 拒绝（不建新任务）。
///
/// enable / disable 可逆（禁用后可用 enable 恢复）；dry_run=true 只出计划不执行。
#[cfg(windows)]
pub fn manage_scheduled_task(name: &str, action: &str, dry_run: bool) -> Result<String, String> {
    let name = name.trim();
    let action = action.trim().to_ascii_lowercase();
    if name.is_empty() || name == "\\*" {
        return Err("任务名不能为空（也不接受通配 \\*，一次只操作一个任务）".into());
    }
    match action.as_str() {
        "enable" | "disable" | "query" => {}
        _ => {
            return Err(format!(
                "action 只支持 enable / disable / query（不开放 delete/run），收到：{action}"
            ))
        }
    }
    // 系统任务黑名单：Windows 内置任务路径前缀。这些任务的启停是系统维护的
    // 一部分，禁用会导致系统异常（更新/备份/碎片整理失效），一定不是用户想让
    // AI 干的；query 仍放行（只读）。
    let lower = name.to_ascii_lowercase().replace('\\', "/");
    let system_prefixes: &[&str] = &["/microsoft/", "/windows/", "/system32/"];
    if action != "query" && system_prefixes.iter().any(|p| lower.starts_with(p)) {
        return Err(format!(
            "拒绝：任务「{name}」是系统内置任务（路径在 \\Microsoft\\ / \\Windows\\ / \\System32\\ 下），不可由 AI 修改（安全守卫）"
        ));
    }
    // 任务必须存在（query 也先探测，给出明确不存在提示）。
    // 存在性/权限交给 PowerShell 实际命令判定，不在此预判：非系统前缀任务
    // 的 enable/disable 由 PowerShell 自己报「找不到任务」；query 也由命令返回
    // 状态或降级说明（系统任务在有管理员时才能读，无管理员时给提示）。
    // —— 这样才不会把「任务不存在」和「无权限读系统任务」两种失败混在一起。
    if dry_run {
        let zh_action = match action.as_str() {
            "enable" => "启用",
            "disable" => "禁用",
            _ => "查询",
        };
        return Ok(format!(
            "（dry-run 计划）将对计划任务「{name}」执行 {zh_action}。任务非系统内置（\\ 前缀已拦），操作可逆。"
        ));
    }
    match action.as_str() {
        "enable" => {
            let out = ps_capture(&format!(
                "$ErrorActionPreference='Stop'; $n='{escaped}'; Enable-ScheduledTask -TaskName $n -ErrorAction Stop; 'enabled'",
                escaped = escape_ps_single_quoted(name)
            ));
            match out {
                Ok(o) if o.trim() == "enabled" => Ok(format!("已启用计划任务「{name}」")),
                Ok(o) => Err(format!("启用任务未确认成功，输出：{}", o.trim())),
                Err(e) => Err(format!(
                    "启用计划任务「{name}」失败：{e}（任务可能不存在或需管理员权限）"
                )),
            }
        }
        "disable" => {
            let out = ps_capture(&format!(
                "$ErrorActionPreference='Stop'; $n='{escaped}'; Disable-ScheduledTask -TaskName $n -ErrorAction Stop; 'disabled'",
                escaped = escape_ps_single_quoted(name)
            ));
            match out {
                Ok(o) if o.trim() == "disabled" => {
                    Ok(format!("已禁用计划任务「{name}」（可随时用 enable 恢复）"))
                }
                Ok(o) => Err(format!("禁用任务未确认成功，输出：{}", o.trim())),
                Err(e) => Err(format!(
                    "禁用计划任务「{name}」失败：{e}（任务可能不存在或需管理员权限）"
                )),
            }
        }
        // query：只读，返回任务当前状态；失败降级说明（系统任务可能无权读）
        _ => {
            match ps_capture(&format!(
                "$ErrorActionPreference='SilentlyContinue'; \
                 $n='{escaped}'; $t=Get-ScheduledTask -TaskName $n; \
                 ($t.State.ToString());",
                escaped = escape_ps_single_quoted(name)
            )) {
                Ok(o) => Ok(format!("计划任务「{name}」当前状态：{}", o.trim())),
                Err(e) => Err(format!(
                    "查询计划任务「{name}」失败：{e}（可能是系统任务需管理员权限，或任务不存在）"
                )),
            }
        }
    }
}

#[cfg(not(windows))]
pub fn manage_scheduled_task(_name: &str, _action: &str, _dry_run: bool) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}
