//! temp-hunter —— DiskPilot 官方插件（只读诊断工具）。
//!
//! 用法: temp-hunter.exe [TopN]
//!   默认 TopN=20
//!
//! 扫描用户 %TEMP% 与系统 C:\Windows\Temp，按文件年龄分桶统计
//! （>30 天 / 7–30 天 / <7 天），并列出最大的 TopN 个临时文件。
//! 只做扫描与报告，不删除、不修改任何文件。跳过符号链接防目录环。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

/// 深度上限：临时目录常见多层嵌套，超过 8 层的子树跳过（报告里说明）。
const MAX_DEPTH: usize = 8;
/// 文件数上限：防止超大 Temp 卡死扫描，超出即提前收尾。
const MAX_FILES: usize = 200_000;

fn walk(dir: &Path, depth: usize, out: &mut Vec<(u64, SystemTime, PathBuf)>, truncated: &mut bool) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        *truncated = true;
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if out.len() >= MAX_FILES {
            *truncated = true;
            return;
        }
        let p = e.path();
        // symlink_metadata 不跟随链接：目录环 / 链到外盘都跳过。
        let Ok(m) = p.symlink_metadata() else { continue };
        let ft = m.file_type();
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            walk(&p, depth + 1, out, truncated);
        } else if ft.is_file() {
            let mtime = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            out.push((m.len(), mtime, p));
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

fn age_days(mtime: SystemTime, now: SystemTime) -> u64 {
    now.duration_since(mtime)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("temp-hunter —— 临时目录扫描器（只读，不做任何删除）");
        println!("用法: temp-hunter.exe [TopN]");
        println!("  扫描 %TEMP% 与 %SystemRoot%\\Temp，按年龄分桶统计并列出最大文件。");
        return;
    }
    let top_n: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(20);

    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(t) = env::var_os("TEMP").or_else(|| env::var_os("TMP")) {
        roots.push(PathBuf::from(t));
    }
    if let Some(w) = env::var_os("SystemRoot") {
        roots.push(PathBuf::from(w).join("Temp"));
    }
    if roots.is_empty() {
        eprintln!("错误: 找不到任何临时目录（TEMP/TMP/SystemRoot 均未设置）");
        std::process::exit(2);
    }

    let now = SystemTime::now();
    let t0 = Instant::now();
    let mut files: Vec<(u64, SystemTime, PathBuf)> = Vec::new();
    let mut truncated = false;
    for r in &roots {
        println!("扫描: {}", r.display());
        walk(r, 0, &mut files, &mut truncated);
    }
    let scanned = files.len();
    if truncated {
        println!("(已达深度 {MAX_DEPTH} 层或 {MAX_FILES} 文件上限，扫描提前收尾——报告为下界)");
    }
    let total: u64 = files.iter().map(|(s, _, _)| s).sum();

    // 按年龄分桶
    let mut old30 = (0u64, 0usize); // (bytes, count) >30 天
    let mut mid7 = (0u64, 0usize); // 7-30 天
    let mut new7 = (0u64, 0usize); // <7 天
    for (s, m, _) in &files {
        match age_days(*m, now) {
            d if d > 30 => {
                old30.0 += s;
                old30.1 += 1;
            }
            d if d >= 7 => {
                mid7.0 += s;
                mid7.1 += 1;
            }
            _ => {
                new7.0 += s;
                new7.1 += 1;
            }
        }
    }

    println!(
        "\n扫描完成: {scanned} 个文件，合计 {}，耗时 {:.1}s",
        fmt_size(total),
        t0.elapsed().as_secs_f64()
    );
    println!("按年龄分桶（临时文件通常越老越可清）:");
    println!(
        "  >30 天: {:>4} 个文件，合计 {:>10}",
        old30.1,
        fmt_size(old30.0)
    );
    println!(
        "  7-30 天: {:>4} 个文件，合计 {:>10}",
        mid7.1,
        fmt_size(mid7.0)
    );
    println!(
        "  <7 天:   {:>4} 个文件，合计 {:>10}",
        new7.1,
        fmt_size(new7.0)
    );

    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.truncate(top_n);
    if files.is_empty() {
        println!("\n临时目录为空。");
    } else {
        println!("\n最大的 {} 个临时文件:", files.len());
        for (i, (size, m, p)) in files.iter().enumerate() {
            println!(
                "{:>3}. {:>10}  {:>3}d  {}",
                i + 1,
                fmt_size(*size),
                age_days(*m, now),
                p.display()
            );
        }
    }
    println!("\n提示: 本工具只读不删。如需清理，请在 DiskPilot 主程序里走「确认后删除」流程（系统临时 scope）。");
}
