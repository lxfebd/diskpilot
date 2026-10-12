//! 磁盘健康域（从 hw.rs 拆分，2026-10-12）。
//! HW_DISK_PS / hw_info command / hw_disk_cached / hw_disk_collect / parse_cdi_report。

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::AppHandle;

use super::hw_core::{hw_info_cached, ps_capture};
use crate::toolbelt::toolbelt_explicit_root;

/// 硬盘健康 TTL：SMART 通电时间/温度/磨损不会秒变，5 分钟缓存。
const HW_DISK_TTL: Duration = Duration::from_secs(300);
struct HwDiskSnapshot {
    at: std::time::Instant,
    value: serde_json::Value,
}
static HW_DISK_SNAPSHOT: OnceLock<Mutex<Option<HwDiskSnapshot>>> = OnceLock::new();
fn hw_disk_snapshot_lock() -> &'static Mutex<Option<HwDiskSnapshot>> {
    HW_DISK_SNAPSHOT.get_or_init(|| Mutex::new(None))
}

const HW_DISK_PS: &str = r#"$ErrorActionPreference='SilentlyContinue'
$disks=Get-PhysicalDisk
$out=[ordered]@{
  disks=@($disks | ForEach-Object { $rc=$_ | Get-StorageReliabilityCounter; [ordered]@{ friendly=$_.FriendlyName; media=$_.MediaType; bus=$_.BusType; size=$_.Size; health=[string]$_.HealthStatus; operational=($_.OperationalStatus -join ','); is_boot=$_.IsBoot; power_on_hours=$rc.PowerOnHours; temperature_c=$rc.Temperature; wear_pct=$rc.Wear; read_errors=$rc.ReadErrorsTotal; write_errors=$rc.WriteErrorsTotal } })
  reliability=@(@($disks | Get-StorageReliabilityCounter -ErrorAction SilentlyContinue).Count -gt 0)
}
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 读 CPU/GPU/内存/硬盘/主板/BIOS/系统/温度传感器，零确认。
/// 走 hardware_snapshot TTL 缓存（5 分钟），AI 多轮重复查询不再每次跑 PowerShell。
/// refresh=true 时无视 TTL 强制重跑（用户手动刷新）。
/// 读不到的字段一律 null，不编造。
#[tauri::command]
pub(crate) async fn hw_info(refresh: Option<bool>) -> Result<serde_json::Value, String> {
    let force = refresh.unwrap_or(false);
    tokio::task::spawn_blocking(move || hw_info_cached(force))
        .await
        .map_err(|e| e.to_string())?
}

/// 读硬盘健康（WMI + CrystalDiskInfo 补充），带 5 分钟缓存。
/// WMI 部分每次都便宜，但 CrystalDiskInfo /CopyExit 每次都要启动真实程序，
/// 切页/多轮查询时必须复用上次结果。force=true 时无视 TTL 强制重跑（手动刷新）。
pub(crate) fn hw_disk_cached(app: &AppHandle, force: bool) -> Result<serde_json::Value, String> {
    let now = std::time::Instant::now();
    if !force {
        if let Ok(guard) = hw_disk_snapshot_lock().lock() {
            if let Some(snap) = guard.as_ref() {
                if now.duration_since(snap.at) < HW_DISK_TTL {
                    return Ok(snap.value.clone());
                }
            }
        }
    }
    let explicit = toolbelt_explicit_root(app);
    let v = hw_disk_collect(explicit.as_deref())?;
    if let Ok(mut guard) = hw_disk_snapshot_lock().lock() {
        // at 用采集完成时刻，排队/执行耗时不计入 TTL（与 hw_info_cached 同口径）。
        *guard = Some(HwDiskSnapshot {
            at: std::time::Instant::now(),
            value: v.clone(),
        });
    }
    Ok(v)
}

fn hw_disk_collect(explicit: Option<&std::path::Path>) -> Result<serde_json::Value, String> {
    let raw = ps_capture(HW_DISK_PS, Duration::from_secs(30))?;
    let raw = raw.trim().trim_start_matches('\u{feff}');
    let mut v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("硬盘健康解析失败：{e}"))?;
    // 补一份 CrystalDiskInfo /CopyExit 产出的 DiskInfo.txt 原始字段，
    // WMI 拿不到 SMART 时（无管理员权限）可作为补充参考。
    let mut cdi: Vec<serde_json::Value> = Vec::new();
    if let Ok(root) = diskpilot_toolbelt::find_tools_root(explicit) {
        if let Some(t) = diskpilot_toolbelt::find("crystaldiskinfo") {
            if let Some(rel) = t.exe_rel.as_deref() {
                let exe = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
                if exe.is_file() {
                    let cmd = diskpilot_toolbelt::ResolvedCommand {
                        tool: t.name.clone(),
                        category: t.category.clone(),
                        program: exe.clone(),
                        args: vec!["/CopyExit".to_string()],
                        cwd: exe.parent().unwrap_or(&root).to_path_buf(),
                        risk: diskpilot_toolbelt::Risk::Low,
                        timeout_secs: 90,
                        usage: String::new(),
                    };
                    let _ = diskpilot_toolbelt::run(&cmd);
                    let txt_path = exe.parent().unwrap_or(&root).join("DiskInfo.txt");
                    if let Ok(txt) = std::fs::read_to_string(txt_path) {
                        cdi = parse_cdi_report(&txt);
                    }
                }
            }
        }
    }
    v["cdi_report"] = serde_json::json!(cdi);
    v["note"] = serde_json::json!("SMART 字段（通电时间/温度/磨损/读写错误）在非管理员权限下可能为 null；CrystalDiskInfo 补充字段同样可能不全。");
    Ok(v)
}

/// 硬盘健康：WMI Get-PhysicalDisk + Get-StorageReliabilityCounter
/// （SMART 通电时间/温度/磨损/读写错误），管理员权限不可用时字段降级为 null；
/// 另附 CrystalDiskInfo（若已装）生成的 DiskInfo.txt 原始字段，供补充参考。
/// 5 分钟 TTL 缓存：切页/多轮查询不再重跑 PowerShell 和 CrystalDiskInfo。
/// refresh=true 时无视 TTL 强制重跑（用户手动刷新）。
#[tauri::command]
pub(crate) async fn hw_disk_health(
    app: AppHandle,
    refresh: Option<bool>,
) -> Result<serde_json::Value, String> {
    let force = refresh.unwrap_or(false);
    tokio::task::spawn_blocking(move || hw_disk_cached(&app, force))
        .await
        .map_err(|e| e.to_string())?
}

/// 解析 CrystalDiskInfo /CopyExit 产出的 DiskInfo.txt，按磁盘抽取
/// 健康状态 / 通电时间 / 温度 / 磨损度。标签中英都认，认不出的字段留 null。
fn parse_cdi_report(txt: &str) -> Vec<serde_json::Value> {
    let lines: Vec<&str> = txt.lines().collect();
    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut cur: Option<serde_json::Map<String, serde_json::Value>> = None;
    let mut i = 0usize;
    while i < lines.len() {
        let t = lines[i].trim();
        let is_sep = t.len() >= 10 && t.chars().all(|c| c == '-' || c == '=');
        if is_sep {
            let nxt = lines.get(i + 1).map(|s| s.trim()).unwrap_or("");
            let looks_name = !nxt.is_empty()
                && !nxt.contains(':')
                && !nxt.contains('：')
                && !(nxt.len() >= 10 && nxt.chars().all(|c| c == '-' || c == '='));
            if looks_name {
                if let Some(m) = cur.take() {
                    out.push(serde_json::Value::Object(m));
                }
                let mut m = serde_json::Map::new();
                m.insert(
                    "device".to_string(),
                    serde_json::Value::String(nxt.to_string()),
                );
                cur = Some(m);
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(pos) = t.find(':').or_else(|| t.find('：')) {
            let key = t[..pos].trim().to_lowercase();
            let val = t[pos + 1..].trim().to_string();
            if let Some(m) = cur.as_mut() {
                let target = [
                    "健康状态",
                    "health status",
                    "通电时间",
                    "power on hours",
                    "温度",
                    "temperature",
                    "磨损",
                    "wear leveling count",
                    "剩余寿命",
                    "ssd 剩余寿命",
                ];
                if target.iter().any(|t| t == &key) {
                    m.insert(key, serde_json::Value::String(val.replace(',', "")));
                }
            }
        }
        i += 1;
    }
    if let Some(m) = cur.take() {
        out.push(serde_json::Value::Object(m));
    }
    out
}
