//! 基准主函数域（从 bench.rs 拆分，2026-10-12）。
//! bench_cpu / bench_memory / bench_disk / stress_test / mem_test / stress_test_gpu。

use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::tools::files::PathGuard;
use crate::tools::hw;

use super::bench_pattern::{fill_pattern, verify_pattern};
use super::bench_util::{
    cancel_requested, clear_running_file, clear_stop_file, fastrand_hex, hw_probe_temp,
    measure_bandwidth, memory_latency, pick_threads, probe_temp_async, read_bench, run_par_bench,
    set_cancel, write_bench, write_running_file, RUNNING,
};

/// 1) CPU 基准：多线程整数（质数筛）+ 浮点（π 级数）吞吐。
///
/// `secs` 每项基准时长（默认 3，上限 30）；`threads` 线程数（默认=可用线程数，
/// 上限=线程数×2）。纯计算，只读 L0。
pub fn bench_cpu(secs: Option<u64>, threads: Option<usize>) -> Result<String, String> {
    let secs = secs.unwrap_or(3).clamp(1, 30);
    let n_threads = pick_threads(threads);
    let start = Instant::now();

    // 整数基准：质数计数（Queen 简化版，纯 CPU 整数/寻址压力）
    let int_ops = run_par_bench(n_threads, secs, move |tid, deadline| {
        let mut count: u64 = 0;
        let mut n: u64 = 2 + tid as u64;
        while Instant::now() < deadline {
            let mut prime = true;
            let mut d: u64 = 2;
            while d * d <= n {
                if n.is_multiple_of(d) {
                    prime = false;
                    break;
                }
                d += 1;
            }
            if prime {
                count += 1;
            }
            n += n_threads as u64; // 每个线程走自己的剩余类，互不重叠
        }
        count
    });

    // 浮点基准：π 级数累加（Leibniz 简化，FMA 压力）
    let fp_ops = run_par_bench(n_threads, secs, move |tid, deadline| {
        let mut iters = 0u64;
        let mut k = tid as f64;
        while Instant::now() < deadline {
            // 4/(4k+1) - 4/(4k+3) 交错项
            let term = 4.0 / (4.0 * k + 1.0) - 4.0 / (4.0 * k + 3.0);
            let _ = term * 0.999999; // 防编译期常量折叠，保持 FMA 压力
            iters += 1;
            k += n_threads as f64;
        }
        iters
    });

    let elapsed = start.elapsed().as_secs_f64();
    let int_total: u64 = int_ops.iter().sum();
    let fp_total: u64 = fp_ops.iter().sum();
    let mut out = String::from("CPU 基准结果：\n");
    out.push_str(&format!("线程：{n_threads} · 实测时长：{:.1}s\n", elapsed));
    out.push_str(&format!(
        "整数（质数筛）：{:.0} ops/s\n",
        int_total as f64 / elapsed
    ));
    out.push_str(&format!(
        "浮点（π 级数）：{:.0} iters/s\n",
        fp_total as f64 / elapsed
    ));
    out.push_str(
        "参考：Consumer i7 级 ≈ 1e7-1e8 ops/s（质数筛口径，非 AIDA64 官方分，同量级可比）。\n",
    );
    out.push_str("说明：纯计算只读，无文件写入；短跑受睿频/散热影响，多次取中位更稳。");
    Ok(out)
}

/// 2) 内存基准：读/写/复制带宽 + 延迟（指针追逐）。
///
/// 固定 64MB 缓冲区，避免全缓存命中；延迟用单线程链表追逐（ns）。
/// 纯只读（写基准写的是自身堆内存，无磁盘/系统副作用）L0。
pub fn bench_memory(secs: Option<u64>) -> Result<String, String> {
    let secs = secs.unwrap_or(3).clamp(1, 20);
    let mut buf = vec![0u8; 64 * 1024 * 1024]; // 64MB
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }

    // 读带宽：整块 memcpy 到临时缓存（copy_from_slice 走 libc memcpy，
    // debug/release 都是真实内存读速率；防优化取副本首字节）
    let mut read_dst = vec![0u8; 4 * 1024 * 1024]; // 4MB 读缓冲
    let read_mb = measure_bandwidth(
        secs,
        move |b| {
            let mut acc = 0u64;
            let mut off = 0usize;
            let chunk_len = read_dst.len();
            while off + chunk_len <= b.len() {
                read_dst.copy_from_slice(&b[off..off + chunk_len]);
                acc ^= read_dst[0] as u64;
                off += chunk_len;
            }
            acc
        },
        &mut buf,
    );

    // 写带宽：整块 fill（编译器向量化到缓存行粒度，接近真实写带宽）
    let write_mb = measure_bandwidth(
        secs,
        |b| {
            let mut acc = 0u64;
            for chunk in b.as_chunks_mut::<4096>().0 {
                let byte = (acc.wrapping_add(0x5A) & 0xFF) as u8;
                chunk.fill(byte);
                acc ^= 0xDEADBEEFCAFEBABEu64;
            }
            acc
        },
        &mut buf,
    );

    // 复制带宽
    let copy_mb = measure_bandwidth(
        secs,
        |b| {
            let mut dst = vec![0u8; b.len()];
            dst.copy_from_slice(b);
            dst[0] ^= b[b.len() - 1]; // 防优化
            dst[0] as u64
        },
        &mut buf,
    );

    // 延迟：指针追逐
    let lat_ns = memory_latency(secs);

    let mut out = String::from("内存基准结果：\n");
    out.push_str(&format!("读带宽：{read_mb} MB/s\n"));
    out.push_str(&format!("写带宽：{write_mb} MB/s\n"));
    out.push_str(&format!("复制带宽：{copy_mb} MB/s\n"));
    out.push_str(&format!("延迟（指针追逐）：{lat_ns:.1} ns\n"));
    out.push_str(
        "参考：DDR4-3200 双通道 ≈ 25-40 GB/s 读 / 20-30 GB/s 写 / 60-90ns 延迟；本结果为实测短跑。",
    );
    Ok(out)
}

/// 3) 磁盘基准：顺序/随机读 + 顺序/随机写。
///
/// 读基准：在 `path`（默认系统临时目录）建一次性文件（size_mb 默认 256MB），
/// 分块顺序/随机读，测完即删。写基准：另一个临时文件顺序/随机写，同样即删。
/// `dry_run=true`（默认）只出计划不创建文件；AI 应先给用户看计划，用户同意后
/// 再 `dry_run=false` 真正跑。全程只用临时文件，不碰用户数据目录。
pub fn bench_disk(
    path: Option<String>,
    size_mb: Option<u64>,
    dry_run: Option<bool>,
) -> Result<String, String> {
    let size_mb = size_mb.unwrap_or(256).clamp(16, 2048);
    let size_bytes = size_mb * 1024 * 1024;
    let dry = dry_run.unwrap_or(true);
    let dir = path
        .clone()
        .map(|p| p.trim_end_matches(['\\', '/']).to_string())
        .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());

    if dry {
        return Ok(format!(
            "磁盘基准计划（dry-run）：\n目录：{dir}\n文件：临时文件 {size_mb}MB（测后即删）\n项：顺序读 / 随机读 / 顺序写 / 随机写\n\n用户确认后可传 dry_run=false 执行。注意：临时目录所在磁盘即为被测盘，结果反映该盘真实 IO 能力。"
        ));
    }

    // 真跑（dry_run=false）会在 dir 下创建临时文件写入——这是写操作，目录必须
    // 过写守卫：盘根/系统目录/主目录根一律拒绝（对齐 file_recycle 等写工具，
    // 防止 AI 把磁盘基准的目标指到系统目录）。只读语义不受影响。
    let guard = PathGuard;
    if let Err(e) = guard.check_for_write(std::path::Path::new(&dir)) {
        return Err(format!(
            "磁盘基准目标目录被安全守卫拒绝（{e}）：{dir}。请改用系统临时目录或普通用户目录。"
        ));
    }

    let read_path = std::path::Path::new(&dir).join(format!(
        ".diskpilot_bench_read_{}_{}.tmp",
        std::process::id(),
        fastrand_hex()
    ));
    let write_path = std::path::Path::new(&dir).join(format!(
        ".diskpilot_bench_write_{}_{}.tmp",
        std::process::id(),
        fastrand_hex()
    ));

    // 建读基准文件（固定图案）
    let mut f =
        std::fs::File::create(&read_path).map_err(|e| format!("创建读基准文件失败：{e}"))?;
    let pattern = vec![0x5Au8; 64 * 1024];
    let mut written = 0u64;
    while written < size_bytes {
        let n = pattern.len().min((size_bytes - written) as usize);
        f.write_all(&pattern[..n])
            .map_err(|e| format!("写读基准文件失败：{e}"))?;
        written += n as u64;
    }
    f.sync_all().ok();

    let seq_read = read_bench(&read_path, size_bytes, false, 128 * 1024);
    let rand_read = read_bench(&read_path, size_bytes, true, 4 * 1024);

    // 写基准（顺序/随机写同一个临时文件）
    let seq_write = write_bench(&write_path, size_bytes, false, 128 * 1024);
    let rand_write = write_bench(&write_path, size_bytes, true, 4 * 1024);

    // 清理
    let _ = std::fs::remove_file(&read_path);
    let _ = std::fs::remove_file(&write_path);

    let mut out = String::from("磁盘基准结果：\n");
    out.push_str(&format!("目录：{dir}（临时文件已删除）\n"));
    out.push_str(&format!("顺序读（128KB 块）：{seq_read} MB/s\n"));
    out.push_str(&format!("随机读（4KB 块）：{rand_read} MB/s\n"));
    out.push_str(&format!("顺序写（128KB 块）：{seq_write} MB/s\n"));
    out.push_str(&format!("随机写（4KB 块）：{rand_write} MB/s\n"));
    out.push_str(
        "说明：SSD 顺序读 500-7000MB/s、随机 4K 读 20-200MB/s；HDD 顺序 100-250MB/s。写基准受缓存策略影响，多次取中位。",
    );
    Ok(out)
}

/// 4) 稳定性压测：CPU 多线程压测 + 超温熔断。
///
/// **L2 写类**（纯计算不写盘，但长时间满负荷占 CPU/升温，必须用户确认）。
/// `secs` 时长（默认 5 演示，上限 600）；`max_temp` 熔断阈值（默认 95，60-110）；
/// `threads` 线程数（默认可用线程数）。期间每 ~1s 采样一次 CPU 温度
/// （复用 hw::max_cpu_temp），超阈值立即停止并报告。
pub fn stress_test(
    secs: Option<u64>,
    max_temp: Option<f64>,
    threads: Option<usize>,
) -> Result<String, String> {
    let secs = secs.unwrap_or(5).clamp(1, 600);
    let max_temp = max_temp.unwrap_or(95.0).clamp(60.0, 110.0);
    let n_threads = pick_threads(threads);
    // 新一次压测开始：清历史停止信号 + 标记 RUNNING（供 stress_cancel 判断
    // 「确实有压测在跑」，避免空操作误报「已发送停止信号」）。
    set_cancel(false);
    clear_stop_file();
    RUNNING.store(true, Ordering::Relaxed);
    write_running_file();

    // 温度预取（异步，不阻塞压测启动）：fancmd 探测可能卡满 8s，同步等会让
    // 压测线程 0 启动、任务管理器看不到占用——「卡住」的根因。改为后台线程
    // 抢 3s 窗口，抢不到就等压测结束再取；熔断护栏靠监控循环的采样兜底。
    let (probe_h, probe_slot) = probe_temp_async();
    let start = Instant::now();

    // 启动压测线程（轮询全局熔断标志 + 自身 deadline 兜底，保证正常结束能退出）
    let deadline = start + Duration::from_secs(secs);
    let handles: Vec<_> = (0..n_threads)
        .map(|tid| {
            std::thread::spawn(move || {
                let mut n = 2u64 + tid as u64;
                let mut k = tid as f64;
                let mut int_ops = 0u64;
                let mut fp_ops = 0u64;
                while Instant::now() < deadline {
                    let mut prime = true;
                    let mut d = 2u64;
                    while d * d <= n {
                        if n.is_multiple_of(d) {
                            prime = false;
                            break;
                        }
                        d += 1;
                    }
                    if prime {
                        int_ops += 1;
                    }
                    n += n_threads as u64;
                    let term = 4.0 / (4.0 * k + 1.0) - 4.0 / (4.0 * k + 3.0);
                    let _ = term * 0.999999;
                    fp_ops += 1;
                    k += n_threads as f64;
                    // 停止信号：进程内 flag 或跨进程停止文件（后者解决「停止按钮
                    // 走新 spawn 实例、CANCEL 是另一进程的 flag」问题）
                    if cancel_requested() {
                        break;
                    }
                }
                (int_ops, fp_ops)
            })
        })
        .collect();

    // 温度监控循环：独立后台线程每 ~6s 采样一次（fancmd 单次 4.4s + 超时余量），
    // 记录 (已运行秒数, 温度) 供前端渲染曲线；超阈值 set_cancel(true) 触发压测线程退出。
    // 放后台线程的根因：采样本身（fancmd 4.4s）不能阻塞压测主循环，否则 3s 计划被拖成
    // 数倍（2026-09-14「stress_test 卡住」根因：采样在主线程 + 5s 超时被误杀）。
    let monitor = {
        std::thread::spawn(move || {
            let mut tripped = false;
            let mut trip_temp = 0.0f64;
            let mut samples: Vec<(f64, f64)> = Vec::new();
            let mut last_sample = Instant::now();
            let mut cancelled = false;
            // 采样间隔自适应：压测短（<12s）用 2s 间隔，长压测用 6s——fancmd
            // 单次 ~4.4s，5s 短压测用 6s 间隔一次都采不到，温度护栏等于摆设
            // （2026-09-15 修：短压测也能采样，长压测不因此变慢）。
            let s_interval = if secs < 12 {
                Duration::from_secs(2)
            } else {
                Duration::from_secs(6)
            };
            while start.elapsed() < Duration::from_secs(secs) {
                // 用户点「停止」（stress_cancel / 前端停止按钮）→ 立即退出；
                // 停止文件是跨进程信号：前端停止按钮走新 spawn 实例也有效。
                if cancel_requested() {
                    cancelled = true;
                    break;
                }
                if last_sample.elapsed() >= s_interval {
                    if let Ok(t) = hw_probe_temp() {
                        samples.push((start.elapsed().as_secs_f64(), t));
                        if t >= max_temp {
                            tripped = true;
                            trip_temp = t;
                            set_cancel(true);
                            break;
                        }
                    }
                    // 采样失败不中断压测（降级：无熔断护栏运行，末尾如实提示）
                    last_sample = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            (tripped, trip_temp, samples, cancelled)
        })
    };

    // 收尾：等压测线程退出（采样线程并行跑，不阻塞）
    let mut int_ops = 0u64;
    let mut fp_ops = 0u64;
    for h in handles {
        if let Ok((i, f)) = h.join() {
            int_ops += i;
            fp_ops += f;
        }
    }
    // 取消信号必须在复位**之前**捕获：监控线程可能正阻塞在 hw_probe_temp
    // （fancmd 最长 ~8s），主线程先 set_cancel(false)/clear_stop_file() 会让它
    // 醒来后读到「无取消」→ 返回 cancelled=false → 1s join 窗口超时走兜底
    // (false,…) → 用户按了停止却报「✅ 正常结束」（2026-10-11 修复的报告竞态）。
    let cancel_seen = cancel_requested();
    set_cancel(false);
    RUNNING.store(false, Ordering::Relaxed);
    clear_stop_file();
    clear_running_file();
    let elapsed = start.elapsed().as_secs_f64();

    // 温度预取收尾：探测线程与压测并行跑，压测结束后给它 ≤2.5s 的有限短窗
    // 把温度带回来（fancmd 正常机器 3-5s 出值，压测 ≥5s 基本等得到；短压测/
    // 探测慢则如实降级「无熔断护栏」，绝不阻塞返回）。
    let pre_temp = {
        let deadline = Instant::now() + Duration::from_millis(2500);
        loop {
            {
                let g = probe_slot.lock().unwrap();
                if let Some(r) = g.as_ref() {
                    break r.clone();
                }
            }
            if Instant::now() >= deadline {
                break Err(
                    "温度探测未在时限内完成（fancmd/ACPI 探测慢或权限不足，可提权后重试）".into(),
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    let _ = probe_h; // 探测线程句柄：不 join（避免等满 8s 超时），压测返回后进程退出即回收

    // 监控线程收尾：熔断动作已在监控线程内通过 set_cancel 生效（压测线程被
    // 提前中断即熔断证明），这里只为拿曲线/熔断文案——同样只给 1s 短窗，
    // 拿不到就放弃，监控线程卡在 hw_probe_temp（最长 8s+3s）不阻塞返回。
    let (tripped, trip_temp, samples, cancelled) = {
        let deadline = Instant::now() + Duration::from_millis(1000);
        loop {
            if monitor.is_finished() {
                match monitor.join() {
                    Ok(v) => break v,
                    Err(_) => break (false, 0.0f64, Vec::new(), false),
                }
            }
            if Instant::now() >= deadline {
                break (false, 0.0f64, Vec::new(), false);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    // 合成最终取消态：监控线程观察到的 OR 复位前捕获到的。超温熔断也会
    // set_cancel(true)（见监控循环），因此用 !tripped 排除，避免把熔断误报
    // 成「用户取消」——熔断有自己更准确的 ⚠️ 分支。
    let cancelled = cancelled || (cancel_seen && !tripped);
    // 峰值 = 采样曲线最大值；压测前温度更高时取它（避免「峰值低于压测前」的假象）
    let peak = samples
        .iter()
        .map(|(_, v)| *v)
        .fold(f64::NEG_INFINITY, f64::max);
    let peak = if peak.is_finite() && peak > 0.0 {
        peak
    } else {
        pre_temp.as_ref().copied().unwrap_or(0.0)
    };

    let mut out = String::from("稳定性压测结果：\n");
    out.push_str(&format!(
        "线程：{n_threads} · 计划时长：{secs}s · 实际：{:.1}s\n",
        elapsed
    ));
    match &pre_temp {
        Ok(t) => out.push_str(&format!("压测前 CPU 温度：{:.1}℃\n", t)),
        Err(e) => out.push_str(&format!("压测前温度不可读：{e}\n")),
    }
    out.push_str(&format!("峰值 CPU 温度：{peak:.1}℃\n"));
    if peak <= 0.0 {
        out.push_str("⚠️ 温度监控不可用（fancmd/ACPI 均未读到温度）：本次压测无超温熔断护栏，属高风险裸跑。\n");
    }
    if !samples.is_empty() {
        // 温度曲线行：秒数=温度℃（逗号分隔），供前端渲染实时折线 / AI 判断升温趋势
        let line: Vec<String> = samples
            .iter()
            .map(|(t, v)| format!("{:.1}s={v:.1}°C", t))
            .collect();
        out.push_str(&format!("温度曲线：{}\n", line.join("; ")));
    }
    out.push_str(&format!(
        "整数运算：{:.0} ops/s · 浮点迭代：{:.0}/s\n",
        int_ops as f64 / elapsed,
        fp_ops as f64 / elapsed
    ));
    if cancelled {
        out.push_str("🛑 已按停止信号结束（用户取消）——压测已提前停止。\n");
    } else if tripped {
        out.push_str(&format!(
            "⚠️ 超温熔断：CPU 温度达到 {trip_temp:.1}℃（阈值 {max_temp:.1}℃），已自动停止压测保护硬件。\n"
        ));
    } else {
        out.push_str(&format!(
            "✅ 正常结束：未超 {max_temp:.1}℃ 阈值，系统稳定。\n"
        ));
    }
    out.push_str("说明：纯计算满载，无文件写入；压测升温属正常，超过阈值已自动熔断。");
    Ok(out)
}

/// 内存稳定性测试（MemTest86 简化版）：进程内堆分配 size_mb，用 5 种数据
/// pattern 反复写入并读回校验，发现位翻转/不稳定内存。
///
/// 纯只读（写自身堆内存，无磁盘/系统副作用）L0，但耗时稍长。
///
/// - `size_mb` 缓冲大小（默认 256，64-4096）
/// - `rounds` 轮数（默认 1，1-10）
pub fn mem_test(size_mb: Option<u64>, rounds: Option<u32>) -> Result<String, String> {
    let size_mb = size_mb.unwrap_or(256).clamp(64, 4096);
    let rounds = rounds.unwrap_or(1).clamp(1, 10);
    let n = (size_mb * 1024 * 1024) as usize;
    let mut buf = vec![0u8; n];

    let name_of = ["全零 0x00", "全一 0xFF", "AA/55 交替", "字节递增", "伪随机"];
    let start = Instant::now();
    let mut total_errs = 0u64;
    let mut global_first: Option<usize> = None;
    let mut failed_patterns: Vec<String> = Vec::new();

    for r in 1..=rounds {
        for (pi, &kind) in [0u8, 1, 2, 3, 4].iter().enumerate() {
            fill_pattern(&mut buf, kind);
            let (errs, first) = verify_pattern(&buf, kind);
            total_errs += errs;
            if global_first.is_none() {
                global_first = first;
            }
            if errs > 0 {
                failed_patterns.push(format!("第{r}轮·{}：{errs} 字节不匹配", name_of[pi]));
            }
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let total_mb = n as u64 * 5 * rounds as u64 / 1024 / 1024;

    let mut out = String::from("内存稳定性测试结果（MemTest86 简化版）：\n");
    out.push_str(&format!("分配：{size_mb} MB（进程内堆，无磁盘副作用）\n"));
    out.push_str(&format!(
        "模式：5 种 · 轮数：{rounds} · 实测：{:.1}s · 总校验：{total_mb} MB\n",
        elapsed
    ));
    if total_errs == 0 {
        out.push_str("✅ 读写校验通过：全部 5 种 pattern 重写+读回一致，未发现位翻转。\n");
    } else {
        let first_desc = global_first
            .map(|i| format!("首个错误偏移 0x{i:x} ≈ {} MB 处", i / 1024 / 1024))
            .unwrap_or_default();
        out.push_str(&format!(
            "⚠️ 共 {total_errs} 个字节校验失败（{first_desc}）。\n"
        ));
        for p in &failed_patterns {
            out.push_str(&format!("  - {p}\n"));
        }
        out.push_str(
            "可能原因：内存条不稳定 / XMP 超频过激 / 散热不足。建议进一步跑完整 MemTest86。\n",
        );
    }
    out.push_str(
        "说明：真实 MemTest86 在 pre-boot 环境可覆盖全部寻址空间；本工具测进程堆内存的读写正确性，能发现明显位翻转，但覆盖不了 BIOS/硬件寻址层。",
    );
    Ok(out)
}

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
