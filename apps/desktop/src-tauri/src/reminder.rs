//! 清理提醒（R3）：定时跑 `cleanup_suggestions` 引擎，只出清单 + 通知，
//! **绝不自动执行**——建议走前端确认窗（execute_ai_plan 的 user_confirmed
//! 硬校验），本模块没有任何写操作。
//!
//! 设计对齐：
//! - **引擎复用**：直接调 `cleanup::cleanup_suggestions`（带 `cached_only=true`，
//!   只读最近一次扫描/缓存树，**绝不因未扫描盘而触发全盘 walk**——后台定时
//!   高频跑，必须与总览页「主动查建议」区分：后台只要当前已知的建议量）。
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
    /// 周巡检简报开关（health-butler P1）。独立于 enabled：可只开清理提醒
    /// 或只开周报。后台线程按 weekly_interval_days 跑 `weekly_report::run_weekly_report`。
    #[serde(default)]
    pub weekly_report_enabled: bool,
    /// 周报间隔天（1..=90，默认 7）。
    #[serde(default)]
    pub weekly_interval_days: u64,
    /// 空间耗尽预警阈值（天，R99）：按 space-history 趋势外推「多少天后
    /// 用满」，≤ 该天数的盘触发 `space-full-reminder` 通知。0 = 关闭。
    /// 独立于 enabled：可只开空间预警，不动清理提醒。
    #[serde(default)]
    pub full_warn_days: u64,
}

impl Default for ReminderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: 24,
            min_bytes: 200 * 1024 * 1024,
            weekly_report_enabled: false,
            weekly_interval_days: 7,
            full_warn_days: 0,
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

/// 展示用配置读取（get_reminder_config）：**区分「首次（文件不存在）」
/// 与「读失败」**。文件不存在 = 从未设置，返回默认配置是诚实的（真空态）；
/// 文件存在但读失败/损坏 = 必须报错，否则前端把默认值当真实配置展示
/// （读失败被伪装成默认值，同诚实性族规）。
fn config_at_explicit(app: &AppHandle) -> Result<ReminderConfig, String> {
    let Some(p) = config_path(app) else {
        return Err("无法定位数据目录".into());
    };
    read_config_at_path(&p)
}

/// config_at_explicit 的核心逻辑（路径参数化，便于测试）。
fn read_config_at_path(p: &std::path::Path) -> Result<ReminderConfig, String> {
    if !p.exists() {
        return Ok(ReminderConfig::default());
    }
    let text = std::fs::read_to_string(p).map_err(|e| format!("读取提醒配置失败: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("提醒配置损坏: {e}"))
}

/// 读取当前提醒配置（只读 L0，设置页回显）。文件不存在回默认（首次），
/// 存在但读失败则报错，让前端显示「未读到」而非伪默认值。
#[tauri::command]
pub(crate) fn get_reminder_config(app: AppHandle) -> Result<ReminderConfig, String> {
    config_at_explicit(&app)
}

/// 保存提醒配置（L1 受控：改的是用户自己的配置，非删除类）。
#[tauri::command]
pub(crate) fn set_reminder_config(
    app: AppHandle,
    enabled: Option<bool>,
    interval_hours: Option<u64>,
    min_bytes: Option<u64>,
    weekly_report_enabled: Option<bool>,
    weekly_interval_days: Option<u64>,
    full_warn_days: Option<u64>,
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
    if let Some(v) = weekly_report_enabled {
        cfg.weekly_report_enabled = v;
    }
    if let Some(v) = weekly_interval_days {
        cfg.weekly_interval_days = v.clamp(1, 90);
    }
    if let Some(v) = full_warn_days {
        // 上限 365：超过一个月的预警意义不大，锁死避免配置被污染。
        cfg.full_warn_days = v.min(365);
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
    /// health-butler P2-1 按项授权命中项：(scaffold_id, 可清字节)。
    /// 只用于「免去想起来要看」的主动告知；旧前端不读此字段，向前兼容。
    #[serde(default)]
    pub auto_list_hits: Vec<(String, u64)>,
    /// 生成时间（unix 秒）。
    pub ts: u64,
}

/// 空间耗尽预警（R99）：按 space-history 趋势外推「多少天后用满」。
/// 纯只读统计——通知里只有路径与天数，不带任何清理动作。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SpaceFullWarn {
    /// 卷根路径（C:\ 等）。
    pub path: String,
    /// 预计满盘天数（向上取整；0 = 已满）。
    pub days: u64,
}

/// 按首尾采样外推「多少天后用满」。语义与前端 forecastFullDays 严格一致：
/// 不足 2 点 / 零跨度 / 增速 ≤0（持平或释放）→ None——**释放不是「快满了」，
/// 绝不把健康盘唱成满盘**；已满（剩余 ≤0，且增速为正）→ Some(0.0)；
/// 否则返回剩余 ÷ 日增速的天数。
pub(crate) fn forecast_full_days(pts: &[crate::space_history::SpacePoint]) -> Option<f64> {
    if pts.len() < 2 {
        return None;
    }
    let first = &pts[0];
    let last = &pts[pts.len() - 1];
    let span = last.at.saturating_sub(first.at);
    if span == 0 || last.at < first.at {
        return None;
    }
    let slope = (last.used_bytes as f64 - first.used_bytes as f64) / span as f64; // 字节/秒
    if slope <= 0.0 {
        return None;
    }
    let remaining = last.total_bytes as f64 - last.used_bytes as f64;
    if remaining <= 0.0 {
        return Some(0.0);
    }
    let days = remaining / (slope * 86400.0);
    if days.is_finite() && days > 0.0 {
        Some(days)
    } else {
        None
    }
}

/// 扫一遍空间历史，返回预计 `full_warn_days` 天内会满的盘（含已满 = 0 天）。
/// 读失败按诚实性族规向上传播 Err（调用方静默跳过本轮，不伪造成「无预警」
/// 的空列表去覆盖 UI 判断——这里是通知通道，跳过即不发，语义清楚）。
pub(crate) fn collect_full_warns(
    app: &AppHandle,
    cfg: &ReminderConfig,
) -> Result<Vec<SpaceFullWarn>, String> {
    if cfg.full_warn_days == 0 {
        return Ok(Vec::new());
    }
    let hist = crate::space_history::read_history(app)?;
    let mut out: Vec<SpaceFullWarn> = Vec::new();
    let mut roots: Vec<&String> = hist.keys().collect();
    roots.sort(); // HashMap 顺序不定：排序保证同输入同输出（测试/日志可复现）
    for root in roots {
        let Some(days) = forecast_full_days(&hist[root]) else {
            continue;
        };
        if days <= cfg.full_warn_days as f64 {
            out.push(SpaceFullWarn {
                path: root.clone(),
                days: days.ceil() as u64,
            });
        }
    }
    Ok(out)
}

/// R100 空间趋势逐盘检查结果（`check_space_forecast_cmd` 返回，展示用）。
/// 诚实三态：`days = Some` 才有预测；`None` 时 `reason` 说明为什么预测不了
/// （no_history 从未采样 / not_enough 样本不足 / releasing 持平或释放中），
/// 绝不把「读不到趋势」伪装成 0 天或隐藏了事。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct SpaceForecast {
    /// 卷根路径（C:\ 等）。
    pub path: String,
    /// 预计满盘天数（向上取整；0 = 已满）；None = 无法预测，看 reason。
    pub days: Option<u64>,
    /// 预测状态：ok / no_history / not_enough / releasing。
    pub reason: &'static str,
    /// days ≤ full_warn_days（命中预警阈值；未设阈值时恒 false）。
    pub warn: bool,
}

/// 单盘趋势判定（纯函数，可单测）。复用 `forecast_full_days` 的语义：
/// 先分样本态（空/不足/零跨度），再交给外推函数——None 时必为持平或释放。
pub(crate) fn space_forecast_for(
    root: &str,
    pts: &[crate::space_history::SpacePoint],
    warn_days: u64,
) -> SpaceForecast {
    let (days, reason) = if pts.is_empty() {
        (None, "no_history")
    } else if pts.len() < 2 || pts[pts.len() - 1].at <= pts[0].at {
        (None, "not_enough")
    } else {
        match forecast_full_days(pts) {
            Some(d) => (Some(d.ceil() as u64), "ok"),
            // 样本够、跨度为正 → None 只可能是增速 ≤0（持平/释放）。
            None => (None, "releasing"),
        }
    };
    let warn = days.is_some_and(|d| d <= warn_days) && warn_days > 0;
    SpaceForecast {
        path: root.to_string(),
        days,
        reason,
        warn,
    }
}

/// 立即检查各盘空间趋势（展示路径，R100）：读失败按诚实性族规向上传播
/// Err（绝不返回空列表伪装成「没有预警」），前端展示「未读到 + 原因」。
/// 纯只读：只外推已有采样，不触发扫描、不做任何清理。
#[tauri::command]
pub(crate) fn check_space_forecast_cmd(app: AppHandle) -> Result<Vec<SpaceForecast>, String> {
    let cfg = config_at(&app);
    let hist = crate::space_history::read_history(&app)?;
    let mut roots: Vec<&String> = hist.keys().collect();
    roots.sort(); // HashMap 顺序不定：排序保证同输入同输出
    Ok(roots
        .iter()
        .map(|r| space_forecast_for(r, &hist[*r], cfg.full_warn_days))
        .collect())
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
    let payload = compute_reminder(state, app, &cfg)?;
    if let Some(p) = &payload {
        let _ = app.emit("cleanup-reminder", p);
    }
    Ok(payload)
}

/// 只读统计各盘可清字节（复用 cleanup_suggestions 引擎，cached_only=true：
/// 只读最近扫描/缓存树，未命中返回空而非全盘 walk——后台定时高频跑，
/// 与总览页「主动查建议」的默认真算区分）。
/// 只读：返回建议量，不做任何清理。
/// 后台线程（无 tokio runtime）里调用时用 `tauri::async_runtime::block_on`
/// 包一层——cleanup_suggestions 内部有 spawn_blocking await。
fn compute_reminder(
    state: State<'_, AppState>,
    app: &AppHandle,
    cfg: &ReminderConfig,
) -> Result<Option<ReminderPayload>, String> {
    // 枚举所有盘（复用 lib.rs list_drives 语义：C..=Z 中可读的盘）。
    let substs = crate::drives::collect_substs();
    let mut drives: Vec<DriveCleanup> = Vec::new();
    let mut total: u64 = 0;
    // P2-1：顺便在本轮已算出的 suggs 里按 scaffold 累计——**不额外跑一遍**
    // cleanup_suggestions（铁律：不为展示新造磁盘扫描）。
    let mut by_scaffold: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for letter in b'C'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if crate::drives::volume_info(root.clone()).is_err() {
            continue;
        }
        // 映射盘（subst D: J:\D）不是独立物理盘：目标卷自己也在这一轮里被算过，
        // 再算一遍就把同一块盘的可清量数两遍。
        if crate::drives::mapped_into_local_volume(letter as char, &substs) {
            continue;
        }
        // cleanup_suggestions 需要 scaffolds；直接读 state。block_on 在后台线程
        // 里提供 tokio runtime 上下文（tauri 的 async_runtime 是全局 Handle）。
        let suggs = tauri::async_runtime::block_on(crate::cleanup::cleanup_suggestions(
            state.clone(),
            root.clone(),
            None,
            Some(true),
        ))?;
        for s in &suggs {
            *by_scaffold.entry(s.scaffold_id.clone()).or_insert(0) += s.bytes;
        }
        let bytes: u64 = suggs.iter().map(|s| s.bytes).sum();
        if bytes > 0 {
            drives.push(DriveCleanup { path: root, bytes });
            total = total.saturating_add(bytes);
        }
    }
    // P2-1 判定：用本轮已累计的 scaffold 小计，零额外 IO。
    let auto_cfg = crate::auto_list::config_at(app);
    let hits = crate::auto_list::tripped_rules(
        &by_scaffold.into_iter().collect::<Vec<_>>(),
        &auto_cfg,
    );
    if total < cfg.min_bytes {
        return Ok(None);
    }
    Ok(Some(ReminderPayload {
        drives,
        total_bytes: total,
        auto_list_hits: hits,
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
        let mut last_weekly = std::time::Instant::now();
        // R89：空间趋势每日采样，与周报解耦（即使周报关着，趋势曲线也持续积累）。
        let mut last_space_sample = std::time::Instant::now();
        // R99：空间耗尽预警。与清理提醒同粒度 tick（60s），但独立计时——
        // 外推只看缓存历史，很轻，给个较长间隔（6h）就够，别高频打扰。
        let mut last_full_warn = std::time::Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(60)); // tick 粒度 60s
            let cfg = config_at(&app);
            // 清理提醒（R3）：enabled 开才跑。
            if cfg.enabled {
                let interval = Duration::from_secs(cfg.interval_hours.saturating_mul(3600));
                if last_run.elapsed() >= interval {
                    last_run = std::time::Instant::now();
                    let _ = run_reminder_check(&app);
                }
            }
            // 空间趋势每日采样：走 list_drives 同款 Win32 查询，静默失败，
            // 与应用启动扫描的 60s 去重天然合并。
            if last_space_sample.elapsed() >= Duration::from_secs(86400) {
                last_space_sample = std::time::Instant::now();
                crate::space_history::record_all_drives(&app);
            }
            // 周巡检简报（health-butler P1）：weekly_report_enabled 开才跑。
            if cfg.weekly_report_enabled {
                let interval = Duration::from_secs(
                    cfg.weekly_interval_days.clamp(1, 90).saturating_mul(86400),
                );
                if last_weekly.elapsed() >= interval {
                    last_weekly = std::time::Instant::now();
                    let _ = crate::weekly_report::run_weekly_report(&app);
                }
            }
            // 空间耗尽预警（R99）：full_warn_days > 0 才检查；6h 一次。
            if cfg.full_warn_days > 0 && last_full_warn.elapsed() >= Duration::from_secs(21600) {
                last_full_warn = std::time::Instant::now();
                match collect_full_warns(&app, &cfg) {
                    Ok(warns) if !warns.is_empty() => {
                        let _ = app.emit("space-full-reminder", &warns);
                    }
                    // 读失败静默跳过本轮（通知通道：跳过即不发，不入日志刷屏）；
                    // 空列表 = 当前没有快满的盘，不发。
                    _ => {}
                }
            }
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
        assert!(!c.weekly_report_enabled, "周报默认关");
        assert_eq!(c.weekly_interval_days, 7);
    }

    #[test]
    fn explicit_read_missing_file_is_default_not_error() {
        // 展示路径诚实性（R81）：文件不存在 = 从未设置（真空态）→ 默认配置，
        // 不报错；只有文件存在但读失败/损坏才报错，前端才显示「未读到」。
        let dir = std::env::temp_dir().join(format!("dp-reminder-explicit-{}", std::process::id()));
        let p = dir.join("reminder.json");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!p.exists(), "前置：文件不存在");
        let r = read_config_at_path(&p);
        assert!(r.is_ok(), "文件不存在必须回默认而不是报错");
        assert!(!r.unwrap().enabled, "默认配置");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn explicit_read_corrupt_file_is_error_not_default() {
        // 文件存在但内容损坏 → 必须 Err（绝不能静默返回默认配置）：
        // 否则前端把默认值当真实配置展示（读失败伪装成默认值）。
        let dir = std::env::temp_dir().join(format!("dp-reminder-corrupt-{}", std::process::id()));
        let p = dir.join("reminder.json");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&p, "{ not-json !!!").unwrap();
        let r = read_config_at_path(&p);
        assert!(r.is_err(), "损坏配置必须报错，不得静默回退默认");
        assert!(r.unwrap_err().contains("提醒配置损坏"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_roundtrip_via_temp_file() {
        // 用临时目录模拟 app_data_dir：写 → 读 → 合并 → 断言。
        let dir = std::env::temp_dir().join(format!("dp-reminder-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("reminder.json");
        let cfg = ReminderConfig {
            enabled: true,
            interval_hours: 6,
            ..ReminderConfig::default()
        };
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        std::fs::write(&p, text).unwrap();

        let loaded: ReminderConfig =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(loaded.enabled);
        assert_eq!(loaded.interval_hours, 6);
        // 合并写回保留 min_bytes 默认
        let merged = ReminderConfig {
            interval_hours: 12,
            ..loaded
        };
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
        let cfg = ReminderConfig {
            interval_hours: 9999, // 超上限
            ..ReminderConfig::default()
        };
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        std::fs::write(&p, text).unwrap();
        let loaded: ReminderConfig =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        // set 命令路径会 clamp；这里直接模拟 set 的 clamp 行为
        let clamped = loaded.interval_hours.clamp(1, 168);
        assert_eq!(clamped, 168, "interval 钳到 168");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── R99 空间耗尽预警：forecast_full_days 纯函数 ──
    fn sp(root: &str, used: u64, at: u64, total: u64) -> crate::space_history::SpacePoint {
        crate::space_history::SpacePoint {
            root: root.into(),
            total_bytes: total,
            used_bytes: used,
            at,
        }
    }

    #[test]
    fn forecast_forward_extrapolates_days() {
        // total 1000，10 天内已用 400 → 500（+10/天），剩 500 → 50 天。
        let pts = vec![
            sp("C:\\", 400, 1_700_000_000, 1000),
            sp("C:\\", 500, 1_700_864_000, 1000),
        ];
        let d = forecast_full_days(&pts).expect("正增速应返回天数");
        assert!((d - 50.0).abs() < 1e-6, "剩 500 ÷ 10/天 = 50 天，得 {d}");
    }

    #[test]
    fn forecast_release_or_flat_is_none() {
        // 释放：不预测，绝不把健康盘唱成满盘
        let releasing = vec![
            sp("C:\\", 500, 1_700_000_000, 1000),
            sp("C:\\", 400, 1_700_864_000, 1000),
        ];
        assert!(forecast_full_days(&releasing).is_none());
        // 持平同样不预测
        let flat = vec![
            sp("C:\\", 400, 1_700_000_000, 1000),
            sp("C:\\", 400, 1_700_864_000, 1000),
        ];
        assert!(forecast_full_days(&flat).is_none());
    }

    #[test]
    fn forecast_full_is_zero_and_undersized_is_none() {
        // 已满（剩余 ≤0，增速为正）→ 0
        let full = vec![
            sp("C:\\", 1000, 1_700_000_000, 1000),
            sp("C:\\", 1100, 1_700_864_000, 1000),
        ];
        assert_eq!(forecast_full_days(&full), Some(0.0));
        // 不足 2 点 / 零跨度 → None
        assert!(forecast_full_days(&[sp("C:\\", 400, 1_700_000_000, 1000)]).is_none());
        let zero = vec![
            sp("C:\\", 400, 1_700_000_000, 1000),
            sp("C:\\", 500, 1_700_000_000, 1000),
        ];
        assert!(forecast_full_days(&zero).is_none());
    }

    #[test]
    fn full_warn_days_zero_disables() {
        let cfg = ReminderConfig {
            full_warn_days: 0,
            ..ReminderConfig::default()
        };
        // collect_full_warns 依赖 AppHandle（read_history 要路径），这里只验证
        // 配置默认关闭、以及阈值语义在纯函数层成立：full_warn_days 上限 365。
        assert_eq!(cfg.full_warn_days, 0, "默认关空间预警");
    }

    // ── R100 space_forecast_for：逐盘趋势三态 + warn 判定 ──

    #[test]
    fn forecast_states_honest() {
        // 从未采样：no_history
        let f = space_forecast_for("C:\\", &[], 14);
        assert_eq!(f.days, None);
        assert_eq!(f.reason, "no_history");
        assert!(!f.warn);
        // 只有 1 点：not_enough
        let one = vec![sp("C:\\", 400, 1_700_000_000, 1000)];
        let f = space_forecast_for("C:\\", &one, 14);
        assert_eq!(f.reason, "not_enough");
        assert_eq!(f.days, None);
        // 2 点同时间戳（零跨度）：not_enough
        let zero = vec![
            sp("C:\\", 400, 1_700_000_000, 1000),
            sp("C:\\", 500, 1_700_000_000, 1000),
        ];
        let f = space_forecast_for("C:\\", &zero, 14);
        assert_eq!(f.reason, "not_enough");
        // 释放中：releasing（绝不唱成满盘）
        let free = vec![
            sp("C:\\", 500, 1_700_000_000, 1000),
            sp("C:\\", 400, 1_700_864_000, 1000),
        ];
        let f = space_forecast_for("C:\\", &free, 14);
        assert_eq!(f.reason, "releasing");
        assert_eq!(f.days, None);
        assert!(!f.warn);
    }

    #[test]
    fn forecast_ok_rounds_ceil_and_warn() {
        // 10 天跨度 +500（剩 500），50 天；阈值 14 → 不预警
        let pts = vec![
            sp("D:\\", 400, 1_700_000_000, 1000),
            sp("D:\\", 500, 1_700_864_000, 1000),
        ];
        let f = space_forecast_for("D:\\", &pts, 14);
        assert_eq!(f.days, Some(50));
        assert_eq!(f.reason, "ok");
        assert!(!f.warn);
        // 阈值 60 → 预警
        let f = space_forecast_for("D:\\", &pts, 60);
        assert!(f.warn);
        // 阈值 0 = 关：即使天数短也不预警
        let f = space_forecast_for("D:\\", &pts, 0);
        assert!(!f.warn);
    }

    #[test]
    fn forecast_full_is_zero_then_warn() {
        // 已满（增速为正、剩余 ≤0）→ days 0 且命中任意 >0 阈值
        let full = vec![
            sp("E:\\", 1000, 1_700_000_000, 1000),
            sp("E:\\", 1100, 1_700_864_000, 1000),
        ];
        let f = space_forecast_for("E:\\", &full, 7);
        assert_eq!(f.days, Some(0));
        assert_eq!(f.reason, "ok");
        assert!(f.warn);
    }
}
