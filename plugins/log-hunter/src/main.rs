//! log-hunter —— DiskPilot 官方插件（只读诊断工具）。
//!
//! 用法: log-hunter.exe [目录] [最小MB] [TopN]
//!   默认: 目录=%USERPROFILE% 最小MB=10 TopN=20
//!
//! 扫描常见日志类扩展名（.log / .dmp / .wer / .tmp.log / .old 等）
//! 中大于阈值的文件，按大小列 TopN。只做扫描与报告，不删除、
//! 不修改任何文件。跳过符号链接防目录环。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// 命中的日志类扩展名（小写比较）。
const LOG_EXTS: &[&str] = &["log", "dmp", "wer", "hdmp", "mdmp", "trace", "etl"];

fn is_log_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // 逐层剥扩展名："app.log" / "app.log.3" / "app.20261101.log.gz" 任一层
    // 命中日志扩展名即算（轮转 + 压缩形态都覆盖）。
    let mut cur: &str = &lower;
    loop {
        let Some((stem, ext)) = cur.rsplit_once('.') else {
            return false;
        };
        if LOG_EXTS.contains(&ext) {
            return true;
        }
        // 尾段全是数字（轮转序号 "app.log.3"）才继续往里剥，避免无限循环。
        if ext.chars().all(|c| c.is_ascii_digit()) && !stem.is_empty() {
            cur = stem;
        } else {
            // 压缩后缀（gz/zip/7z/xz）也剥掉再看一层
            if matches!(ext, "gz" | "zip" | "7z" | "xz" | "zst") && !stem.is_empty() {
                cur = stem;
            } else {
                return false;
            }
        }
    }
}

fn walk(dir: &Path, out: &mut Vec<(u64, PathBuf)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(m) = p.symlink_metadata() else { continue };
        let ft = m.file_type();
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            walk(&p, out);
        } else if ft.is_file() {
            let name = e.file_name().to_string_lossy().to_string();
            if is_log_name(&name) {
                out.push((m.len(), p));
            }
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
        println!("log-hunter —— 大日志文件扫描器（只读，不做任何删除）");
        println!("用法: log-hunter.exe [目录] [最小MB] [TopN]");
        println!("  目录    扫描根目录（默认 %USERPROFILE%）");
        println!("  最小MB  只报告大于该值的日志（默认 10）");
        println!("  TopN    最多显示前 N 个（默认 20）");
        return;
    }
    let root = args
        .first()
        .cloned()
        .or_else(|| env::var_os("USERPROFILE").map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| ".".to_string());
    let min_mb: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let top_n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);

    let root_p = Path::new(&root);
    if !root_p.is_dir() {
        eprintln!("错误: 「{root}」不是目录");
        std::process::exit(2);
    }
    println!("扫描目录: {}", root_p.display());
    println!("日志扩展名: {}", LOG_EXTS.join(", "));
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
        "扫描完成: {scanned} 个日志文件，合计 {}，耗时 {:.1}s",
        fmt_size(total),
        t0.elapsed().as_secs_f64()
    );
    if files.is_empty() {
        println!("没有大于 {min_mb} MB 的日志文件。");
        return;
    }
    println!("\n最大的 {} 个日志文件:", files.len());
    for (i, (size, p)) in files.iter().enumerate() {
        println!("{:>3}. {:>10}  {}", i + 1, fmt_size(*size), p.display());
    }
    println!("\n提示: 本工具只读不删。崩溃转储等有专门的 DiskPilot 清理脚本，请在主程序里走「确认后删除」流程。");
}

#[cfg(test)]
mod tests {
    use super::is_log_name;

    #[test]
    fn matches_common_log_exts() {
        assert!(is_log_name("app.log"));
        assert!(is_log_name("crash.DMP"));
        assert!(is_log_name("WERReport.wer"));
        assert!(is_log_name("setup.etl"));
    }

    #[test]
    fn matches_rotated_logs() {
        assert!(is_log_name("app.log.3"));
        assert!(is_log_name("app.20261101.log.gz"));
    }

    #[test]
    fn rejects_non_logs() {
        assert!(!is_log_name("photo.jpg"));
        assert!(!is_log_name("chat.db"));
        assert!(!is_log_name("noext"));
    }
}
