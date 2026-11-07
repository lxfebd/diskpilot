//! browser-cache-hunter —— DiskPilot 官方插件（只读诊断工具，跨平台可用，Windows 最全）。
//!
//! 用法: browser-cache-hunter.exe [--top N]
//!
//! 只读审计主流浏览器的缓存目录体积（不删除任何文件）：
//!   Chrome / Edge / Brave / Opera / Vivaldi：%LOCALAPPDATA%/<厂商>/User Data/<profile>/ 下的
//!     Cache、Code Cache、GPUCache、Media Cache、Service Worker/CacheStorage、grader 等；
//!   Firefox：%APPDATA%/Mozilla/Firefox/Profiles/<profile>/cache2 + startupCache。
//! 每个 profile 汇总总字节 + Top N 子目录；末尾给出「关闭浏览器后可安全清理」的估算。
//! 只做目录体积统计，**不删文件、不改配置、不杀浏览器进程**。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// 一个浏览器的缓存根候选：(浏览器名, profile 目录, 该 profile 下要审计的子目录名)。
struct CacheSite {
    browser: &'static str,
    profile_dir: PathBuf,
    subdirs: Vec<&'static str>,
}

fn local_appdata() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA").map(PathBuf::from)
}
fn roaming_appdata() -> Option<PathBuf> {
    env::var_os("APPDATA").map(PathBuf::from)
}

/// 枚举 Chrome 系浏览器的全部 profile（User Data 下除 Default 外还有 Profile *）。
fn chrome_family_sites(browser: &'static str, user_data: PathBuf) -> Vec<CacheSite> {
    let mut sites = Vec::new();
    let entries = match fs::read_dir(&user_data) {
        Ok(e) => e,
        Err(_) => return sites,
    };
    let mut profiles: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            p.is_dir() && (name == "Default" || name.starts_with("Profile "))
        })
        .collect();
    profiles.sort();
    for p in profiles {
        sites.push(CacheSite {
            browser,
            profile_dir: p,
            subdirs: vec![
                "Cache",
                "Code Cache",
                "GPUCache",
                "Media Cache",
                "Service Worker/CacheStorage",
                "Shared Dictionary Cache",
                " DawnGraphiteCache",
                "DawnWebGPUCache",
            ],
        });
    }
    sites
}

fn firefox_sites() -> Vec<CacheSite> {
    let mut sites = Vec::new();
    let Some(roaming) = roaming_appdata() else { return sites };
    let profiles_dir = roaming.join("Mozilla").join("Firefox").join("Profiles");
    if let Ok(entries) = fs::read_dir(&profiles_dir) {
        let mut ps: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        ps.sort();
        for p in ps {
            sites.push(CacheSite {
                browser: "Firefox",
                profile_dir: p,
                subdirs: vec!["cache2", "startupCache", "jumpListCache"],
            });
        }
    }
    sites
}

/// 只读递归求目录字节数，带深度/条目上限防卡死（符号链接不跟随）。
fn dir_size(p: &Path, depth: usize, budget: &mut usize) -> u64 {
    if depth > 20 || *budget == 0 {
        return 0;
    }
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(p) else { return 0 };
    for e in entries.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let ft = match e.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_symlink() {
            continue;
        }
        let path = e.path();
        if ft.is_dir() {
            total += dir_size(&path, depth + 1, budget);
        } else if ft.is_file() {
            total += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

fn fmt_size(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;
    if n >= TB {
        format!("{:.2} TB", n as f64 / TB as f64)
    } else if n >= GB {
        format!("{:.2} GB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{:.1} MB", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.1} KB", n as f64 / KB as f64)
    } else {
        format!("{n} B")
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("browser-cache-hunter —— 浏览器缓存审计器（只读，不删文件不杀进程）");
        println!("用法: browser-cache-hunter.exe [--top N]");
        println!("  审计 Chrome/Edge/Brave/Opera/Vivaldi/Firefox 各 profile 的缓存目录体积，");
        println!("  输出每 profile 总量 + Top N 子目录。--top 默认 5。");
        return;
    }
    let mut top_n = 5usize;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--top" {
            if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<usize>().ok()) {
                top_n = v.max(1);
            }
            i += 2;
        } else {
            i += 1;
        }
    }

    let t0 = Instant::now();
    let mut sites: Vec<CacheSite> = Vec::new();
    if let Some(local) = local_appdata() {
        sites.extend(chrome_family_sites("Chrome", local.join("Google").join("Chrome").join("User Data")));
        sites.extend(chrome_family_sites("Edge", local.join("Microsoft").join("Edge").join("User Data")));
        sites.extend(chrome_family_sites("Brave", local.join("BraveSoftware").join("Brave-Browser").join("User Data")));
        sites.extend(chrome_family_sites("Opera", local.join("Opera Software").join("Opera Stable")));
        sites.extend(chrome_family_sites("Vivaldi", local.join("Vivaldi").join("User Data")));
    }
    sites.extend(firefox_sites());

    println!("浏览器缓存审计（只读）: {} 个 profile 候选，耗时 {:.2}s", sites.len(), 0.0);
    let mut grand_total = 0u64;
    let mut any = false;
    for site in &sites {
        let profile_name = site
            .profile_dir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut per_sub: Vec<(String, u64)> = Vec::new();
        for sub in &site.subdirs {
            let dir = site.profile_dir.join(sub.trim_start_matches(' '));
            if !dir.is_dir() {
                continue;
            }
            let mut budget = 2_000_000usize; // 单目录条目上限，防异常目录卡死
            let sz = dir_size(&dir, 0, &mut budget);
            if sz > 0 {
                per_sub.push((sub.trim_start_matches(' ').to_string(), sz));
            }
        }
        if per_sub.is_empty() {
            continue;
        }
        any = true;
        per_sub.sort_by_key(|e| std::cmp::Reverse(e.1));
        let total: u64 = per_sub.iter().map(|e| e.1).sum();
        grand_total += total;
        println!(
            "\n{} [{}]: 共 {}（{} 项缓存子目录）",
            site.browser,
            profile_name,
            fmt_size(total),
            per_sub.len()
        );
        for (name, sz) in per_sub.iter().take(top_n) {
            println!("  {:>28}  {}", name, fmt_size(*sz));
        }
    }
    println!("\n汇总: 全部浏览器缓存共 {}，统计耗时 {:.2}s", fmt_size(grand_total), t0.elapsed().as_secs_f64());
    if any {
        println!("提示: 缓存目录关闭浏览器后可安全清理（主程序走「确认后执行」链路，本插件只读不删）。");
    } else {
        println!("(未发现任何已知浏览器的缓存目录——可能未安装或路径非标准)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_size_sums_files_and_skips_missing() {
        let tmp = std::env::temp_dir().join(format!("bch-test-{}", std::process::id()));
        fs::create_dir_all(tmp.join("a/b")).unwrap();
        fs::write(tmp.join("f1.bin"), vec![0u8; 100]).unwrap();
        fs::write(tmp.join("a/f2.bin"), vec![0u8; 200]).unwrap();
        fs::write(tmp.join("a/b/f3.bin"), vec![0u8; 50]).unwrap();
        let mut budget = 10_000;
        let sz = dir_size(&tmp, 0, &mut budget);
        assert_eq!(sz, 350);
        assert_eq!(dir_size(Path::new("不存在的目录xyz"), 0, &mut budget), 0);
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn dir_size_respects_budget() {
        let tmp = std::env::temp_dir().join(format!("bch-budget-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        for i in 0..100 {
            fs::write(tmp.join(format!("f{i}")), vec![0u8; 10]).unwrap();
        }
        let mut budget = 5;
        let sz = dir_size(&tmp, 0, &mut budget);
        assert!(sz <= 50, "预算耗尽应停止遍历: {sz}");
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn fmt_size_units() {
        assert_eq!(fmt_size(512), "512 B");
        assert_eq!(fmt_size(1024), "1.0 KB");
        assert_eq!(fmt_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(fmt_size(3 * 1024u64.pow(3)), "3.00 GB");
    }
}
