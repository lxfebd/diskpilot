//! bigfile-finder —— DiskPilot 官方插件（只读诊断工具）。
//!
//! 用法: bigfile-finder.exe <目录> [最小MB] [TopN]
//!   默认: 目录=当前目录 最小MB=100 TopN=20
//!
//! 只做扫描与报告，不删除、不修改任何文件。跳过符号链接防目录环。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn walk(dir: &Path, out: &mut Vec<(u64, PathBuf)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        // symlink_metadata 不跟随链接：目录环 / 链到外盘都跳过。
        let Ok(m) = p.symlink_metadata() else { continue };
        let ft = m.file_type();
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            walk(&p, out);
        } else if ft.is_file() {
            out.push((m.len(), p));
        }
    }
}

fn fmt_size(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if n >= GB {
        format!("{:.2} GB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{:.2} MB", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.1} KB", n as f64 / KB as f64)
    } else {
        format!("{n} B")
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("bigfile-finder —— 大文件扫描器（只读，不做任何删除）");
        println!("用法: bigfile-finder.exe <目录> [最小MB] [TopN]");
        println!("  目录    要扫描的根目录（默认当前目录）");
        println!("  最小MB  只报告大于该值的文件（默认 100）");
        println!("  TopN    最多显示前 N 个（默认 20）");
        return;
    }
    let root = args.first().cloned().unwrap_or_else(|| ".".to_string());
    let min_mb: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(100);
    let top_n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);

    let root_p = Path::new(&root);
    if !root_p.is_dir() {
        eprintln!("错误: 「{root}」不是目录");
        std::process::exit(2);
    }
    println!("扫描目录: {}", root_p.display());
    println!("阈值: >= {min_mb} MB，Top {top_n}");
    let t0 = Instant::now();
    let mut files: Vec<(u64, PathBuf)> = Vec::new();
    walk(root_p, &mut files);
    let scanned = files.len();
    let total: u64 = files.iter().map(|(s, _)| s).sum();
    files.retain(|(s, _)| *s >= min_mb * 1024 * 1024);
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.truncate(top_n);
    println!(
        "扫描完成: {scanned} 个文件，合计 {}，耗时 {:.1}s",
        fmt_size(total),
        t0.elapsed().as_secs_f64()
    );
    if files.is_empty() {
        println!("没有大于 {min_mb} MB 的文件。");
        return;
    }
    println!("\n最大的 {} 个文件:", files.len());
    for (i, (size, p)) in files.iter().enumerate() {
        println!("{:>3}. {:>10}  {}", i + 1, fmt_size(*size), p.display());
    }
    println!("\n提示: 本工具只读不删。如需清理，请在 DiskPilot 主程序里走「确认后删除」流程。");
}
