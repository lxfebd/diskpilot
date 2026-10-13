//! 启动项管理：枚举注册表 Run 键 + 启动文件夹里的自启动项，
//! 支持启用/禁用（值/文件名加 .disabled 备份，可恢复）/删除
//! （文件夹项进回收站，回收站没接住时先复制备份再删；注册表值没有回收站，
//! 删除前先备份成 .reg，可双击导入恢复）。
//! 写操作都是 L2 权限（权限中心开关 + 每次确认），后端纵深校验。

use std::time::Duration;

use crate::hw::ps_capture;

/// 单个启动项。
#[derive(serde::Serialize, Clone)]
pub(crate) struct StartupItem {
    /// 稳定 id：位置 + 序号拼接，前端开关的 key。
    pub(crate) id: String,
    /// 显示名（注册表值名 / 文件夹文件名）。
    pub(crate) name: String,
    /// 命令（注册表值内容）；文件夹项为空。
    pub(crate) command: String,
    /// 来源：registry_hkcu / registry_hklm / folder_user / folder_machine
    pub(crate) location: String,
    /// 当前是否启用。
    pub(crate) enabled: bool,
    /// 具体注册表路径（registry 项）或文件完整路径（folder 项）。
    pub(crate) target: String,
}

const HKCU_RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const HKLM_RUN: &str = r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run";
const FOLDER_REL: &str = r"\Microsoft\Windows\Start Menu\Programs\Startup";

// PowerShell 单引号字面量转义（单引号翻倍 `''`）统一在共享 crate
// `diskpilot-psutil`（原 desktop startup.rs 与 agent-server sys.rs 各一份，
// 防漂移合并）。启用/禁用/删除注册表启动项时，值名来自**注册表数据**
// （可被恶意软件写成 `x'; <任意 PS> #`），必须先转义再拼进 `'...'` 字面量，
// 否则启停操作本身成为代码执行入口（与 agent-server 同一条铁律）。
use diskpilot_psutil::escape_ps_single_quoted;

/// 枚举启动项（只读，无确认要求）。
/// registry 部分用 PowerShell 读注册表（值名+值内容保真），文件夹部分直接读目录。
#[tauri::command]
pub(crate) async fn list_startup_items() -> Result<Vec<StartupItem>, String> {
    tokio::task::spawn_blocking(list_startup_items_blocking)
        .await
        .map_err(|e| e.to_string())?
}

fn list_startup_items_blocking() -> Result<Vec<StartupItem>, String> {
    let mut out: Vec<StartupItem> = Vec::new();
    let mut seq: u32 = 0;

    // ── 注册表 Run 键（HKCU + HKLM）──
    // 一次 PS 拿两个键的值，输出 JSON，避免解析 reg.exe 的本地化标题行。
    let reg_ps = r#"
$ErrorActionPreference='SilentlyContinue'
$keys = @(
  @{ loc='registry_hkcu'; path='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' },
  @{ loc='registry_hklm'; path='HKLM:\Software\Microsoft\Windows\CurrentVersion\Run' }
)
$items = @()
foreach ($k in $keys) {
  if (Test-Path $k.path) {
    $p = Get-ItemProperty -Path $k.path
    $p.PSObject.Properties | Where-Object { $_.Name -notmatch '^(PSPath|PSParentPath|PSChildName|PSDrive|PSProvider)$' } | ForEach-Object {
      $items += [ordered]@{ loc=$k.loc; name=$_.Name; value=[string]$_.Value }
    }
  }
}
ConvertTo-Json -InputObject $items -Depth 3 -Compress
"#;
    if let Ok(raw) = ps_capture(reg_ps, Duration::from_secs(15)) {
        let raw = raw.trim().trim_start_matches('\u{feff}');
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
            // ConvertTo-Json 单项折叠成对象时按单元素数组处理。
            let arr = if let Some(a) = v.as_array() {
                a.clone()
            } else if v.is_object() {
                vec![v]
            } else {
                Vec::new()
            };
            for e in arr {
                let loc = e["loc"].as_str().unwrap_or("registry_hkcu").to_string();
                let name = e["name"].as_str().unwrap_or("").to_string();
                let value = e["value"].as_str().unwrap_or("").to_string();
                if name.is_empty() {
                    continue;
                }
                let key = if loc == "registry_hklm" {
                    HKLM_RUN
                } else {
                    HKCU_RUN
                };
                seq += 1;
                out.push(StartupItem {
                    id: format!("{loc}-{seq}"),
                    name: name.clone(),
                    command: value,
                    location: loc,
                    enabled: true,
                    target: format!("{key}\\{name}"),
                });
            }
        }
    }
    // ── 启动文件夹（用户 + 机器）──
    #[cfg(windows)]
    {
        use std::path::PathBuf;
        let mut dirs: Vec<(String, PathBuf)> = Vec::new();
        if let Ok(appdata) = std::env::var("APPDATA") {
            dirs.push((
                "folder_user".to_string(),
                PathBuf::from(appdata).join(FOLDER_REL.trim_start_matches('\\')),
            ));
        }
        if let Ok(pdata) = std::env::var("PROGRAMDATA") {
            dirs.push((
                "folder_machine".to_string(),
                PathBuf::from(pdata).join(FOLDER_REL.trim_start_matches('\\')),
            ));
        }
        for (loc, dir) in dirs {
            if !dir.is_dir() {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let path = e.path();
                let is_dir = path.is_dir();
                let raw_name = e.file_name().to_string_lossy().into_owned();
                let (display, enabled) = match raw_name.strip_suffix(".disabled") {
                    Some(stripped) => (stripped.to_string(), false),
                    None => (raw_name.clone(), true),
                };
                // 隐藏系统文件（desktop.ini 等）不是启动项，不展示
                let is_hidden = e
                    .metadata()
                    .map(|md| {
                        #[cfg(windows)]
                        {
                            use std::os::windows::fs::MetadataExt;
                            md.file_attributes() & 0x2 != 0 // FILE_ATTRIBUTE_HIDDEN
                        }
                        #[cfg(not(windows))]
                        {
                            false
                        }
                    })
                    .unwrap_or(false);
                if is_hidden || raw_name.starts_with('.') {
                    continue;
                }
                seq += 1;
                out.push(StartupItem {
                    id: format!("{loc}-{seq}"),
                    name: display,
                    command: if is_dir {
                        String::from("<文件夹>")
                    } else {
                        String::new()
                    },
                    location: loc.clone(),
                    enabled,
                    target: path.to_string_lossy().into_owned(),
                });
            }
        }
    }

    // 排序：启用在前，位置分组，名字排。
    out.sort_by(|a, b| {
        b.enabled
            .cmp(&a.enabled)
            .then(a.location.cmp(&b.location))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// 启停一个启动项。
/// enable=true：恢复（.disabled 改回原名 / 注册表值恢复）；
/// enable=false：禁用（文件夹文件改名 .disabled / 注册表值改名 .disabled）。
/// 需要 L2 权限（perm_grants 含 startup.manage）+ 用户确认。
#[tauri::command]
pub(crate) async fn set_startup_item(
    state: tauri::State<'_, crate::AppState>,
    id: String,
    enable: bool,
    confirmed: Option<bool>,
) -> Result<(), String> {
    if confirmed != Some(true) {
        return Err("修改启动项需要用户明确确认（confirmed=true）。请先在确认面板中确认。".into());
    }
    if !state.perm_grants.lock().unwrap().contains("startup.manage") {
        return Err(
            "拒绝执行：未开启「管理启动项」权限（startup.manage）。请先在权限中心开启。".into(),
        );
    }
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || set_startup_item_blocking(&id2, enable))
        .await
        .map_err(|e| e.to_string())?
}

fn set_startup_item_blocking(id: &str, enable: bool) -> Result<(), String> {
    // 重新枚举找到目标项（id 携带位置信息，直接重查比传参更不易被篡改）。
    let items = list_startup_items_blocking()?;
    let item = items
        .iter()
        .find(|i| i.id == id)
        .ok_or_else(|| format!("找不到启动项「{id}」"))?;

    match item.location.as_str() {
        "registry_hkcu" | "registry_hklm" => {
            // 注册表项：禁用 = 把值改名加 .disabled 备份；启用 = 从 .disabled 恢复。
            let key = if item.location == "registry_hkcu" {
                HKCU_RUN
            } else {
                HKLM_RUN
            };
            let value_name = &item.name;
            let disabled_name = format!("{value_name}.disabled");
            // 注册表值名是外部数据（恶意软件可伪造引号构造）——转义后拼入。
            let en = escape_ps_single_quoted(value_name);
            let dis = escape_ps_single_quoted(&disabled_name);
            // 用 PowerShell 读写注册表，值内容保真（避免 reg.exe 本地化输出歧义）。
            let script = if enable {
                format!(
                    "$k='{key}'; \
                     $d=(Get-ItemProperty -Path $k -Name '{dis}' -ErrorAction SilentlyContinue).'{dis}'; \
                     if ($null -eq $d) {{ throw '备份值不存在：{dis}' }}; \
                     Set-ItemProperty -Path $k -Name '{en}' -Value $d -Type String; \
                     Remove-ItemProperty -Path $k -Name '{dis}' -ErrorAction SilentlyContinue; \
                     'ok'"
                )
            } else {
                format!(
                    "$k='{key}'; \
                     $d=(Get-ItemProperty -Path $k -Name '{en}' -ErrorAction SilentlyContinue).'{en}'; \
                     if ($null -eq $d) {{ throw '值不存在：{en}' }}; \
                     Set-ItemProperty -Path $k -Name '{dis}' -Value $d -Type String; \
                     Remove-ItemProperty -Path $k -Name '{en}' -ErrorAction SilentlyContinue; \
                     'ok'"
                )
            };
            ps_capture(&script, Duration::from_secs(15))?;
            Ok(())
        }
        "folder_user" | "folder_machine" => {
            // 文件夹项：禁用 = 文件名加 .disabled 后缀（保留原文件，可恢复）。
            let path = std::path::PathBuf::from(&item.target);
            if !path.exists() {
                return Err(format!("启动项文件不存在：{}", path.display()));
            }
            let new_path = if enable {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match name.strip_suffix(".disabled") {
                    Some(stripped) => path.with_file_name(stripped),
                    None => return Err("该项未处于禁用状态".into()),
                }
            } else {
                let mut os = path.clone().into_os_string();
                os.push(".disabled");
                std::path::PathBuf::from(os)
            };
            if new_path.exists() {
                return Err(format!("目标路径已存在：{}", new_path.display()));
            }
            std::fs::rename(&path, &new_path)
                .map_err(|e| format!("{}启动项失败：{e}", if enable { "恢复" } else { "禁用" }))?;
            Ok(())
        }
        other => Err(format!("未知的启动项来源：{other}")),
    }
}

/// 备份文件名里的非法字符换成 `_`：值名来自注册表数据，可以是任意文本。
fn sanitize_file_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        out.push(match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        });
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        "_".to_string()
    } else {
        trimmed.to_string()
    }
}

/// 把注册表 Run 值写成可双击导入的 .reg 文本（纯函数，可测）。
///
/// 注册表值没有回收站：删掉就没了。所以删除前先落一份 .reg，
/// 用户双击即可恢复原启动项，不必依赖本应用。
fn reg_backup_text(location: &str, name: &str, command: &str) -> String {
    let hive = if location == "registry_hklm" {
        r"HKEY_LOCAL_MACHINE\Software\Microsoft\Windows\CurrentVersion\Run"
    } else {
        r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run"
    };
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "Windows Registry Editor Version 5.00\r\n\r\n\
         ; DiskPilot 删除启动项前的备份：双击导入即可恢复这个启动项。\r\n\
         ; 按 REG_SZ 写回；原值若是 REG_EXPAND_SZ（含 %变量%），导入后不会展开。\r\n\
         [{hive}]\r\n\
         \"{}\"=\"{}\"\r\n",
        esc(name),
        esc(command)
    )
}

/// 备份文件的字节内容：UTF-16LE + BOM（reg.exe 认的「Unicode .reg」）。
///
/// 写成 UTF-8 无 BOM 会被 reg.exe 按系统 ANSI 代码页解，中文值名导入后就乱了——
/// 备份恢复不了等于没备份。
fn reg_backup_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + text.len() * 2);
    out.extend_from_slice(&[0xFF, 0xFE]);
    for u in text.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

/// 备份文件名用的时间戳（秒）：同一项删多次不会互相覆盖。
fn backup_stamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 删除一个启动项。
/// 文件夹项进系统回收站（可在回收站还原）；注册表值没有回收站，
/// 先把值备份成 .reg（写失败就不删），返回备份文件路径（文件夹项返回空）。
/// 需要 L2 权限 + 用户确认。
#[tauri::command]
pub(crate) async fn remove_startup_item(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    id: String,
    confirmed: Option<bool>,
) -> Result<String, String> {
    if confirmed != Some(true) {
        return Err("删除启动项需要用户明确确认（confirmed=true）。请先在确认面板中确认。".into());
    }
    if !state.perm_grants.lock().unwrap().contains("startup.manage") {
        return Err(
            "拒绝执行：未开启「管理启动项」权限（startup.manage）。请先在权限中心开启。".into(),
        );
    }
    use tauri::Manager;
    // 备份目录拿不到就不删：注册表值删了没有第二条退路。
    let backup_dir = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("startup-backups"))
        .ok_or_else(|| "拿不到应用数据目录，无法备份：未删除任何启动项。".to_string())?;
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || remove_startup_item_blocking(&id2, &backup_dir))
        .await
        .map_err(|e| e.to_string())?
}

fn remove_startup_item_blocking(id: &str, backup_dir: &std::path::Path) -> Result<String, String> {
    let items = list_startup_items_blocking()?;
    let item = items
        .iter()
        .find(|i| i.id == id)
        .ok_or_else(|| format!("找不到启动项「{id}」"))?;

    match item.location.as_str() {
        "folder_user" | "folder_machine" => {
            let path = std::path::PathBuf::from(&item.target);
            if !path.exists() {
                return Err(format!("启动项文件不存在：{}", path.display()));
            }
            // 进回收站（可恢复）。回收站没接住时不再「直接删了事」：
            // 先把文件复制到备份目录，复制成功才删——不留没有退路的删除。
            let recycled = diskpilot_executor::recycle_path(&path);
            match recycled {
                Ok(()) => Ok(String::new()),
                Err(e) => {
                    if path.is_dir() {
                        return Err(format!(
                            "回收站没接住这个目录项（{e}），未删除：请手动把它移出启动文件夹。"
                        ));
                    }
                    std::fs::create_dir_all(backup_dir)
                        .map_err(|e2| format!("创建备份目录失败，未删除：{e2}"))?;
                    let name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "startup-item".to_string());
                    let file = backup_dir.join(format!(
                        "{}-{}-{name}",
                        sanitize_file_name(&item.name),
                        backup_stamp()
                    ));
                    std::fs::copy(&path, &file)
                        .map_err(|e2| format!("备份启动项文件失败，未删除：{e2}"))?;
                    std::fs::remove_file(&path).map_err(|e2| {
                        format!("删除启动项失败（原件已备份到 {}）：{e2}", file.display())
                    })?;
                    Ok(file.display().to_string())
                }
            }
        }
        "registry_hkcu" | "registry_hklm" => {
            let key = if item.location == "registry_hkcu" {
                HKCU_RUN
            } else {
                HKLM_RUN
            };
            // 先备份再删：备份写不下去就停，绝不留下「删了又恢复不了」的状态。
            std::fs::create_dir_all(backup_dir)
                .map_err(|e| format!("创建备份目录失败，未删除：{e}"))?;
            let stamp = backup_stamp();
            let file = backup_dir.join(format!("{}-{stamp}.reg", sanitize_file_name(&item.name)));
            std::fs::write(
                &file,
                reg_backup_bytes(&reg_backup_text(&item.location, &item.name, &item.command)),
            )
            .map_err(|e| format!("备份启动项失败，未删除：{e}"))?;
            // 值名是外部数据——转义后拼入（防单引号逃逸注入）。
            let script = format!(
                "$k='{key}'; \
                 Remove-ItemProperty -Path $k -Name '{}' -ErrorAction Stop; \
                 'ok'",
                escape_ps_single_quoted(&item.name)
            );
            match ps_capture(&script, Duration::from_secs(15)) {
                Ok(_) => Ok(file.display().to_string()),
                Err(e) => Err(format!(
                    "删除失败（已备份到 {}，注册表未改动）：{e}",
                    file.display()
                )),
            }
        }
        other => Err(format!("未知的启动项来源：{other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{reg_backup_text, sanitize_file_name};

    #[test]
    fn reg_backup_is_importable_and_escapes_paths() {
        let text = reg_backup_text("registry_hkcu", "MyApp", r"C:\Program Files\X\a.exe");
        assert!(text.starts_with("Windows Registry Editor Version 5.00"));
        assert!(text.contains(r"[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run]"));
        // 注册表 .reg 里反斜杠要翻倍，引号要转义。
        assert!(text.contains(r#""MyApp"="C:\\Program Files\\X\\a.exe""#));
        // HKLM 项写的是 HKLM 视图，不会错写到当前用户。
        let machine = reg_backup_text("registry_hklm", "svc", r"C:\a.exe");
        assert!(
            machine.contains(r"[HKEY_LOCAL_MACHINE\Software\Microsoft\Windows\CurrentVersion\Run]")
        );
        assert!(!machine.contains("HKEY_CURRENT_USER"));
    }

    #[test]
    fn backup_bytes_are_unicode_reg_not_utf8() {
        // reg.exe 认 UTF-16LE+BOM 的 .reg；UTF-8 无 BOM 会被按 ANSI 代码页解，中文值名会乱。
        let bytes =
            super::reg_backup_bytes(&reg_backup_text("registry_hkcu", "中文启动项", r"C:\a.exe"));
        assert_eq!(&bytes[..2], &[0xFF, 0xFE], "必须有 UTF-16LE BOM");
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .filter(|b| b.len() == 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        let decoded = String::from_utf16(&units).expect("应是合法 UTF-16LE");
        assert!(decoded.contains("中文启动项"), "中文值名要能原样读回");
        assert!(decoded.starts_with("Windows Registry Editor Version 5.00"));
    }

    #[test]
    fn backup_file_name_never_carries_path_separators() {
        // 值名可被恶意软件写成任意文本：不能借它逃出备份目录。
        assert_eq!(sanitize_file_name(r"..\..\x"), ".._.._x");
        assert_eq!(sanitize_file_name("a:b|c?d*e"), "a_b_c_d_e");
        assert_eq!(sanitize_file_name("   "), "_");
        assert_eq!(sanitize_file_name("正常名字"), "正常名字");
    }
}
