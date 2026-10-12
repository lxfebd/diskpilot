//! 温度采样域：THERMAL_TTL 缓存 / sample_thermal_c / read_thermal_max_c。
//! 从 hw_test.rs 拆分（2026-10-12）。TTL 缓存避免压测循环进程风暴，
//! None（传感器不可读）也缓存；热区未就绪标记值 2731 单独排除。

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::hw::ps_capture;
/// 温度采样 TTL 缓存：传感器读数秒级内不会突变，10 秒内直接复用，
/// 避免压测循环每 2 秒 spawn 一个 PowerShell（长测试最多 ~900 次进程风暴）。
/// None（传感器不可读）也缓存：不可读的机器不会每轮都白 spawn 一遍。
const THERMAL_TTL: Duration = Duration::from_secs(10);
/// (采样时刻, 传感器温度)。温度可能读不到（None）。
type ThermalEntry = (std::time::Instant, Option<f32>);
type ThermalCache = Mutex<Option<ThermalEntry>>;
static THERMAL_CACHE: OnceLock<ThermalCache> = OnceLock::new();
fn thermal_cache_lock() -> &'static ThermalCache {
    THERMAL_CACHE.get_or_init(|| Mutex::new(None))
}

/// 采样当前温度（带 TTL 缓存），读不到返回 None。
pub(crate) fn sample_thermal_c() -> Option<f32> {
    let now = std::time::Instant::now();
    if let Ok(guard) = thermal_cache_lock().lock() {
        if let Some((at, v)) = guard.as_ref() {
            if now.duration_since(*at) < THERMAL_TTL {
                return *v;
            }
        }
    }
    let v = read_thermal_max_c();
    if let Ok(mut guard) = thermal_cache_lock().lock() {
        *guard = Some((std::time::Instant::now(), v));
    }
    v
}

/// 读本机 CPU/GPU 温度传感器的最大值（0.1K 单位），失败返回 None。
/// 真正的 PowerShell spawn 点；调用方应走 sample_thermal_c() 走缓存。
fn read_thermal_max_c() -> Option<f32> {
    let script = "(Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue | Measure-Object -Property CurrentTemperature -Maximum).Maximum";
    let s = ps_capture(script, Duration::from_secs(8)).ok()?;
    let v: f64 = s.trim().parse().ok()?;
    let c = v / 10.0 - 273.15;
    // 热区未就绪报标记值 2731（=27.31℃，恰好落入 0~120 区间）——必须单独排除，
    // 否则把「未就绪假值」当成真实温度显示（电脑温度=室温 的假象根源）。
    if (0.0..=120.0).contains(&c) && !(27.0..=27.6).contains(&c) {
        Some(c as f32)
    } else {
        None
    }
}
