//! reg-hunter —— DiskPilot 官方插件（只读诊断工具，Windows）。
//!
//! 用法: reg-hunter.exe [--top N]
//!   默认 TopN=10。
//!
//! 用 `reg query /s` 只读枚举：
//!   - 卸载项（HKLM/HKCU ...\Uninstall，含 Wow6432Node）——InstallLocation /
//!     DisplayIcon 指向的路径不存在 → 失效残留（疑似卸载不干净）。
//!   - Run / RunOnce 启动项——命令路径不存在 → 失效启动项。
//! 纯函数只做文本判定，路径存在性由 main 用 fs::metadata 注入。
//! 只做查询与报告，**不删注册表键、不改任何配置**。

use std::env;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// 一个注册表值（reg query /s 输出里的一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct RegVal {
    key: String,   // 完整键路径
    name: String,  // 值名（DisplayName / InstallLocation / 空=默认值）
    data: String,  // 数据
}

/// 解析 `reg query <key> /s` 输出为值列表（可单测）。
/// 格式：空行分隔键块，键行以 HKEY_ 开头，值行缩进 4 空格: "名    REG_SZ    数据"。
fn parse_reg_query(out: &str) -> Vec<RegVal> {
    let mut v = Vec::new();
    let mut cur_key = String::new();
    for line in out.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with("HKEY_") {
            cur_key = line.trim().to_string();
            continue;
        }
        // 值行：前导空白 + 值名(可含空格) + 空白 + REG_xxx + 空白 + 数据(保留内部空格)
        let t = line.trim_start();
        if line.len() == t.len() {
            continue; // 键行已处理，其余非缩进行忽略
        }
        // 找到第一个 REG_ 类型子串的字节位置
        let ty_pos = match t.find("REG_") {
            Some(p) => p,
            None => continue,
        };
        // 类型 token 结束位置（到下一个空白）
        let ty_end = t[ty_pos..]
            .find(char::is_whitespace)
            .map(|i| ty_pos + i)
            .unwrap_or(t.len());
        let ty = &t[ty_pos..ty_end];
        if !ty.starts_with("REG_") || ty.len() <= 4 {
            continue;
        }
        let name = t[..ty_pos].trim().to_string();
        let name = if name == "(Default)" { String::new() } else { name };
        let data = t[ty_end..].trim().to_string();
        v.push(RegVal { key: cur_key.clone(), name, data });
    }
    v
}

/// 卸载项条目（从值列表聚合一个键的关键字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct UninstallEntry {
    key: String,
    display_name: String,
    install_location: String,
    display_icon: String,
}

/// 把值列表按键聚合成卸载项（可单测）。只认 ...\Uninstall\ 下的键。
fn collect_uninstall(vals: &[RegVal]) -> Vec<UninstallEntry> {
    use std::collections::HashMap;
    let mut m: HashMap<String, UninstallEntry> = HashMap::new();
    for v in vals {
        if !v.key.contains("\\Uninstall\\") {
            continue;
        }
        let e = m.entry(v.key.clone()).or_insert_with(|| UninstallEntry {
            key: v.key.clone(),
            display_name: String::new(),
            install_location: String::new(),
            display_icon: String::new(),
        });
        match v.name.as_str() {
            "DisplayName" => e.display_name = v.data.clone(),
            "InstallLocation" => e.install_location = v.data.clone(),
            "DisplayIcon" => e.display_icon = v.data.clone(),
            _ => {}
        }
    }
    let mut v: Vec<UninstallEntry> = m.into_values().collect();
    v.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    v
}

/// 判定卸载项是否失效（纯函数，路径存在性外部注入，可单测）。
/// 规则：有 InstallLocation 且路径不存在，或 DisplayIcon 含路径且不存在 → 失效。
fn stale_reasons(e: &UninstallEntry, loc_exists: bool, icon_exists: bool) -> Vec<String> {
    let mut r = Vec::new();
    if !e.install_location.is_empty() && !loc_exists {
        r.push(format!("InstallLocation 不存在: {}", e.install_location));
    }
    if !e.display_icon.is_empty() && !icon_exists {
        // DisplayIcon 可能是 "C:\a\b.exe,0" 形式，剥掉 ,N 后缀
        let p = e.display_icon.split(',').next().unwrap_or("").trim_matches('"');
        if !p.is_empty() && !icon_exists {
            r.push(format!("DisplayIcon 不存在: {}", p));
        }
    }
    r
}

/// Run 启动项条目。
#[derive(Debug, Clone, PartialEq, Eq)]
struct RunEntry {
    key: String,
    name: String,
    data: String,
}

/// 从值列表里挑出 Run/RunOnce 键下的非空值（可单测）。
fn collect_run(vals: &[RegVal]) -> Vec<RunEntry> {
    vals.iter()
        .filter(|v| v.key.contains("\\CurrentVersion\\Run") && !v.name.is_empty())
        .map(|v| RunEntry { key: v.key.clone(), name: v.name.clone(), data: v.data.clone() })
        .collect()
}

/// 判定启动项命令路径是否存在（纯函数部分：剥引号/参数；存在性外部注入）。
fn run_cmd_path(data: &str) -> String {
    let d = data.trim();
    if d.starts_with('"') {
        if let Some(end) = d[1..].find('"') {
            return d[1..1 + end].to_string();
        }
    }
    // 无引号：取到第一个空格（含参数）或整段
    d.split_whitespace().next().unwrap_or("").to_string()
}

fn path_exists(p: &str) -> bool {
    !p.is_empty() && Path::new(p).exists()
}

/// 路径是否在系统盘（C:）——非系统盘路径"不存在"多半是外置盘没插，单独标注防误报。
fn is_system_drive_path(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 2 && b[0].to_ascii_lowercase() == b'c' as u8 && b[1] == b':' as u8
}

fn run_reg(key: &str) -> Option<String> {
    let out = Command::new("reg").args(["query", key, "/s"]).output().ok()?;
    // reg 在中文系统输出 GBK；值名/键名是 ASCII，数据可能中文——lossy 兜底
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("reg-hunter —— 注册表审计器（只读，不删键不改配置）");
        println!("用法: reg-hunter.exe [--top N]");
        println!("  审计卸载残留（路径失效）与 Run/RunOnce 失效启动项，TopN 默认 10。");
        return;
    }
    let top_n: usize = args
        .iter()
        .position(|a| a == "--top")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10);

    let t0 = Instant::now();
    let keys = [
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run",
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\RunOnce",
        "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run",
        "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\RunOnce",
    ];
    let mut vals = Vec::new();
    for k in keys {
        if let Some(out) = run_reg(k) {
            vals.extend(parse_reg_query(&out));
        }
    }
    let uninstalls = collect_uninstall(&vals);
    let runs = collect_run(&vals);

    // 失效判定（路径存在性在这里注入）
    let mut stale: Vec<(&UninstallEntry, Vec<String>)> = uninstalls
        .iter()
        .map(|e| {
            let loc_ok = e.install_location.is_empty() || path_exists(&e.install_location);
            let icon_raw = e.display_icon.split(',').next().unwrap_or("").trim_matches('"').to_string();
            let icon_ok = icon_raw.is_empty() || path_exists(&icon_raw);
            let mut reasons = stale_reasons(e, loc_ok, icon_ok);
            // 非系统盘路径不可达 → 标注"疑似外置盘未挂载"，与真失效区分
            let on_sys = is_system_drive_path(&e.install_location)
                || is_system_drive_path(&icon_raw);
            if !reasons.is_empty() && !on_sys {
                reasons.push("路径不在系统盘，可能是外置盘未挂载".to_string());
            }
            (e, reasons)
        })
        .filter(|(_, r)| !r.is_empty())
        .collect();
    stale.sort_by(|a, b| a.0.display_name.cmp(&b.0.display_name));

    let mut dead_runs: Vec<&RunEntry> = runs
        .iter()
        .filter(|r| {
            let p = run_cmd_path(&r.data);
            // 无盘符/无路径分隔的（如 cmd.exe、rundll32.exe）是系统命令，不查存在性
            if p.is_empty() || (!p.contains(':') && !p.contains('\\') && !p.contains('/')) {
                return false;
            }
            !path_exists(&p)
        })
        .collect();
    dead_runs.sort_by(|a, b| a.key.cmp(&b.key).then_with(|| a.name.cmp(&b.name)));

    println!(
        "注册表审计（只读）: 卸载项 {} 条，Run/RunOnce 启动项 {} 条，失效卸载 {} 条，失效启动项 {} 条，耗时 {:.2?}",
        uninstalls.len(),
        runs.len(),
        stale.len(),
        dead_runs.len(),
        t0.elapsed()
    );
    if !stale.is_empty() {
        println!("失效卸载残留 Top {}（共 {} 条）:", top_n, stale.len());
        for (e, reasons) in stale.iter().take(top_n) {
            println!("  [{}] {}", e.key.rsplit('\\').next().unwrap_or(&e.key), e.display_name);
            for r in reasons {
                println!("      - {}", r);
            }
        }
    }
    if !dead_runs.is_empty() {
        println!("失效启动项（命令路径不存在）Top {}（共 {} 条）:", top_n, dead_runs.len());
        for r in dead_runs.iter().take(top_n) {
            println!("  [{}] {} → {}", r.key.rsplit('\\').next().unwrap_or(&r.key), r.name, run_cmd_path(&r.data));
        }
    }
    if stale.is_empty() && dead_runs.is_empty() {
        println!("未发现失效残留。");
    }
    println!("提示: 本插件只读不删注册表键；清理请人工确认后在 regedit 操作。");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reg_query_values() {
        let out = "HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Foo\n    DisplayName    REG_SZ    Foo App 1.0\n    InstallLocation    REG_SZ    C:\\Program Files\\Foo\\\n\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run\n    Bar    REG_SZ    \"C:\\Tools\\bar.exe\" /min\n";
        let v = parse_reg_query(out);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].name, "DisplayName");
        assert_eq!(v[0].data, "Foo App 1.0");
        assert_eq!(v[2].name, "Bar");
        assert_eq!(v[2].data, "\"C:\\Tools\\bar.exe\" /min");
    }

    #[test]
    fn stale_reasons_flags_missing_paths() {
        let e = UninstallEntry {
            key: "HKLM\\...\\Uninstall\\X".into(),
            display_name: "X".into(),
            install_location: "C:\\Gone\\".into(),
            display_icon: "C:\\Gone\\x.exe,0".into(),
        };
        let r = stale_reasons(&e, false, false);
        assert_eq!(r.len(), 2);
        // 路径存在 → 不失效
        assert!(stale_reasons(&e, true, true).is_empty());
    }

    #[test]
    fn run_cmd_path_strips_args_and_quotes() {
        assert_eq!(run_cmd_path("\"C:\\A B\\c.exe\" /x"), "C:\\A B\\c.exe");
        assert_eq!(run_cmd_path("C:\\A\\c.exe /x"), "C:\\A\\c.exe");
        assert_eq!(run_cmd_path("rundll32.exe shell32.dll,Control_RunDLL"), "rundll32.exe");
    }
}
