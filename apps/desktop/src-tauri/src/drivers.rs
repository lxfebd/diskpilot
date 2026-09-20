//! 驱动更新检查：枚举本机已装驱动（PS CIM，秒回，不联网），
//! 再对照 Windows Update 可选驱动更新（联网，慢，手动触发）给出可更新列表。
//! 只读检查，L2 权限仅用于「是否允许跑 Windows Update 查询」——
//! 查询本身不改系统，安装驱动需要用户自行去系统设置/厂商官网进行。

use std::time::Duration;

use crate::hw::ps_capture;

/// PowerShell ConvertTo-Json 在结果只有 1 项时折叠成对象而非数组，
/// 解析统一走这里兜底：数组原样返回，对象按单元素数组处理。
fn json_array_of(v: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(a) = v.as_array() {
        return a.clone();
    }
    if v.is_object() {
        return vec![v.clone()];
    }
    Vec::new()
}

/// 已装驱动条目（从 CIM 枚举）。
#[derive(serde::Serialize, Clone)]
pub(crate) struct InstalledDriver {
    pub(crate) name: String,
    pub(crate) provider: String,
    pub(crate) version: String,
    pub(crate) date: String,
    pub(crate) class: String,
}

/// Windows Update 可选更新条目（联网查询结果）。
#[derive(serde::Serialize, Clone)]
pub(crate) struct DriverUpdate {
    pub(crate) title: String,
    pub(crate) kb: String,
    pub(crate) driver_provider: String,
    pub(crate) driver_version: String,
    pub(crate) category: String,
    pub(crate) is_driver: bool,
}

const INSTALLED_PS: &str = r#"$ErrorActionPreference='SilentlyContinue'
$drivers=Get-CimInstance Win32_PnPSignedDriver | Where-Object { $_.DeviceName -and $_.DriverVersion } | Sort-Object DeviceName -Unique
$out=@($drivers | ForEach-Object {
  [ordered]@{
    name=[string]$_.DeviceName
    provider=[string]$_.DriverProviderName
    version=[string]$_.DriverVersion
    date=[string]$_.DriverDate
    class=[string]$_.DeviceClass
  }
})
ConvertTo-Json -InputObject $out -Depth 3 -Compress
"#;

/// 枚举已装驱动（只读、秒回、不联网）。
/// 结果按名称排序，DeviceName+DriverVersion 相同的去重。
#[tauri::command]
pub(crate) async fn list_installed_drivers() -> Result<Vec<InstalledDriver>, String> {
    tokio::task::spawn_blocking(|| {
        let raw = ps_capture(INSTALLED_PS, Duration::from_secs(30))?;
        let raw = raw.trim().trim_start_matches('\u{feff}');
        let v: serde_json::Value =
            serde_json::from_str(raw).map_err(|e| format!("驱动列表解析失败：{e}"))?;
        let arr = json_array_of(&v);
        let mut out = Vec::with_capacity(arr.len());
        for e in arr {
            out.push(InstalledDriver {
                name: e["name"].as_str().unwrap_or("").to_string(),
                provider: e["provider"].as_str().unwrap_or("").to_string(),
                version: e["version"].as_str().unwrap_or("").to_string(),
                date: e["date"].as_str().unwrap_or("").to_string(),
                class: e["class"].as_str().unwrap_or("").to_string(),
            });
        }
        out.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.class.cmp(&b.class))
        });
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 查询 Windows Update 可选更新（联网，慢）。
/// 需要 L2 权限（perm_grants 含 driver.check）——查询本身不改系统，
/// 但会向微软服务器发请求且耗时数秒到数十秒，做成显式授权动作。
/// 结果过滤：只保留驱动类（is_driver=true）与可选更新里名字像驱动的条目，
/// 其余（系统功能包/语言包）不列出。
#[tauri::command]
pub(crate) async fn check_driver_updates(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<DriverUpdate>, String> {
    if !state.perm_grants.lock().unwrap().contains("driver.check") {
        return Err(
            "拒绝执行：未开启「驱动更新检查」权限（driver.check）。请先在权限中心开启。".into(),
        );
    }
    tokio::task::spawn_blocking(|| check_driver_updates_blocking())
        .await
        .map_err(|e| e.to_string())?
}

fn check_driver_updates_blocking() -> Result<Vec<DriverUpdate>, String> {
    // 用系统原生 PowerShell Windows Update 模块（Win10+ 内置），
    // 不需要下载额外脚本。Type='Driver' 过滤驱动类更新（实测 Type=2 数字
    // 条件在部分系统上不识别，带引号的字符串条件是文档标准写法）。
    let script = r#"$ErrorActionPreference='SilentlyContinue'
$updates=@()
try {
  $session=New-Object -ComObject Microsoft.Update.Session
  $searcher=$session.CreateUpdateSearcher()
  $searcher.ServerSelection=2 # windowsUpdate
  $results=$searcher.Search('IsInstalled=0 and Type=''Driver''')
  $updates=@($results.Updates | ForEach-Object {
    $cat=@($_.Categories | ForEach-Object { $_.Name }) -join ','
    $kb=''
    if ($_.KBArticleIDs) { $kb=($_.KBArticleIDs -join ',') }
    [ordered]@{
      title=[string]$_.Title
      kb=[string]$kb
      driver_provider=''
      driver_version=''
      category=[string]$cat
      is_driver=$true
    }
  })
} catch {}
ConvertTo-Json -InputObject $updates -Depth 4 -Compress
"#;
    let raw = ps_capture(script, Duration::from_secs(90))?;
    let raw = raw.trim().trim_start_matches('\u{feff}');
    if raw.is_empty() {
        // 空输出：要么没有可选驱动更新，要么模块不支持（老系统）。
        return Ok(Vec::new());
    }
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("Windows Update 结果解析失败：{e}"))?;
    // ConvertTo-Json 单项时会折叠成对象而非数组，这里兜底。
    let arr = json_array_of(&v);
    let mut out = Vec::with_capacity(arr.len());
    for e in arr {
        out.push(DriverUpdate {
            title: e["title"].as_str().unwrap_or("").to_string(),
            kb: e["kb"].as_str().unwrap_or("").to_string(),
            driver_provider: e["driver_provider"].as_str().unwrap_or("").to_string(),
            driver_version: e["driver_version"].as_str().unwrap_or("").to_string(),
            category: e["category"].as_str().unwrap_or("").to_string(),
            is_driver: e["is_driver"].as_bool().unwrap_or(false),
        });
    }
    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(out)
}
