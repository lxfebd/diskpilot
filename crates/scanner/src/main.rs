//! 扫描性能冒烟 CLI：`cargo run -p diskpilot-scanner --features bench -- bench C`
//! 内存吞吐：`cargo run -p diskpilot-scanner --features bench -- mem [条数]`
//!
//! Windows 专用（mft.rs 仅在 cfg(windows) 编译）。
//! - `bench C`：管理员权限打开卷设备，对同一卷先后跑快路径（整块读 $MFT +
//!   内存解析）与慢路径（逐条 ntfs::file()），输出各自耗时与树根统计。
//! - `mem [N]`：无需管理员，对 N 条合成 FILE 记录跑 fixup_all+parse_record
//!   热循环，测纯 CPU 吞吐。

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("用法:");
        eprintln!("  diskpilot-scanner bench <盘符>   真实卷快/慢路径对比（需管理员）");
        eprintln!("  diskpilot-scanner mem [条数]      内存吞吐基准（默认 1000000 条）");
        std::process::exit(2);
    }
    let sub = args[1].as_str();

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
