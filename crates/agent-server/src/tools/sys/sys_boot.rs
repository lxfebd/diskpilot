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

#[cfg(windows)]
use super::sys_services::wide;

#[cfg(not(windows))]
use super::NOT_WINDOWS;

// ── 3. 开机自启项枚举 ────────────────────────────────────────────────────

#[cfg(windows)]
const SCHEDULED_TASK_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$out=@(Get-ScheduledTask | Where-Object { $_.State -ne 'Disabled' } | Select-Object TaskName,TaskPath,State | Select-Object -First 100)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 手写展开 `%VAR%`——`Win32_System_Environment` feature 未开，无法调 `ExpandEnvironmentStringsW`。
/// 只展开 4 个常用变量；其他 `%VAR%` 原样透传。
#[cfg(windows)]
fn expand_env_simple(s: &str) -> String {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let local_appdata = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let programfiles = std::env::var("ProgramFiles").unwrap_or_default();
    let programfiles_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();
    let systemroot = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());

    // 替换顺序：长变量名先换，避免 `%AppData%` 被 `%App` 误吃（虽然这里没 `%App`，但保险）。
    let subs: [(&str, &str); 6] = [
        ("%LocalAppData%", local_appdata.as_str()),
        ("%AppData%", appdata.as_str()),
        ("%ProgramFiles(x86)%", programfiles_x86.as_str()),
        ("%ProgramFiles%", programfiles.as_str()),
        ("%SystemRoot%", systemroot.as_str()),
        ("%USERPROFILE%", home.as_str()),
    ];
    let mut out = s.to_string();
    for (from, to) in subs {
        out = out.replace(from, to);
    }
    out
}

/// 解析命令行第一个 token 作为 exe 路径（去引号、简单 `%VAR%` 展开）。
///
/// 例：`"C:\...\svc.exe" --flag` → `C:\...\svc.exe`
///     `%SystemRoot%\System32\foo.exe` → `C:\Windows\System32\foo.exe`
#[cfg(windows)]
fn parse_exe_token(cmd: &str) -> String {
    let t = cmd.trim();
    if t.is_empty() {
        return String::new();
    }
    let first = t.split_whitespace().next().unwrap_or("").to_string();
    // 展开环境变量
    let expanded = expand_env_simple(&first);
    // 去首尾引号
    let stripped = expanded.trim_matches('"');
    stripped.trim().to_string()
}

#[cfg(windows)]
struct RunEntry {
    source: String, // 前缀标签，如 "[HKCU\Run]"
    name: String,   // Run 值名
    command: String,
    exe_exists: bool, // 指向的文件是否存在（false = 僵尸启动项）
}

/// 枚举一个 Run 注册表键下的所有子值（`RegEnumValueW` 递归）。
#[cfg(windows)]
fn enum_run_key(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
    flags: windows_sys::Win32::System::Registry::REG_SAM_FLAGS,
    prefix: &str,
    out: &mut Vec<RunEntry>,
) {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };
    let mut key: HKEY = std::ptr::null_mut();
    let rc = unsafe { RegOpenKeyExW(root, wide(subkey).as_ptr(), 0, KEY_READ | flags, &mut key) };
    if rc != 0 {
        return;
    }
    let mut idx = 0u32;
    loop {
        let mut name_buf = [0u16; 512];
        let mut name_len = name_buf.len() as u32;
        let mut type_buf = 0u32;
        let mut data_buf = [0u16; 2048];
        let mut data_len = data_buf.len() as u32;
        let r = unsafe {
            RegEnumValueW(
                key,
                idx,
                name_buf.as_mut_ptr(),
                &mut name_len,
                std::ptr::null(),
                &mut type_buf,
                data_buf.as_mut_ptr() as *mut u8,
                &mut data_len,
            )
        };
        if r != 0 {
            break;
        }
        idx += 1;
        // RegEnumValueW 的 name_len / data_len 不含 NUL，手动处理。
        let nname = (name_len as usize).min(name_buf.len());
        let name = String::from_utf16_lossy(&name_buf[..nname]);
        if name.is_empty() || name == "(Default)" {
            continue;
        }
        if type_buf != REG_SZ && type_buf != REG_EXPAND_SZ {
            continue;
        }
        let n_data = if data_len >= 2 {
            (data_len as usize / 2 - 1).min(data_buf.len())
        } else {
            0
        };
        let command_raw = String::from_utf16_lossy(&data_buf[..n_data]);
        if command_raw.trim().is_empty() {
            continue;
        }
        // REG_EXPAND_SZ 含 % 变量展开
        let expanded = if type_buf == REG_EXPAND_SZ {
            expand_env_simple(&command_raw)
        } else {
            command_raw.clone()
        };
        let exe = parse_exe_token(&expanded);
        let exists = !exe.is_empty() && std::path::Path::new(&exe).exists();
        out.push(RunEntry {
            source: prefix.to_string(),
            name: name.clone(),
            command: expanded,
            exe_exists: exists,
        });
    }
    unsafe { RegCloseKey(key) };
}

/// 枚举一个 Startup 文件夹（用户/机器），只列文件、过滤 `desktop.ini` 和隐藏。
#[cfg(windows)]
fn enum_startup_folder(dir: &std::path::Path, out: &mut Vec<RunEntry>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(x) => x,
        Err(_) => return,
    };
    for ent in rd.flatten() {
        let file_name = ent.file_name();
        let name_str = file_name.to_string_lossy();
        // 过滤隐藏 + desktop.ini + 目录
        if name_str.starts_with('.') {
            continue;
        }
        if name_str.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        let ft = match ent.file_type() {
            Ok(x) => x,
            Err(_) => continue,
        };
        if ft.is_dir() {
            continue;
        }
        let full = ent.path().display().to_string();
        out.push(RunEntry {
            source: "[启动文件夹]".to_string(),
            name: name_str.to_string(),
            command: full.clone(),
            exe_exists: true, // read_dir 能列出来就一定存在
        });
    }
}

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
