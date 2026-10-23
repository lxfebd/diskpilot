// 全应用共用的健康阈值，统一放一处。
// 目的：同一件事不许两处各写一个数——改一处，另一处就给出互相矛盾的结论。
//
// TEMP_WARN_C：温度警戒线。与后端 apps/desktop/src-tauri/src/hw_report/fan.rs
// 的 `t >= 80.0` 判「偏热」同口径；体检条温度格、温度采样卡、周报温度项都用它。
export const TEMP_WARN_C = 80;

// SCAN_STALE_WARN_DAYS：扫描时效「最久 N 天没扫」的分界线（默认）。
// 与后端 collect_scan_stale 的默认值同口径；用户可在提醒设置里改配置，这里只是展示默认。
export const SCAN_STALE_WARN_DAYS = 21;

// DEFAULT_THERMAL_SUSTAINED_C：持续超温提醒的默认阈值（℃）。
// 与后端 temp_breaker 的默认值同口径；提醒设置里可改。
export const DEFAULT_THERMAL_SUSTAINED_C = 85;
