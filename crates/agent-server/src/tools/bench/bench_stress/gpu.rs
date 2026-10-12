//! GPU 稳定性压测域：stress_test_gpu（FurMark 对齐，温度熔断守门版）。
//! 从 bench_stress.rs 拆分（2026-10-12）。纯后端无法让 GPU 满载，本工具
//! 如实说明压载引擎边界，承担温度熔断守门（nvidia-smi 采样）。

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::tools::hw;

use crate::tools::bench::bench_util::{
    cancel_requested, clear_running_file, clear_stop_file, set_cancel, write_running_file, RUNNING,
};

/// 6) GPU 稳定性压测（FurMark 对齐，温度熔断守门版）。
///
/// 纯后端无法让 GPU 满载（深度压载需 OpenCL/CUDA/WebGL 图形引擎，属厂商 SDK 范畴，
/// 同 G12 GPGPU 结论）。本工具**如实说明**压载引擎边界，并承担温度熔断守门：
/// 用户在前端「GPU 甜甜圈」WebGL 压测页 / 外挂 FurMark（toolbelt_run）/ 游戏/渲染任务
/// 进行 GPU 负载时，本工具在 secs 秒内持续采样 GPU 温度（nvidia-smi），超过 max_temp
/// 即报熔断信号保护硬件。无 N 卡 / 无 nvidia-smi → 如实降级「无温度护栏」。
///
/// **写类 L1**（长时间占资源/升温，需用户确认，走 hw.stress 权限门）。
/// 6) GPU 稳定性压测（FurMark 对齐，温度熔断守门版）。
///
/// 纯后端无法让 GPU 满载（深度压载需 OpenCL/CUDA/WebGL 图形引擎，属厂商 SDK 范畴，
/// 同 G12 GPGPU 结论）。本工具**如实说明**压载引擎边界，并承担温度熔断守门：
/// 用户在前端「GPU 甜甜圈」WebGL 压测页 / 外挂 FurMark（toolbelt_run）/ 游戏/渲染任务
/// 进行 GPU 负载时，本工具在 secs 秒内持续采样 GPU 温度（nvidia-smi），超过 max_temp
/// 即报熔断信号保护硬件。无 N 卡 / 无 nvidia-smi → 如实降级「无温度护栏」。
///
/// **写类 L1**（长时间占资源/升温，需用户确认，走 hw.stress 权限门）。
pub fn stress_test_gpu(secs: Option<u64>, max_temp: Option<f64>) -> Result<String, String> {
    let secs = secs.unwrap_or(5).clamp(1, 120);
    let max_temp = max_temp.unwrap_or(90.0).clamp(60.0, 110.0);
    set_cancel(false);
    clear_stop_file();
    RUNNING.store(true, Ordering::Relaxed);
    write_running_file();

    // GPU 概览（可降级）
    let gpu_line = hw::collect_gpu_snapshot()
        .map(|s| s.lines().next().unwrap_or("").trim().to_string())
        .unwrap_or_else(|_| "GPU 信息读取失败".into());

    let temp_src = hw::gpu_temp_c();

    let mut out = String::from("GPU 稳定性压测结果：\n");
    out.push_str(&format!("显卡：{gpu_line}\n"));
    match &temp_src {
        Ok(t) => out.push_str(&format!("压测前 GPU 温度：{:.0}℃\n", t)),
        Err(e) => out.push_str(&format!("⚠️ 无 GPU 温度源（{e}）——温度熔断护栏不可用\n")),
    }
    out.push_str(
        "压载引擎：本工具集未内置 GPU 渲染/计算引擎（深度压载需 OpenCL/CUDA/WebGL，属厂商 SDK 范畴）。\n",
    );
    out.push_str(
        "压载方式：1) 前端基准面板「GPU 甜甜圈」WebGL 全屏压测页；2) 外挂 FurMark（toolbelt_run）。\n",
    );

    if temp_src.is_ok() {
        out.push_str(&format!(
            "温度熔断守门：接下来 {secs}s 持续采样 GPU 温度（你的压测程序正在运行时即生效），超过 {:.0}℃ 即报熔断信号…\n",
            max_temp
        ));
        let start = Instant::now();
        let mut peak = temp_src.as_ref().copied().unwrap_or(0.0);
        let mut tripped = false;
        let mut trip_temp = 0.0f64;
        let mut last = Instant::now();
        let mut cancelled = false;
        while start.elapsed() < Duration::from_secs(secs) {
            // 用户点「停止」→ set_cancel(true)/停止文件，立即退出监控
            // （stress_cancel 工具 / 前端停止按钮；文件信号跨进程生效）
            if cancel_requested() {
                cancelled = true;
                break;
            }
            if last.elapsed() >= Duration::from_secs(1) {
                if let Ok(t) = hw::gpu_temp_c() {
                    if t > peak {
                        peak = t;
                    }
                    if t >= max_temp {
                        tripped = true;
                        trip_temp = t;
                        break;
                    }
                }
                last = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        out.push_str(&format!("峰值 GPU 温度：{peak:.0}℃\n"));
        if cancelled {
            out.push_str("🛑 已按停止信号结束（用户取消）——建议确认压测程序已退出。\n");
        } else if tripped {
            out.push_str(&format!(
                "⚠️ 超温熔断信号：GPU 温度达到 {trip_temp:.0}℃（阈值 {max_temp:.0}℃），建议立即停止压测程序保护硬件。\n"
            ));
        } else {
            out.push_str(&format!(
                "✅ {secs}s 监控完成，未超 {max_temp:.0}℃ 阈值，温度稳定。\n"
            ));
        }
    } else {
        out.push_str("⚠️ 无温度源 → 熔断护栏失效，请确认 NVIDIA 显卡驱动（nvidia-smi）可用。\n");
    }
    set_cancel(false);
    RUNNING.store(false, Ordering::Relaxed);
    clear_stop_file();
    clear_running_file();
    out.push_str(
        "说明：GPU 满载压测的核心是渲染引擎（FurMark 甜甜圈），纯后端无法复刻；本工具承担温度熔断守门与压载方式引导。",
    );
    Ok(out)
}
