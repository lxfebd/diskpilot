//! 启动项管理：枚举注册表 Run 键 + 启动文件夹里的自启动项，
//! 支持启用/禁用（值/文件名加 .disabled 备份，可恢复）/删除（进回收站）。
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

/// 枚举启动项（只读，无确认要求）。
/// registry 部分用 PowerShell 读注册表（值名+值内容保真），文件夹部分直接读目录。
#[tauri::command]
pub(crate) async fn list_startup_items() -> Result<Vec<StartupItem>, String> {
    tokio::task::spawn_blocking(|| list_startup_items_blocking())
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
            // 用 PowerShell 读写注册表，值内容保真（避免 reg.exe 本地化输出歧义）。
            let script = if enable {
                format!(
                    "$k='{key}'; \
                     $d=(Get-ItemProperty -Path $k -Name '{disabled_name}' -ErrorAction SilentlyContinue).'{disabled_name}'; \
                     if ($null -eq $d) {{ throw '备份值不存在：{disabled_name}' }}; \
                     Set-ItemProperty -Path $k -Name '{value_name}' -Value $d -Type String; \
                     Remove-ItemProperty -Path $k -Name '{disabled_name}' -ErrorAction SilentlyContinue; \
                     'ok'"
                )
            } else {
                format!(
                    "$k='{key}'; \
                     $d=(Get-ItemProperty -Path $k -Name '{value_name}' -ErrorAction SilentlyContinue).'{value_name}'; \
                     if ($null -eq $d) {{ throw '值不存在：{value_name}' }}; \
                     Set-ItemProperty -Path $k -Name '{disabled_name}' -Value $d -Type String; \
                     Remove-ItemProperty -Path $k -Name '{value_name}' -ErrorAction SilentlyContinue; \
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
            std::fs::rename(&path, &new_path).map_err(|e| {
                format!(
                    "{}{}",
                    if enable { "恢复" } else { "禁用" },
                    format!("启动项失败：{e}")
                )
            })?;
            Ok(())
        }
        other => Err(format!("未知的启动项来源：{other}")),
    }
}

/// 删除一个启动项（文件夹项进回收站；注册表项直接删值）。
/// 需要 L2 权限 + 用户确认。
#[tauri::command]
pub(crate) async fn remove_startup_item(
    state: tauri::State<'_, crate::AppState>,
    id: String,
    confirmed: Option<bool>,
) -> Result<(), String> {
    if confirmed != Some(true) {
        return Err("删除启动项需要用户明确确认（confirmed=true）。请先在确认面板中确认。".into());
    }
    if !state.perm_grants.lock().unwrap().contains("startup.manage") {
        return Err(
            "拒绝执行：未开启「管理启动项」权限（startup.manage）。请先在权限中心开启。".into(),
        );
    }
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || remove_startup_item_blocking(&id2))
        .await
        .map_err(|e| e.to_string())?
}

fn remove_startup_item_blocking(id: &str) -> Result<(), String> {
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
            // 进回收站（可恢复）。trash 误报/失败时退化为直接删除（用户已确认）。
            let recycled = diskpilot_executor::recycle_path(&path);
            match recycled {
                Ok(()) => Ok(()),
                Err(_) => {
                    if path.exists() {
                        std::fs::remove_file(&path)
                            .or_else(|_| std::fs::remove_dir(&path))
                            .map_err(|e| format!("删除启动项失败：{e}"))?;
                    }
                    Ok(())
                }
            }
        }
        "registry_hkcu" | "registry_hklm" => {
            let key = if item.location == "registry_hkcu" {
                HKCU_RUN
            } else {
                HKLM_RUN
            };
            let script = format!(
                "$k='{key}'; \
                 Remove-ItemProperty -Path $k -Name '{}' -ErrorAction Stop; \
                 'ok'",
                item.name
            );
            ps_capture(&script, Duration::from_secs(15))?;
            Ok(())
        }
        other => Err(format!("未知的启动项来源：{other}")),
    }
}
