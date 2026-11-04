//! dup-finder —— DiskPilot 官方插件（只读诊断工具）。
//!
//! 用法: dup-finder.exe <目录> [最小KB]
//!   默认: 目录=当前目录 最小KB=64
//!
//! 判定: 文件大小相同 且 全量内容哈希（SipHash via DefaultHasher）相同 → 归为
//! 「疑似重复」组。哈希理论上存在碰撞，因此输出标注「疑似」，最终由人确认。
//! 只读：不删除、不移动任何文件。跳过符号链接。

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

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
            out.push((m.len(), p));
        }
    }
}

/// 全量读文件算哈希。读失败（权限等）返回 None，该文件跳过比对。
fn content_hash(p: &Path) -> Option<u64> {
    let mut f = fs::File::open(p).ok()?;
    let mut h = DefaultHasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => buf[..n].hash(&mut h),
            Err(_) => return None,
        }
    }
    Some(h.finish())
}

fn fmt_size(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if n >= GB {
        format!("{:.2} GB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{:.2} MB", n as f64 / MB as f64)
    } else {
        format!("{:.1} KB", n as f64 / KB as f64)
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("dup-finder —— 疑似重复文件查找（只读，不做任何删除）");
        println!("用法: dup-finder.exe <目录> [最小KB]");
        println!("  大小相同且内容哈希相同的文件归为一组，标注「疑似重复」。");
        println!("  最小KB 默认 64（太小的文件不比对，避免海量小文件拖慢）。");
        return;
    }
    let root = args.first().cloned().unwrap_or_else(|| ".".to_string());
    let min_kb: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);
    let root_p = Path::new(&root);
    if !root_p.is_dir() {
        eprintln!("错误: 「{root}」不是目录");
        std::process::exit(2);
    }
    let t0 = Instant::now();
    let mut files: Vec<(u64, PathBuf)> = Vec::new();
    walk(root_p, &mut files);
    let scanned = files.len();
    println!("扫描 {scanned} 个文件（>= {min_kb} KB 参与比对）...");

    // 按大小分组，只处理组内 >=2 的
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for (s, p) in files {
        if s >= min_kb * 1024 {
            by_size.entry(s).or_default().push(p);
        }
    }
    let mut groups: Vec<(u64, Vec<PathBuf>)> = Vec::new();
    let mut n = 0usize;
    for (size, paths) in by_size {
        if paths.len() < 2 {
            continue;
        }
        // 组内按内容哈希再分
        let mut by_hash: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        for p in paths {
            n += 1;
            if n % 200 == 0 {
                println!("  已哈希 {n} 个候选文件...");
            }
            if let Some(h) = content_hash(&p) {
                by_hash.entry(h).or_default().push(p);
            }
        }
        for (_, mut ps) in by_hash {
            if ps.len() >= 2 {
                ps.sort();
                groups.push((size, ps));
            }
        }
    }
    groups.sort_by(|a, b| (b.0 * b.1.len() as u64).cmp(&(a.0 * a.1.len() as u64)));

    println!(
        "\n完成: 扫描 {scanned} 个，发现 {} 组疑似重复，耗时 {:.1}s",
        groups.len(),
        t0.elapsed().as_secs_f64()
    );
    if groups.is_empty() {
        println!("没有发现疑似重复文件。");
        return;
    }
    let mut wasted = 0u64;
    for (i, (size, ps)) in groups.iter().enumerate() {
        let dup = ps.len() - 1;
        wasted += size * dup as u64;
        println!(
            "\n组 {}: {} 个文件各 {}（可回收约 {}）",
            i + 1,
            ps.len(),
            fmt_size(*size),
            fmt_size(size * dup as u64)
        );
        for p in ps {
            println!("    {}", p.display());
        }
    }
    println!(
        "\n合计疑似可回收: {}（哈希碰撞理论存在，标注「疑似」——请人工确认后再删）。",
        fmt_size(wasted)
    );
    println!("提示: 本工具只读不删。删除请走 DiskPilot 主程序的确认流程。");
}
