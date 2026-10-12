//! 卸载推荐逻辑（§6.5）：按体积/冷启动时间推荐，系统级组件永不推荐。
pub(crate) const SECONDS_PER_MONTH: f64 = 60.0 * 60.0 * 24.0 * 30.4375;

/// Compute (default_recommended, recommendation_reason) per §6.5 of the
/// design doc. `now_secs` is injected so tests can pin time.
///
/// System-level components (SteamVR runtime, Steamworks redistributables,
/// Steam client itself) are never recommended — uninstalling them breaks
/// other games, so the UI must not surface them as reclaimable.
/// appid → 名称（仅注释，不参与逻辑）：
///   30        Steam（客户端本体）
///   240       Steam App（客户端辅助）
///   480       Steam Client（旧版客户端）
///   675        Steamworks Common Redistributables
///   713       Steam Client Bootstrapper
///   570       Steamworks Shared Redistributables
///   105600    Steam Remote Play
///   228980    SteamVR for HTC Vive
///   250820    SteamVR（运行时）
///   446873    SteamVR for Valve Index
///   1675200   Steam Client Beta
pub(crate) const SYSTEM_APPS: &[u32] = &[
    30, 240, 480, 675, 570, 713, 105600, 228980, 250820, 446873, 1675200,
];

pub(crate) fn compute_recommendation(
    size_bytes: u64,
    last_played_ts: Option<u64>,
    is_ghost: bool,
    now_secs: u64,
    appid: u32,
) -> (bool, Option<String>) {
    if SYSTEM_APPS.contains(&appid) {
        return (false, None);
    }
    if is_ghost {
        return (true, Some("ACF 存在但安装目录缺失".to_string()));
    }
    let size_gb = size_bytes as f64 / 1_000_000_000.0;
    if size_gb < 5.0 {
        return (false, None);
    }
    let months_since: f64 = match last_played_ts {
        None => 12.0, // "never played" treated as 12-month-stale per §6.5
        Some(t) if t >= now_secs => 0.0,
        Some(t) => (now_secs - t) as f64 / SECONDS_PER_MONTH,
    };
    let recommended =
        (size_gb >= 30.0 && months_since >= 6.0) || (size_gb >= 50.0 && months_since >= 3.0);
    if !recommended {
        return (false, None);
    }
    let size_str = if size_gb >= 10.0 {
        format!("{:.0}GB", size_gb)
    } else {
        format!("{:.1}GB", size_gb)
    };
    let time_str = if last_played_ts.is_none() {
        "从未启动".to_string()
    } else {
        let m = months_since as u32;
        if m >= 24 {
            format!("{} 年未启动", m / 12)
        } else if m >= 12 {
            "1 年未启动".to_string()
        } else {
            format!("{} 个月未启动", m.max(1))
        }
    };
    (true, Some(format!("{} · {}", size_str, time_str)))
}

pub(crate) fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
