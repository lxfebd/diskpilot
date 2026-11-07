//! port-hunter —— DiskPilot 官方插件（只读诊断工具，Windows）。
//!
//! 用法: port-hunter.exe [TopN] [--proto tcp|udp|all] [--listen]
//!   默认 TopN=30, --proto all, 不带 --listen（列出全部连接态）
//!
//! 解析 `netstat -ano` 拿到 (协议/本地地址/外端/状态/PID)，再用
//! `tasklist /FO CSV /NH` 批量把 PID 映射成进程名，按本地端口排序输出
//! TopN 占用表 + 摘要统计。只做查询与报告，**不杀进程、不改任何配置**。

use std::collections::HashMap;
use std::env;
use std::process::Command;
use std::time::Instant;

/// 一条 netstat 记录。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Conn {
    proto: String,       // TCP / UDP
    local: String,       // 0.0.0.0:8080 / [::]:443
    foreign: String,     // 远端地址
    state: String,       // LISTENING / ESTABLISHED / ...（UDP 为空）
    pid: u32,
}

/// 从 netstat -ano 原始输出解析连接表（可单测）。
fn parse_netstat(out: &str) -> Vec<Conn> {
    let mut v = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        // 形如:  TCP    0.0.0.0:135    0.0.0.0:0    LISTENING    1234
        //       UDP    0.0.0.0:5353   *:*                   5678
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 {
            continue;
        }
        let proto = f[0].to_ascii_uppercase();
        if proto != "TCP" && proto != "UDP" {
            continue;
        }
        let local = f[1].to_string();
        let foreign = f[2].to_string();
        let (state, pid) = if proto == "TCP" {
            if f.len() < 5 {
                continue;
            }
            (f[3].to_string(), f[4].parse::<u32>().unwrap_or(0))
        } else {
            // UDP 行没有 state 列，PID 在第 4 列
            (String::new(), f[3].parse::<u32>().unwrap_or(0))
        };
        v.push(Conn {
            proto,
            local,
            foreign,
            state,
            pid,
        });
    }
    v
}

/// 从 `tasklist /FO CSV /NH` 输出解析 PID → 进程名（可单测）。
/// CSV 行: "explorer.exe","1234","Console","1","50,000 K"
fn parse_tasklist(out: &str) -> HashMap<u32, String> {
    let mut m = HashMap::new();
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // 按引号分段: "name","pid",...
        let mut parts = line.split("\",\"");
        let name = parts.next().unwrap_or("").trim_matches('"');
        let pid_s = parts.next().unwrap_or("").trim_matches('"');
        if let Ok(pid) = pid_s.parse::<u32>() {
            m.insert(pid, name.to_string());
        }
    }
    m
}

/// 取本地地址里的端口号（冒号最后一段；IPv6 形如 [::]:443）。
fn local_port(local: &str) -> u16 {
    local.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0)
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    // 系统中文环境下 netstat/tasklist 输出是 GBK，lossy 兜底解码。
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("port-hunter —— 端口占用审计器（只读，不杀进程不改配置）");
        println!("用法: port-hunter.exe [TopN] [--proto tcp|udp|all] [--listen]");
        println!("  默认 TopN=30，--proto all；--listen 只列监听态（服务端口）。");
        return;
    }
    let top_n: usize = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let proto_filter = args
        .iter()
        .position(|a| a == "--proto")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "all".to_string());
    let listen_only = args.iter().any(|a| a == "--listen");

    let t0 = Instant::now();
    let netstat = run("netstat", &["-ano"]).unwrap_or_else(|| {
        eprintln!("错误: 无法运行 netstat -ano（需要 Windows）");
        std::process::exit(2);
    });
    let mut conns = parse_netstat(&netstat);
    if proto_filter == "tcp" {
        conns.retain(|c| c.proto == "TCP");
    } else if proto_filter == "udp" {
        conns.retain(|c| c.proto == "UDP");
    }
    if listen_only {
        conns.retain(|c| c.state.eq_ignore_ascii_case("LISTENING"));
    }

    // PID → 进程名（批量一次 tasklist，比逐 PID 查询快几个量级）。
    let tasklist = run("tasklist", &["/FO", "CSV", "/NH"]).unwrap_or_default();
    let pid_name = parse_tasklist(&tasklist);

    // 摘要
    let total = conns.len();
    let listening = conns.iter().filter(|c| c.state.eq_ignore_ascii_case("LISTENING")).count();
    let tcp_n = conns.iter().filter(|c| c.proto == "TCP").count();
    let udp_n = conns.iter().filter(|c| c.proto == "UDP").count();
    let distinct_pids = {
        let mut s: Vec<u32> = conns.iter().map(|c| c.pid).collect();
        s.sort_unstable();
        s.dedup();
        s.len()
    };
    println!(
        "端口占用审计: 共 {total} 条记录（TCP {tcp_n} / UDP {udp_n}），LISTENING {listening}，涉及 {distinct_pids} 个进程，耗时 {:.2}s",
        t0.elapsed().as_secs_f64()
    );
    if listen_only {
        println!("(已过滤: 仅 LISTENING 态)");
    }
    if proto_filter != "all" {
        println!("(已过滤: 仅 {proto_filter})");
    }

    // 按本地端口排序后取 TopN
    conns.sort_by_key(|c| (local_port(&c.local), c.proto.clone()));
    let shown = conns.iter().take(top_n).collect::<Vec<_>>();
    if shown.is_empty() {
        println!("\n没有匹配的连接记录。");
    } else {
        println!("\nTop {} 占用（按本地端口排序）:", shown.len());
        println!("{:>5} {:>4} {:>22} {:>12} {:>6}  {}", "PORT", "PROTO", "LOCAL", "STATE", "PID", "PROCESS");
        for c in shown {
            let name = pid_name.get(&c.pid).cloned().unwrap_or_else(|| "<unknown>".into());
            println!(
                "{:>5} {:>4} {:>22} {:>12} {:>6}  {}",
                local_port(&c.local),
                c.proto,
                c.local,
                if c.state.is_empty() { "-" } else { &c.state },
                c.pid,
                name
            );
        }
    }
    println!("\n提示: 本工具只读不杀进程。如需结束占用进程，请在 DiskPilot 主程序里走「确认后结束」流程。");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tcp_and_udp_lines() {
        let sample = "\
  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1024
  TCP    192.168.1.5:49152      140.82.112.3:443       ESTABLISHED     2048
  TCP    [::]:8080              [::]:0                 LISTENING       300
  UDP    0.0.0.0:5353           *:*                                    4096
";
        let v = parse_netstat(sample);
        assert_eq!(v.len(), 4);
        assert_eq!(v[0].proto, "TCP");
        assert_eq!(local_port(&v[0].local), 135);
        assert_eq!(v[0].state, "LISTENING");
        assert_eq!(v[0].pid, 1024);
        assert_eq!(local_port(&v[2].local), 8080, "IPv6 [::]:8080 端口应为 8080");
        assert_eq!(v[3].proto, "UDP");
        assert_eq!(v[3].state, "");
        assert_eq!(v[3].pid, 4096);
    }

    #[test]
    fn skips_header_and_garbage() {
        let v = parse_netstat("Active Connections\n\n  Proto Local\n  XXX bad line\n");
        assert!(v.is_empty());
    }

    #[test]
    fn tasklist_csv_mapping() {
        let sample = "\"explorer.exe\",\"1234\",\"Console\",\"1\",\"50,000 K\"\n\"svchost.exe\",\"56\",\"Services\",\"0\",\"10,000 K\"\n";
        let m = parse_tasklist(sample);
        assert_eq!(m.get(&1234).map(|s| s.as_str()), Some("explorer.exe"));
        assert_eq!(m.get(&56).map(|s| s.as_str()), Some("svchost.exe"));
    }
}
