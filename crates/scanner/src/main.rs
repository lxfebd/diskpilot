//! 扫描性能冒烟 CLI：`cargo run -p diskpilot-scanner --features bench -- bench C`
//! 内存吞吐：`cargo run -p diskpilot-scanner --features bench -- mem [条数]`
//! walkdir 压测（跨平台）：`cargo run -p diskpilot-scanner --features bench -- walk <目录> [重复次数]`
//!
//! - `walk <dir> [N]`：R511 性能基线。对目录跑 N 次 scan_with_stats（默认 3 次，
//!   报最佳与均值），输出 files/s、GB/s 与 phase 分解（walk / build_tree / total），
//!   用于回归对比扫描优化效果。跨平台可用。
//! - `bench C` / `mem [N]`：Windows 专用（mft.rs 仅在 cfg(windows) 编译）。
//!   `bench C` 管理员权限打开卷设备，快/慢路径对比；`mem` 测 MFT 解析纯 CPU 吞吐。

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("用法:");
        eprintln!("  diskpilot-scanner walk <目录> [重复次数]  walkdir 压测（跨平台，默认 3 次）");
        eprintln!("  diskpilot-scanner bench <盘符>   真实卷快/慢路径对比（需管理员，Windows）");
        eprintln!("  diskpilot-scanner mem [条数]      内存吞吐基准（默认 1000000 条，Windows）");
        std::process::exit(2);
    }
    let sub = args[1].as_str();

    // walk 子命令跨平台：不依赖 cfg(windows) 的 mft 模块。
    if sub == "walk" {
        let dir = args
            .get(2)
            .expect("walk 需要目录参数（例如 C:\\\\Windows）");
        let reps: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3);
        let reps = reps.max(1);
        eprintln!("walk 压测: {dir} × {reps} 次（报最佳/均值）");
        let mut best: Option<std::time::Duration> = None;
        let mut total = std::time::Duration::ZERO;
        let mut last_stats = None;
        for i in 0..reps {
            let t0 = std::time::Instant::now();
            let (_tree, stats) =
                diskpilot_scanner::scan_with_stats(dir, Default::default(), |_| {}).unwrap_or_else(
                    |e| {
                        eprintln!("扫描失败: {e:#}");
                        std::process::exit(1);
                    },
                );
            let wall = t0.elapsed();
            best = Some(best.map_or(wall, |b| b.min(wall)));
            total += wall;
            let fs = stats.files_seen as f64;
            let gs = stats.bytes_seen as f64 / 1e9;
            eprintln!(
                "  run{}: files={} bytes={:.2}GB wall={:.2}s | {:.0} files/s {:.2} GB/s | walk={}ms tree={}ms total={}ms mode={}",
                i + 1,
                stats.files_seen,
                gs,
                wall.as_secs_f64(),
                fs / wall.as_secs_f64().max(1e-9),
                gs / wall.as_secs_f64().max(1e-9),
                stats.walk_ms,
                stats.build_tree_ms,
                stats.total_ms,
                stats.mode
            );
            last_stats = Some(stats);
        }
        if let (Some(b), Some(s)) = (best, last_stats) {
            let fs = s.files_seen as f64;
            let gs = s.bytes_seen as f64 / 1e9;
            println!(
                "BEST wall={:.2}s files/s={:.0} GB/s={:.2} (files={} bytes={} mode={}) AVG wall={:.2}s",
                b.as_secs_f64(),
                fs / b.as_secs_f64().max(1e-9),
                gs / b.as_secs_f64().max(1e-9),
                s.files_seen,
                s.bytes_seen,
                s.mode,
                (total / reps as u32).as_secs_f64()
            );
        }
        return;
    }

    #[cfg(windows)]
    {
        match sub {
            "bench" => {
                let letter = args
                    .get(2)
                    .and_then(|s| s.chars().next())
                    .expect("bench 需要盘符（例如 C）")
                    .to_ascii_uppercase();
                match diskpilot_scanner::mft_bench::bench::run(letter) {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("基准失败: {:#}", e);
                        std::process::exit(1);
                    }
                }
            }
            "mem" => {
                let n = args
                    .get(2)
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(1_000_000);
                match diskpilot_scanner::mft_bench::bench::run_mem(n) {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("内存基准失败: {:#}", e);
                        std::process::exit(1);
                    }
                }
            }
            _ => {
                eprintln!("未知子命令: {sub}");
                std::process::exit(2);
            }
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("bench/mem 仅在 Windows 上可用（需打开 NTFS 卷设备/合成记录布局）");
        std::process::exit(2);
    }
}
