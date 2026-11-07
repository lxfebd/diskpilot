//! schtask-hunter —— DiskPilot 官方插件（只读诊断工具，Windows）。
//!
//! 用法: schtask-hunter.exe [--top N]
//!   默认 TopN=10，列出可疑计划任务 Top N + 摘要。
//!
//! 解析 `schtasks /Query /FO CSV /V` 拿到任务全景（状态/上次结果/触发器/命令路径），
//! 用纯函数判定可疑项：上次运行失败、启用且登录/开机触发、命令路径在临时目录。
//! 只做查询与报告，**不禁用任务、不改任何配置**。

use std::env;
use std::process::Command;
use std::time::Instant;

/// 一条计划任务记录（来自 schtasks /FO CSV /V 的关键列）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Task {
    name: String,
    state: String,       // Enabled / Disabled
    last_result: String, // 上次结果码（0 = 成功）
    task_to_run: String, // 命令路径
    schedule_type: String,
}

/// 解析一行 CSV（处理引号内逗号），可单测。
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if in_quotes && chars.peek() == Some(&'"') {
                cur.push('"');
                chars.next();
            } else {
                in_quotes = !in_quotes;
            }
        } else if c == ',' && !in_quotes {
            fields.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    fields.push(cur.trim().to_string());
    fields
}

/// 列名（中英文环境都覆盖）→ 索引。
fn col_index(header: &[String], names: &[&str]) -> Option<usize> {
    header.iter().position(|h| {
        let hl = h.to_ascii_lowercase();
        names.iter().any(|n| hl == *n)
    })
}

/// 从 schtasks /FO CSV /V 输出解析任务表（可单测）。首行是列头。
fn parse_schtasks(out: &str) -> Vec<Task> {
    let mut lines = out.lines();
    let header_line = match lines.next() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let header = parse_csv_line(header_line);
    let i_name = col_index(&header, &["taskname", "任务名"]);
    let i_state = col_index(&header, &["scheduled task state", "计划任务状态", "计划的任务状态"]);
    let i_last = col_index(&header, &["last result", "上次结果"]);
    let i_run = col_index(&header, &["task to run", "要运行的任务"]);
    let i_sched = col_index(&header, &["schedule type", "计划类型"]);
    let mut v = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let f = parse_csv_line(line);
        let get = |i: Option<usize>| i.and_then(|i| f.get(i)).cloned().unwrap_or_default();
        // schtasks /V 分页时会重复打印列头行（TaskName/上次结果 等）——跳过
        let name = get(i_name);
        let last = get(i_last);
        if name.eq_ignore_ascii_case("taskname") || name == "任务名" || last.eq_ignore_ascii_case("last result") || last == "上次结果" {
            continue;
        }
        v.push(Task {
            name,
            state: get(i_state),
            last_result: last,
            task_to_run: get(i_run),
            schedule_type: get(i_sched),
        });
    }
    v
}

/// 任务是否启用（中英文值都认：Enabled / 已启用）。
fn is_enabled(state: &str) -> bool {
    let s = state.trim();
    s.eq_ignore_ascii_case("enabled") || s == "已启用" || s == "Enabled"
}

/// 可疑原因判定（纯函数，可单测）。返回原因列表，空 = 不可疑。
fn suspicious_reasons(t: &Task) -> Vec<String> {
    let mut r = Vec::new();
    let enabled = is_enabled(&t.state);
    // 上次结果非 0 → 失败，但 267009（尚未运行）/267011（尚未到计划时间）是中性码不算失败
    if !t.last_result.is_empty() && t.last_result != "0" && t.last_result != "267009" && t.last_result != "267011" {
        r.push(format!("上次运行失败（结果码 {}）", t.last_result));
    }
    let low_run = t.task_to_run.to_ascii_lowercase();
    let tempish = low_run.contains("\\temp\\") || low_run.contains("\\appdata\\roaming\\") || low_run.contains("%temp%") || low_run.contains("/temp/");
    // 系统目录内的命令（Windows/Program Files）不算路径可疑，避免微软自带任务刷屏
    let systemish = low_run.contains("\\windows\\") || low_run.contains("\\program files") || low_run.contains("system32") || low_run == "com handler" || low_run.is_empty();
    let bootish = {
        let s = t.schedule_type.to_ascii_lowercase();
        s.contains("logon") || s.contains("startup") || s.contains("boot") || s.contains("登录") || s.contains("启动") || s.contains("开机")
    };
    if enabled && bootish && tempish {
        r.push("启用 + 登录/开机触发 + 命令在临时目录（高危组合）".to_string());
    } else if enabled && bootish && !systemish {
        r.push("启用 + 登录/开机触发 + 命令在非系统目录".to_string());
    }
    if enabled && tempish && !bootish {
        r.push("启用 + 命令在临时目录".to_string());
    }
    r
}

/// 跑系统命令并拿 stdout。schtasks 在中文 Windows 下输出 GBK，中文列头会被
/// from_utf8_lossy 糟蹋成替换符导致列名匹配失败——统一先 `chcp 65001` 切 UTF-8。
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("cmd")
        .args(["/c", "chcp 65001 >nul &"])
        .arg(cmd)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("schtask-hunter —— 计划任务审计器（只读，不禁用任务不改配置）");
        println!("用法: schtask-hunter.exe [--top N]");
        println!("  默认 TopN=10，输出可疑任务 Top N + 摘要。");
        return;
    }
    let top_n: usize = args
        .iter()
        .position(|a| a == "--top")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10);

    let t0 = Instant::now();
    let out = run("schtasks", &["/Query", "/FO", "CSV", "/V"]).unwrap_or_else(|| {
        eprintln!("错误: 无法运行 schtasks /Query /V（需要 Windows）");
        std::process::exit(2);
    });
    let tasks = parse_schtasks(&out);
    let total = tasks.len();
    let enabled = tasks.iter().filter(|t| is_enabled(&t.state)).count();
    let failed = tasks.iter().filter(|t| !t.last_result.is_empty() && t.last_result != "0" && t.last_result != "267009" && t.last_result != "267011").count();
    let mut flagged: Vec<(&Task, Vec<String>)> = tasks
        .iter()
        .map(|t| (t, suspicious_reasons(t)))
        .filter(|(_, r)| !r.is_empty())
        .collect();
    flagged.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.name.cmp(&b.0.name)));

    println!("计划任务审计（只读）: 共 {} 项，启用 {} 项，上次失败 {} 项，耗时 {:.2?}", total, enabled, failed, t0.elapsed());
    if flagged.is_empty() {
        println!("未发现可疑任务。");
    } else {
        println!("可疑任务 Top {}（共 {} 项）:", top_n, flagged.len());
        for (t, reasons) in flagged.iter().take(top_n) {
            println!("  [{}] {}", t.name, reasons.join("；"));
            println!("      命令: {}", if t.task_to_run.is_empty() { "(无)" } else { &t.task_to_run });
        }
    }
    println!("提示: 本插件只读不改任务；处置请在「任务计划程序」里人工确认后操作。");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_line_handles_quotes_and_commas() {
        let f = parse_csv_line("\"a,b\",\"c\"\"d\",plain");
        assert_eq!(f, vec!["a,b".to_string(), "c\"d".to_string(), "plain".to_string()]);
    }

    #[test]
    fn parse_schtasks_zh_header() {
        let out = "\"任务名\",\"计划任务状态\",\"上次结果\",\"要运行的任务\",\"计划类型\"\n\"\\\\Microsoft\\\\X\",\"Enabled\",\"0\",\"C:\\\\Windows\\\\System32\\\\cmd.exe\",\"At logon\"\n\"\\\\Evil\",\"Enabled\",\"1\",\"%TEMP%\\\\x.exe\",\"At logon\"\n";
        let v = parse_schtasks(out);
        assert_eq!(v.len(), 2);
        assert!(v[1].name.contains("Evil"));
        assert_eq!(v[1].last_result, "1");
    }

    #[test]
    fn suspicious_flags_boot_temp_and_failure() {
        let bad = Task {
            name: "x".into(),
            state: "Enabled".into(),
            last_result: "1".into(),
            task_to_run: "C:\\Users\\u\\AppData\\Roaming\\x.exe".into(),
            schedule_type: "At logon".into(),
        };
        let r = suspicious_reasons(&bad);
        assert!(r.iter().any(|s| s.contains("失败")));
        assert!(r.iter().any(|s| s.contains("高危组合")));
        // 正常微软任务不告警
        let ok = Task {
            name: "y".into(),
            state: "Enabled".into(),
            last_result: "0".into(),
            task_to_run: "C:\\Windows\\System32\\svchost.exe".into(),
            schedule_type: "Daily".into(),
        };
        assert!(suspicious_reasons(&ok).is_empty());
        // 尚未运行（267009）不算失败
        let nr = Task { last_result: "267009".into(), ..ok.clone() };
        assert!(suspicious_reasons(&nr).is_empty());
    }
}
