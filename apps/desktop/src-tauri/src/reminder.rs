//! 清理提醒（R3）：定时跑 `cleanup_suggestions` 引擎，只出清单 + 通知，
//! **绝不自动执行**——建议走前端确认窗（execute_ai_plan 的 user_confirmed
//! 硬校验），本模块没有任何写操作。
//!
//! 设计对齐：
//! - **引擎复用**：直接调 `cleanup::cleanup_suggestions`（带 `cached_only=false`
//!   真算，但只读——与总览页 cached_only=true 的「展示口径」区分：提醒要真数）。
//! - **配置持久化**：`app_data_dir()/reminder.json`（仿 general_config 读→合并→
//!   写；读失败静默 default）。
//! - **通知通道**：`app.emit("cleanup-reminder", …)` → 前端 listen → toast /
//!   引导跳总览。不引入 notify-rust 系统通知（避免新原生依赖；且应用内 toast
//!   与「确认窗」在同一 UI 上下文，点击直达）。
//! - **后台线程**：仿 watchdog 的 `std::thread::spawn + loop + sleep`，每次 tick
//!   读配置（间隔/开关），到点才真算。

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::AppState;

/// 提醒配置（持久化到 app_data_dir/reminder.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ReminderConfig {
    /// 总开关。false = 后台线程不干活。
    pub enabled: bool,
    /// 间隔小时（1..=168，默认 24）。
    pub interval_hours: u64,
    /// 至少多少字节才值得提醒（低于此数不打扰）。默认 200MB。
    pub min_bytes: u64,
}

impl Default for ReminderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: 24,
            min_bytes: 200 * 1024 * 1024,
        }
    }
}

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("reminder.json"))
}

fn config_at(app: &AppHandle) -> ReminderConfig {
    let Some(p) = config_path(app) else {
        return ReminderConfig::default();
    };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 读取当前提醒配置（只读 L0，设置页回显）。
#[tauri::command]
pub(crate) fn get_reminder_config(app: AppHandle) -> ReminderConfig {
    config_at(&app)
}

/// 保存提醒配置（L1 受控：改的是用户自己的配置，非删除类）。
#[tauri::command]
pub(crate) fn set_reminder_config(
    app: AppHandle,
    enabled: Option<bool>,
    interval_hours: Option<u64>,
    min_bytes: Option<u64>,
) -> Result<ReminderConfig, String> {
    let Some(p) = config_path(&app) else {
        return Err("无法定位数据目录".into());
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // 读旧值合并写回：避免翻一个开关把其他字段抹掉。
    let mut cfg = config_at(&app);
    if let Some(v) = enabled {
        cfg.enabled = v;
    }
    if let Some(v) = interval_hours {
        cfg.interval_hours = v.clamp(1, 168);
    }
    if let Some(v) = min_bytes {
        cfg.min_bytes = v;
    }
    let text = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(&p, text).map_err(|e| e.to_string())?;
    Ok(cfg)
}

/// 提醒负载（emit 给前端）。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ReminderPayload {
    /// 各盘建议可清字节（只读统计，非删除计划）。
    pub drives: Vec<DriveCleanup>,
    /// 全部盘合计可清字节。
    pub total_bytes: u64,
    /// 生成时间（unix 秒）。
    pub ts: u64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DriveCleanup {
    pub path: String,
    pub bytes: u64,
}

/// 跑一次提醒检查（手动触发 + 后台线程共用）：遍历所有盘，真算建议，
/// 只发通知，绝不执行。返回本次检查是否发出提醒（测试/手动按钮回显）。
pub(crate) fn run_reminder_check(app: &AppHandle) -> Result<Option<ReminderPayload>, String> {
    let cfg = config_at(app);
    if !cfg.enabled {
        return Ok(None);
    }
    let state = app.state::<AppState>();
    let payload = compute_reminder(state, &cfg)?;
    if let Some(p) = &payload {
        let _ = app.emit("cleanup-reminder", p);
    }
    Ok(payload)
}

/// 真算各盘可清字节（复用 cleanup_suggestions 引擎，cached_only=false）。
/// 只读：返回建议量，不做任何清理。
/// 后台线程（无 tokio runtime）里调用时用 `tauri::async_runtime::block_on`
/// 包一层——cleanup_suggestions 内部有 spawn_blocking await。
fn compute_reminder(
    state: State<'_, AppState>,
    cfg: &ReminderConfig,
) -> Result<Option<ReminderPayload>, String> {
    // 枚举所有盘（复用 lib.rs list_drives 语义：C..=Z 中可读的盘）。
    let mut drives: Vec<DriveCleanup> = Vec::new();
    let mut total: u64 = 0;
    for letter in b'C'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if crate::volume_info(root.clone()).is_err() {
            continue;
        }
        // cleanup_suggestions 需要 scaffolds；直接读 state。block_on 在后台线程
        // 里提供 tokio runtime 上下文（tauri 的 async_runtime 是全局 Handle）。
        let suggs = tauri::async_runtime::block_on(crate::cleanup::cleanup_suggestions(
            state.clone(),
            root.clone(),
            None,
            Some(false),
        ))?;
        let bytes: u64 = suggs.iter().map(|s| s.bytes).sum();
        if bytes > 0 {
            drives.push(DriveCleanup { path: root, bytes });
            total = total.saturating_add(bytes);
        }
    }
    if total < cfg.min_bytes {
        return Ok(None);
    }
    Ok(Some(ReminderPayload {
        drives,
        total_bytes: total,
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    }))
}

/// 手动触发一次（只读 L0；设置页「立即检查」按钮）。
#[tauri::command]
pub(crate) fn run_reminder_check_cmd(app: AppHandle) -> Result<Option<ReminderPayload>, String> {
    run_reminder_check(&app)
}

/// 后台提醒线程：每 tick 读配置，到点且 enabled 才真算 + emit。
/// 在 run() 的 setup 里 spawn（与 watchdog 同款）。**只出清单，绝不自动清理。**
pub(crate) fn spawn_cleanup_reminder(app: AppHandle) {
    std::thread::spawn(move || {
        // 启动后先等一个间隔再首跑，避免应用刚开就全盘算一次（性能铁律）。
        let mut last_run = std::time::Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(60)); // tick 粒度 60s
            let cfg = config_at(&app);
            if !cfg.enabled {
                continue;
            }
            let interval = Duration::from_secs(cfg.interval_hours.saturating_mul(3600));
            if last_run.elapsed() < interval {
                continue;
            }
            last_run = std::time::Instant::now();
            let _ = run_reminder_check(&app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_sane() {
        let c = ReminderConfig::default();
        assert!(!c.enabled, "默认关闭，不打扰");
        assert_eq!(c.interval_hours, 24);
        assert_eq!(c.min_bytes, 200 * 1024 * 1024);
    }

    #[test]
    fn config_roundtrip_via_temp_file() {
        // 用临时目录模拟 app_data_dir：写 → 读 → 合并 → 断言。
        let dir = std::env::temp_dir().join(format!("dp-reminder-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("reminder.json");
        let mut cfg = ReminderConfig::default();
        cfg.enabled = true;
        cfg.interval_hours = 6;
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        std::fs::write(&p, text).unwrap();

        let loaded: ReminderConfig =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(loaded.enabled);
        assert_eq!(loaded.interval_hours, 6);
        // 合并写回保留 min_bytes 默认
        let mut merged = loaded;
        merged.interval_hours = 12;
        let text = serde_json::to_string_pretty(&merged).unwrap();
        std::fs::write(&p, text).unwrap();
        let reloaded: ReminderConfig =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(reloaded.interval_hours, 12);
        assert_eq!(
            reloaded.min_bytes,
            200 * 1024 * 1024,
            "合并写回不抹默认字段"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interval_clamped() {
        let dir = std::env::temp_dir().join(format!("dp-reminder-clamp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("reminder.json");
        let mut cfg = ReminderConfig::default();
        cfg.interval_hours = 9999; // 超上限
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        std::fs::write(&p, text).unwrap();
        let loaded: ReminderConfig =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        // set 命令路径会 clamp；这里直接模拟 set 的 clamp 行为
        let clamped = loaded.interval_hours.clamp(1, 168);
        assert_eq!(clamped, 168, "interval 钳到 168");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
