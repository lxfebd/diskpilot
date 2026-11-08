//! svc-hunter —— DiskPilot 官方插件（只读诊断工具，Windows）。
//!
//! 用法: svc-hunter.exe [--top N] [--all]
//!   默认只列可疑服务 TopN=10；--all 列全部服务状态摘要。
//!
//! 只读枚举：`sc query type= service state= all` 拿服务名+状态，
//! 再对每个服务 `sc qc <name>` 拿 BINARY_PATH_NAME。
//! 可疑判定：binpath 非空且不在系统目录（Windows/Program Files 等）——
//! 典型为装在 %TEMP% / 用户目录的服务。系统目录服务不标。
//! **不停服务、不改配置**，只出报告。
//!
//! 中文 Windows 下 sc 输出是 GBK，统一走 `cmd /c "chcp 65001 >nul & ..."` 切 UTF-8。

use std::env;
use std::process::Command;
use std::time::Instant;

/// 一个服务条目。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Service {
    name: String,
    display: String,
    state: String, // RUNNING / STOPPED / …
    binpath: String,
}

/// 解析 `sc query type= service state= all` 输出（可单测）。
/// 块以 SERVICE_NAME: 开头，字段行缩进含 KEY: 值；STATE 行形如 "4  RUNNING"。
fn parse_sc_query(out: &str) -> Vec<(String, String, String)> {
    let mut v = Vec::new();
    let mut name = String::new();
    let mut display = String::new();
    let mut state = String::new();
    let mut flush = |name: &mut String, display: &mut String, state: &mut String, v: &mut Vec<(String, String, String)>| {
        if !name.is_empty() {
            v.push((name.trim().to_string(), display.trim().to_string(), state.trim().to_string()));
        }
        name.clear();
        display.clear();
        state.clear();
    };
    for line in out.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(rest) = t.strip_prefix("SERVICE_NAME:") {
            flush(&mut name, &mut display, &mut state, &mut v);
            name = rest.trim().to_string();
        } else if let Some(rest) = t.strip_prefix("DISPLAY_NAME:") {
            display = rest.trim().to_string();
        } else if let Some(rest) = t.strip_prefix("STATE") {
            // "STATE              : 4  RUNNING" → 取冒号后，去掉前导状态码数字
            let after = rest.splitn(2, ':').nth(1).unwrap_or("").trim();
            state = after
                .split_whitespace()
                .filter(|w| !w.chars().all(|c| c.is_ascii_digit()))
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
    flush(&mut name, &mut display, &mut state, &mut v);
    v
}

/// 解析 `sc qc <name>` 输出取 BINARY_PATH_NAME（可单测）。
/// 注意路径里自带 `C:` 冒号——不能按第一个冒号切，要找字段名后的第一个 ':'。
fn parse_sc_qc_binpath(out: &str) -> String {
    for line in out.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("BINARY_PATH_NAME") {
            if let Some(pos) = rest.find(':') {
                let after = rest[pos + 1..].trim();
                if let Some(stripped) = after.strip_prefix('"') {
                    // 引号路径：取到下一个引号
                    if let Some(end) = stripped.find('"') {
                        return stripped[..end].to_string();
                    }
                }
                // 无引号：取到第一个空格（参数分隔）
                return after.split_whitespace().next().unwrap_or("").to_string();
            }
        }
    }
    String::new()
}

/// 路径是否"系统样"（不在系统目录 = 可疑候选）。空路径不算可疑（驱动服务等）。
fn is_systemish(p: &str) -> bool {
    let l = p.to_ascii_lowercase();
    if l.is_empty() {
        return true; // 空 binpath（内核驱动等）不参与可疑判定
    }
    l.contains("windows\\system32")
        || l.contains("windows\\syswow64")
        || l.contains("\\program files\\")
        || l.contains("\\program files (x86)\\")
        || l.starts_with("system32\\drivers")
        || l.contains("windows\\servicing")
        || l.contains("\\microsoft\\")
}

/// 可疑原因（纯函数，可单测）。
fn suspicious_reasons(svc: &Service) -> Vec<String> {
    let mut r = Vec::new();
    if svc.binpath.is_empty() {
        return r;
    }
    if !is_systemish(&svc.binpath) {
        // 拆出 exe 路径（可能带参数）
        let p = svc.binpath.trim_matches('"').split(' ').next().unwrap_or("");
        let low = p.to_ascii_lowercase();
        if low.contains("\\temp\\") || low.contains("\\tmp\\") || low.contains("\\appdata\\") || low.contains("\\users\\") {
            r.push(format!("二进制在临时/用户目录: {}", p));
        } else {
            r.push(format!("二进制不在系统目录: {}", p));
        }
    }
    if svc.state.eq_ignore_ascii_case("RUNNING") && !is_systemish(&svc.binpath) && !svc.binpath.is_empty() {
        r.push("且当前处于运行状态".to_string());
    }
    r
}

/// 统一走 cmd /c chcp 65001 切 UTF-8，避免中文系统 GBK 输出毁解析。
/// args 原样拼接（调用方需要引号的自己带，如服务名）。
fn run_utf8(prog: &str, args: &[&str]) -> Option<String> {
    let mut inner = String::from(prog);
    for a in args {
        inner.push(' ');
        inner.push_str(a);
    }
    let out = Command::new("cmd")
        .args(["/c", &format!("chcp 65001 >nul & {}", inner)])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("svc-hunter —— Windows 服务审计器（只读，不停服务不改配置）");
        println!("用法: svc-hunter.exe [--top N] [--all]");
        println!("  默认只输出可疑服务 TopN=10；--all 附加全部服务状态计数。");
        return;
    }
    let top_n: usize = args
        .iter()
        .position(|a| a == "--top")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10);
    let show_all = args.iter().any(|a| a == "--all");

    let t0 = Instant::now();
    // sc 的 "type= service" 是两个 token，不能给每个参数都加引号
    let Some(q) = run_utf8("sc", &["query", "type=", "service", "state=", "all"]) else {
        eprintln!("sc query 失败");
        std::process::exit(2);
    };
    let list = parse_sc_query(&q);
    let mut services: Vec<Service> = Vec::new();
    for (name, display, state) in &list {
        // 服务名可能含空格，单独加引号
        let qc = run_utf8("sc", &["qc", &format!("\"{}\"", name)]).unwrap_or_default();
        let binpath = parse_sc_qc_binpath(&qc);
        services.push(Service {
            name: name.clone(),
            display: display.clone(),
            state: state.clone(),
            binpath,
        });
    }
    let running = services.iter().filter(|s| s.state.eq_ignore_ascii_case("RUNNING")).count();
    let mut suspicious: Vec<(&Service, Vec<String>)> = services
        .iter()
        .map(|s| (s, suspicious_reasons(s)))
        .filter(|(_, r)| !r.is_empty())
        .collect();
    suspicious.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.name.cmp(&b.0.name)));

    println!(
        "服务审计（只读）: 共 {} 个服务，运行中 {} 个，可疑 {} 个，耗时 {:.2?}",
        services.len(),
        running,
        suspicious.len(),
        t0.elapsed()
    );
    if suspicious.is_empty() {
        println!("未发现二进制在临时/用户目录的可疑服务。");
    } else {
        println!("可疑服务 Top {}（共 {} 个）:", top_n, suspicious.len());
        for (s, reasons) in suspicious.iter().take(top_n) {
            println!("  {} ({}) [{}]", s.name, s.display, s.state);
            for r in reasons {
                println!("      - {}", r);
            }
        }
    }
    if show_all {
        println!("全部服务状态计数:");
        let mut counts = std::collections::HashMap::new();
        for s in &services {
            *counts.entry(s.state.clone()).or_insert(0) += 1;
        }
        let mut kv: Vec<_> = counts.into_iter().collect();
        kv.sort();
        for (k, n) in kv {
            println!("  {}: {}", if k.is_empty() { "?" } else { &k }, n);
        }
    }
    println!("提示: 本插件只读不停服务；停服务请人工确认后用 sc config 或 services.msc。");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sc_query_blocks() {
        let out = "SERVICE_NAME: Aaa\n        DISPLAY_NAME: Aaa Service\n        STATE              : 4  RUNNING\n\nSERVICE_NAME: Bbb\n        DISPLAY_NAME: Bbb\n        STATE              : 1  STOPPED\n";
        let v = parse_sc_query(out);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], ("Aaa".to_string(), "Aaa Service".to_string(), "RUNNING".to_string()));
        assert_eq!(v[1].2, "STOPPED");
    }

    #[test]
    fn parse_sc_qc_binpath_with_args() {
        let out = "[SC] QueryServiceConfig SUCCESS\n\nSERVICE_NAME: X\n        BINARY_PATH_NAME   : \"C:\\Temp\\evil.exe\" -k\n";
        assert_eq!(parse_sc_qc_binpath(out), "C:\\Temp\\evil.exe");
        assert_eq!(parse_sc_qc_binpath("nothing"), "");
    }

    #[test]
    fn suspicious_flags_temp_binpath() {
        let s = Service {
            name: "x".into(),
            display: "x".into(),
            state: "RUNNING".into(),
            binpath: "C:\\Users\\u\\AppData\\Local\\Temp\\svc.exe".into(),
        };
        let r = suspicious_reasons(&s);
        assert!(r.iter().any(|x| x.contains("临时/用户目录")));
        assert!(r.iter().any(|x| x.contains("运行状态")));
        // 系统目录服务不标
        let ok = Service { binpath: "C:\\Windows\\System32\\svc.exe".into(), ..s.clone() };
        assert!(suspicious_reasons(&ok).is_empty());
        // 空 binpath 不标
        let empty = Service { binpath: String::new(), ..s };
        assert!(suspicious_reasons(&empty).is_empty());
    }
}
