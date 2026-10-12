//! 基准主函数域（从 bench.rs 拆分，2026-10-12）。
//! bench_cpu / bench_memory / bench_disk；稳定性压测域见 `bench_stress`。

use std::io::Write;
use std::time::Instant;

use crate::tools::files::PathGuard;

use super::bench_util::{
    fastrand_hex, measure_bandwidth, memory_latency, pick_threads, read_bench, run_par_bench,
    write_bench,
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
    progress: Option<std::sync::Arc<dyn Fn(u64) + Send + Sync>>,
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

    // 建读基准文件（固定图案）。进度：0→创建文件（10%）→四项基准各 ~20%。
    let mut f =
        std::fs::File::create(&read_path).map_err(|e| format!("创建读基准文件失败：{e}"))?;
    let pattern = vec![0x5Au8; 64 * 1024];
    let mut written = 0u64;
    while written < size_bytes {
        let n = pattern.len().min((size_bytes - written) as usize);
        f.write_all(&pattern[..n])
            .map_err(|e| format!("写读基准文件失败：{e}"))?;
        written += n as u64;
        if let Some(cb) = progress.as_deref() {
            let pct = 100 + written * 50 / size_bytes; // 10%..=60%
            cb(pct);
        }
    }
    f.sync_all().ok();

    let seq_read = if let Some(cb) = progress.as_deref() {
        cb(600); // 顺序读（60%）
        read_bench(&read_path, size_bytes, false, 128 * 1024)
    } else {
        read_bench(&read_path, size_bytes, false, 128 * 1024)
    };
    let rand_read = if let Some(cb) = progress.as_deref() {
        cb(700); // 随机读（70%）
        read_bench(&read_path, size_bytes, true, 4 * 1024)
    } else {
        read_bench(&read_path, size_bytes, true, 4 * 1024)
    };

    // 写基准（顺序/随机写同一个临时文件）
    let seq_write = if let Some(cb) = progress.as_deref() {
        cb(800); // 顺序写（80%）
        write_bench(&write_path, size_bytes, false, 128 * 1024)
    } else {
        write_bench(&write_path, size_bytes, false, 128 * 1024)
    };
    let rand_write = if let Some(cb) = progress.as_deref() {
        cb(900); // 随机写（90%）
        write_bench(&write_path, size_bytes, true, 4 * 1024)
    } else {
        write_bench(&write_path, size_bytes, true, 4 * 1024)
    };

    // 清理
    let _ = std::fs::remove_file(&read_path);
    let _ = std::fs::remove_file(&write_path);
    if let Some(cb) = progress.as_deref() {
        cb(1000); // 完成
    }

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
