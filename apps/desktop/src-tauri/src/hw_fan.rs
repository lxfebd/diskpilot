//! 风扇域（从 hw.rs 拆分，2026-10-12）。
//! fan_* 命令层：建议 / 控制（多通道探测 + 温度熔断回退）/ 状态 / 恢复默认。
//! 通道探测与调速实现见 `hw_fan_probe` 子模块。

use std::time::Duration;

use tauri::{AppHandle, Emitter, State};

use super::hw_core::{hw_info_cached, ps_capture};
use super::hw_test::sample_thermal_c;
use crate::hw_report::build_fan_advice;
use crate::AppState;
use hw_fan_probe::{
    cli_restore_fan_speed, cli_set_fan_speed, fan_guard_running, fan_guard_set, fan_probe_all,
    fan_snapshot_lock, FAN_GUARD_POLL, FAN_PS, FAN_TEMP_BREAKER_C,
};

/// 档位换算对外兼容（hw.rs 的 speed_pct_for_levels 测试引用 crate::hw::hw_fan::）。
pub(crate) use hw_fan_probe::speed_pct_for;

/// 风扇曲线建议（L2）：读温度传感器 + CPU 负载 + 风扇信息（WMI 能读多少算多少），
/// 生成「当前散热状态 + 建议曲线锚点」。只出建议；落地调速走 fan_control（L2，多通道探测）。
#[tauri::command]
pub(crate) async fn fan_curve_advice(refresh: Option<bool>) -> Result<serde_json::Value, String> {
    let force = refresh.unwrap_or(false);
    tokio::task::spawn_blocking(move || {
        let info = hw_info_cached(force)?;
        let mut advice = build_fan_advice(&info);
        // 风扇列表单独查（hw_info 不含）；读失败不致命——建议主体来自温度/负载。
        let fans_json = ps_capture(FAN_PS, Duration::from_secs(20))
            .ok()
            .and_then(|raw| {
                let raw = raw.trim().trim_start_matches('\u{feff}');
                serde_json::from_str::<serde_json::Value>(raw).ok()
            });
        if let Some(fj) = fans_json {
            advice["fans"] = fj.get("fans").cloned().unwrap_or(serde_json::json!([]));
        } else {
            advice["fans"] = serde_json::json!([]);
        }
        Ok(advice)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[path = "hw_fan_probe.rs"]
mod hw_fan_probe;

/// 风扇控制（L2 写操作）：确认 + 权限中心双重校验后，按档位/自定义曲线写入调速通道。
/// 写前把原转速快照存到进程级静态（OnceLock<Mutex>，与 THERMAL_CACHE 同款），
/// 然后 spawn 后台温度守护线程：5s 采样一次，超阈值自动回退原转速并退出。
/// 返回应用结果。前端另有 5s visibility 门控轮询读 fan_status 长期展示状态。
#[tauri::command]
pub(crate) async fn fan_control(
    app: AppHandle,
    state: State<'_, AppState>,
    level: Option<String>,
    curve: Option<Vec<serde_json::Value>>,
    temp_breaker_c: Option<f32>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    // 纵深防御：confirmed + perm_grants 双重校验（对齐 toolbelt_run / plugin_* 模板）。
    if confirmed != Some(true) {
        return Err(
            "fan:confirm: 风扇控制会写入调速通道，必须由用户明确确认（confirmed=true）。请先在确认面板中让用户确认。"
                .into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("fan.control") {
        return Err(
            "fan:denied: 未开启「风扇控制」权限（fan.control）。请先在权限中心开启后重试。".into(),
        );
    }
    let breaker_c = temp_breaker_c
        .unwrap_or(FAN_TEMP_BREAKER_C)
        .clamp(70.0, 105.0);
    let app2 = app.clone();
    tokio::task::spawn_blocking(move || fan_control_blocking(app2, level, curve, breaker_c))
        .await
        .map_err(|e| e.to_string())?
}

fn fan_control_blocking(
    app: AppHandle,
    level: Option<String>,
    curve: Option<Vec<serde_json::Value>>,
    breaker_c: f32,
) -> Result<serde_json::Value, String> {
    let probe = fan_probe_all();
    if !probe["writable"].as_bool().unwrap_or(false) {
        return Ok(serde_json::json!({
            "ok": false,
            "applied": false,
            "probe": probe,
            "note": "当前无可写调速通道，未执行任何写入。这是如实降级：DiskPilot 不会在没有可用通道时伪造控制结果。",
            "recommend": "可安装 NoteBook FanControl（nbfc）获得可写 CLI 通道后重试。"
        }));
    }
    let speed_pct = speed_pct_for(level.as_deref(), curve.as_deref())?;

    // 读取当前转速作为「原转速」快照：Win32_Fan 只有 DesiredSpeed（-1 表自动），
    // 读不到就用 100（主板自动接管的手感）。仅首次应用保存；后续应用不覆盖，
    // 熔断始终回退到最早一次应用前的转速。
    let cli = probe["cli"].clone();
    let current_pct: u32 = probe["wmi"]["fans"]
        .as_array()
        .and_then(|fans| fans.first())
        .and_then(|f| f["desired_rpm"].as_i64())
        .and_then(|v| if v >= 0 { Some(v as u32) } else { None })
        .unwrap_or(100)
        .clamp(0, 100);
    {
        let mut snap = fan_snapshot_lock().lock().unwrap();
        if snap.is_none() {
            *snap = Some(current_pct);
        }
    }
    let original_pct = fan_snapshot_lock()
        .lock()
        .unwrap()
        .unwrap_or(100)
        .clamp(0, 100);

    // 写入。
    cli_set_fan_speed(&cli, speed_pct).map_err(|e| format!("写入调速通道失败：{e}"))?;

    // 后台温度熔断守护：只 spawn 一次（避免多次应用叠出多个守护线程互踩）。
    // 守护线程 5s 采样一次温度；读到 ≥ 阈值就自动回退到原转速，emit 事件，退出。
    if !fan_guard_running() {
        fan_guard_set(true);
        std::thread::spawn(move || loop {
            std::thread::sleep(FAN_GUARD_POLL);
            if let Some(c) = sample_thermal_c() {
                if c >= breaker_c {
                    let reverted = cli_set_fan_speed(&cli, original_pct).is_ok();
                    let note = format!(
                        "温度达到 {c:.0}℃（≥ 熔断阈值 {breaker_c:.0}℃），已自动回退原转速 {}%。",
                        original_pct
                    );
                    let _ = app.emit(
                        "fan-temp-breaker",
                        &serde_json::json!({
                            "temp_c": c,
                            "breaker_c": breaker_c,
                            "reverted": reverted,
                            "original_speed_pct": original_pct,
                            "note": note,
                        }),
                    );
                    fan_guard_set(false);
                    break;
                }
            }
        });
    }

    Ok(serde_json::json!({
        "ok": true,
        "applied": true,
        "channel": probe["best_channel"],
        "speed_pct": speed_pct,
        "original_speed_pct": original_pct,
        "temp_breaker_c": breaker_c,
        "guard_running": true,
        "probe": probe,
        "note": format!("已通过 {} 通道应用转速 {}%；后台温度守护已启动（阈值 {breaker_c:.0}℃，超限自动回退 {}%）。", probe["best_channel"], speed_pct, original_pct)
    }))
}

/// 风扇状态（只读 L0）：通道探测 + 当前温度 + 是否有快照/守护在跑。
/// 供前端面板轮询与 AI 只读查询。
#[tauri::command]
pub(crate) async fn fan_status() -> Result<serde_json::Value, String> {
    let has_snapshot = fan_snapshot_lock().lock().unwrap().is_some();
    let guard_running = fan_guard_running();
    tokio::task::spawn_blocking(move || {
        let probe = fan_probe_all();
        let temp_c = sample_thermal_c();
        Ok(serde_json::json!({
            "probe": probe,
            "temp_c": temp_c,
            "writable": probe["writable"],
            "has_snapshot": has_snapshot,
            "guard_running": guard_running,
            "note": probe["note"],
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 恢复默认（L2 写操作）：确认 + 权限中心双重校验后，把转速写回 100% 让主板
/// 自动接管，并清空原转速快照（后续应用再存新快照）。
#[tauri::command]
pub(crate) async fn fan_curve_apply(
    state: State<'_, AppState>,
    restore_default: Option<bool>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "fan:confirm: 恢复默认风扇转速需要用户明确确认（confirmed=true）。请先在确认面板中让用户确认。"
                .into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("fan.control") {
        return Err(
            "fan:denied: 未开启「风扇控制」权限（fan.control）。请先在权限中心开启后重试。".into(),
        );
    }
    let do_restore = restore_default.unwrap_or(false);
    tokio::task::spawn_blocking(move || {
        let probe = fan_probe_all();
        if !do_restore {
            return Ok(serde_json::json!({
                "ok": true,
                "applied": false,
                "probe": probe,
                "note": "未执行操作（restore_default 未开启）。"
            }));
        }
        if !probe["writable"].as_bool().unwrap_or(false) {
            return Ok(serde_json::json!({
                "ok": false,
                "applied": false,
                "probe": probe,
                "note": "当前无可写调速通道，未执行任何写入。如实降级：没有可用通道时不伪造控制结果。"
            }));
        }
        let cli = probe["cli"].clone();
        cli_restore_fan_speed(&cli).map_err(|e| format!("恢复默认转速失败：{e}"))?;
        // 清空快照：主板已接管，后续应用重新存原转速。
        *fan_snapshot_lock().lock().unwrap() = None;
        fan_guard_set(false);
        Ok(serde_json::json!({
            "ok": true,
            "applied": true,
            "note": "已恢复主板自动控制，主板将接管风扇策略。"
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}
