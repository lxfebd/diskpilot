//! 压测内部工具（从 bench.rs 拆分，2026-10-12）。
//! 跨进程停止/运行标志文件 + 线程数/取消/并行基准/带宽/延迟/温度探测/随机 hex。

use std::io::{Read, Seek, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

/// 跨进程停止信号：`%TEMP%/diskpilot-stress-stop.flag`
///
/// 压测（stress_test / stress_test_gpu）跑在某个 agent-server 进程实例内，
/// 而前端「停止」按钮 / `stress_cancel` 工具走的是另一次 spawn 的全新实例
/// （agent.rs 每次调用重新 spawn，进程级 CANCEL flag 跨进程无效）。因此停止
/// 信号必须落在压测监控循环能看见的地方：一个临时目录下的标志文件。
/// 压测开始清文件、监控循环每 200ms 查文件、`stress_cancel` 写文件——
/// 无论哪个进程实例调用，压测进程都能在下一轮循环检测到并退出。
fn stress_stop_file() -> std::path::PathBuf {
    std::env::temp_dir().join("diskpilot-stress-stop.flag")
}

fn write_stop_file() {
    let _ = std::fs::write(stress_stop_file(), b"1");
}

pub(crate) fn clear_stop_file() {
    let _ = std::fs::remove_file(stress_stop_file());
}

fn stop_file_exists() -> bool {
    stress_stop_file().is_file()
}

/// 跨进程「压测正在运行」标志：stress_test / stress_test_gpu 开始写、结束删。
/// 供另一个进程实例的 stress_cancel 准确判断「确实有压测在跑」——内存 RUNNING
/// flag 是进程内的，跨进程看不到；文件则谁都能读。
fn stress_running_file() -> std::path::PathBuf {
    std::env::temp_dir().join("diskpilot-stress-running.flag")
}

pub(crate) fn write_running_file() {
    let _ = std::fs::write(stress_running_file(), format!("pid={}", std::process::id()));
}

pub(crate) fn clear_running_file() {
    let _ = std::fs::remove_file(stress_running_file());
}

fn running_file_exists() -> bool {
    stress_running_file().is_file()
}

// ── 内部工具函数 ──────────────────────────────────────────────────────────

/// 选择线程数：默认=可用线程数，上限=线程数×2，下限 1。
pub(crate) fn pick_threads(threads: Option<usize>) -> usize {
    let available = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    threads
        .unwrap_or(available)
        .clamp(1, available.saturating_mul(2).max(1))
}

static CANCEL: AtomicBool = AtomicBool::new(false);
/// 是否有压测正在运行（stress_test / stress_test_gpu 开始置 true、结束置 false）。
/// 供 stress_cancel 判断「确实有压测在跑」，避免空操作误报「已发送停止信号」。
pub(crate) static RUNNING: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_cancel(v: bool) {
    CANCEL.store(v, Ordering::Relaxed);
}

/// 压测停止信号：置内存 CANCEL flag + 写跨进程停止文件（stress_cancel 工具 /
/// 前端「停止」按钮）。返回停止前是否确实有压测在跑：以跨进程 running 文件
/// 判定（压测开始写、结束删，任何进程实例都能读到；内存 RUNNING 是进程内的，
/// 跨进程场景不可靠——停止按钮走新 spawn 实例时它恒为 false）。无压测运行时
/// 是安全空操作。
pub fn stress_cancel() -> bool {
    let running = running_file_exists();
    set_cancel(true);
    write_stop_file();
    running
}

/// 统一判断停止信号：进程内 flag 或跨进程停止文件（后者解决「停止按钮走新
/// spawn 实例、CANCEL 是另一进程的 flag」问题）。
pub(crate) fn cancel_requested() -> bool {
    CANCEL.load(Ordering::Relaxed) || stop_file_exists()
}

/// 多线程跑基准直到 deadline，返回每线程的计数（调用方求和）。
/// 闭包签名：`(tid, deadline) -> 计数`；内部循环自己查 deadline 退出。
pub(crate) fn run_par_bench<F>(n: usize, secs: u64, f: F) -> Vec<u64>
where
    F: Fn(usize, Instant) -> u64 + Send + Sync + Clone + 'static,
{
    let deadline = Instant::now() + Duration::from_secs(secs);
    let barrier = Arc::new(Barrier::new(n));
    let mut handles = Vec::with_capacity(n);
    for tid in 0..n {
        let barrier = barrier.clone();
        let f = f.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait(); // 齐射，公平计时
            f(tid, deadline)
        }));
    }
    handles.into_iter().filter_map(|h| h.join().ok()).collect()
}

/// 带宽测量：重复跑 `f` 直到超时，返回 MB/s。
pub(crate) fn measure_bandwidth<F>(secs: u64, mut f: F, buf: &mut [u8]) -> u64
where
    F: FnMut(&mut [u8]) -> u64 + Send,
{
    let start = Instant::now();
    let mut bytes = 0u64;
    let mut acc = 0u64;
    while start.elapsed() < Duration::from_secs(secs) {
        acc ^= f(buf);
        bytes += buf.len() as u64;
    }
    let _ = acc;
    let secs_f = start.elapsed().as_secs_f64();
    if secs_f <= 0.0 {
        0
    } else {
        (bytes as f64 / secs_f / 1024.0 / 1024.0) as u64
    }
}

/// 内存延迟：单线程指针追逐（伪随机链表跳转）。
pub(crate) fn memory_latency(secs: u64) -> f64 {
    const N: usize = 1 << 20; // 4MB 指针数组
    let mut next: Vec<usize> = (1..N).collect();
    next.push(0); // 环形
                  // Fisher-Yates 洗牌打乱，形成伪随机跳转序列
    let mut rng = 0x9E3779B97F4A7C15u64;
    for _ in 0..2 {
        for i in (1..N).rev() {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = (rng as usize) % (i + 1);
            next.swap(i, j);
        }
    }
    let start = Instant::now();
    let mut hops = 0u64;
    let mut p = 0usize;
    while start.elapsed() < Duration::from_secs(secs) {
        p = next[p];
        hops += 1;
    }
    let secs_f = start.elapsed().as_secs_f64();
    if hops == 0 || secs_f <= 0.0 {
        0.0
    } else {
        secs_f * 1e9 / hops as f64
    }
}

/// 磁盘读基准：顺序/随机读，返回 MB/s（0 = 打开失败）。
pub(crate) fn read_bench(path: &std::path::Path, size: u64, random: bool, block: usize) -> u64 {
    let Ok(mut f) = std::fs::File::open(path) else {
        return 0;
    };
    let start = Instant::now();
    let mut buf = vec![0u8; block];
    let mut bytes = 0u64;
    let mut rng = 0x1234567890ABCDEFu64;
    while start.elapsed() < Duration::from_secs(2) {
        if random {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let pos = rng % size.saturating_sub(block as u64);
            let _ = f.seek(std::io::SeekFrom::Start(pos));
        }
        match f.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => bytes += n as u64,
        }
    }
    let secs_f = start.elapsed().as_secs_f64();
    if secs_f <= 0.0 {
        0
    } else {
        (bytes as f64 / secs_f / 1024.0 / 1024.0) as u64
    }
}

/// 磁盘写基准：顺序/随机写，返回 MB/s（0 = 打开失败）。
pub(crate) fn write_bench(path: &std::path::Path, size: u64, random: bool, block: usize) -> u64 {
    let Ok(mut f) = std::fs::File::create(path) else {
        return 0;
    };
    let start = Instant::now();
    let buf = vec![0x5Au8; block];
    let mut bytes = 0u64;
    let mut rng = 0x1234567890ABCDEFu64;
    while start.elapsed() < Duration::from_secs(2) {
        if random {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let pos = rng % size.saturating_sub(block as u64);
            let _ = f.seek(std::io::SeekFrom::Start(pos));
        }
        if f.write_all(&buf).is_err() {
            break;
        }
        bytes += block as u64;
    }
    let _ = f.sync_all();
    let secs_f = start.elapsed().as_secs_f64();
    if secs_f <= 0.0 {
        0
    } else {
        (bytes as f64 / secs_f / 1024.0 / 1024.0) as u64
    }
}

/// 温度探测：复用 hw::max_cpu_temp。
pub(crate) fn hw_probe_temp() -> Result<f64, String> {
    crate::tools::hw::max_cpu_temp()
}

/// 后台异步温度预取：fancmd 探测可能卡满 8s（非管理员/驱动加载慢），若在压测
/// 主线程同步跑，压测线程根本没启动、任务管理器看不到占用就「卡住」。
/// 后台异步温度预取槽：探测结果先写进共享槽（压测熔断循环可随时读），
/// 线程句柄用于压测结束时 join 拿最终读数。
pub(crate) type TempProbeSlot = Arc<std::sync::Mutex<Option<Result<f64, String>>>>;
pub(crate) type TempProbe = (std::thread::JoinHandle<Result<f64, String>>, TempProbeSlot);

/// 后台异步温度预取：fancmd 探测可能卡满 8s（非管理员/驱动加载慢），若在压测
/// 主线程同步跑，压测线程根本没启动、任务管理器看不到占用就「卡住」。
/// 用独立的探测器线程在压测跑的同时去拿温度：先给 3s 窗口抢出 pre_temp，
/// 抢不到就丢给后台继续跑——压测自身的熔断循环会在后续采样里补上温度读数。
pub(crate) fn probe_temp_async() -> TempProbe {
    let slot = Arc::new(std::sync::Mutex::new(None));
    let slot2 = Arc::clone(&slot);
    let handle = std::thread::spawn(move || {
        let r = hw_probe_temp();
        *slot2.lock().unwrap() = Some(r.clone());
        r
    });
    (handle, slot)
}

pub(crate) fn fastrand_hex() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{n:08x}")
}
