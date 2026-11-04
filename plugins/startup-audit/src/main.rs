//! startup-audit —— DiskPilot 官方插件（只读审计工具）。
//!
//! 用法: startup-audit.exe
//!
//! 列出三类开机启动项（全部只读查询，不做任何修改）:
//!   1. 注册表 HKCU\...\Run（当前用户）
//!   2. 注册表 HKLM\...\Run（所有用户，需管理员才能看到全部，否则尽力而为）
//!   3. 启动文件夹（当前用户 + 公共）里的快捷方式/程序
//!
//! 输出原始 reg query 结果 + 文件夹清单，并在末尾给出"如何手动禁用"的提示。

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn reg_query(key: &str) -> String {
    let out = Command::new("reg")
        .args(["query", key])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        Ok(o) => format!("(查询失败: {}) {}", o.status, String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("(无法执行 reg.exe: {e})"),
    }
}

fn list_dir(label: &str, dir: Option<PathBuf>) {
    let Some(d) = dir else { return };
    println!("\n== {label}: {}", d.display());
    let Ok(rd) = fs::read_dir(&d) else {
        println!("  (无法读取)");
        return;
    };
    let mut items: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    items.sort();
    if items.is_empty() {
        println!("  (空)");
    }
    for it in items {
        println!("  - {it}");
    }
}

fn main() {
    if env::args().skip(1).any(|a| a == "-h" || a == "--help") {
        println!("startup-audit —— 开机启动项审计（只读，不做任何修改）");
        println!("用法: startup-audit.exe");
        return;
    }
    println!("启动项审计（只读报告，不做任何修改）\n");

    println!("== 1. 注册表 HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run（当前用户）");
    print!("{}", reg_query(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run"));

    println!("\n== 2. 注册表 HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Run（所有用户）");
    print!("{}", reg_query(r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run"));

    println!("\n== 3. 注册表 HKCU\\...\\Explorer\\RunOnce（当前用户一次性）");
    print!("{}", reg_query(r"HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce"));

    // 启动文件夹
    let appdata = env::var("APPDATA").ok().map(PathBuf::from);
    let programdata = env::var("ProgramData").ok().map(PathBuf::from);
    if let Some(a) = appdata {
        list_dir(
            "启动文件夹（当前用户）",
            Some(a.join(r"Microsoft\Windows\Start Menu\Programs\Startup")),
        );
    }
    if let Some(p) = programdata {
        list_dir(
            "启动文件夹（公共，所有用户）",
            Some(p.join(r"Microsoft\Windows\Start Menu\Programs\Startup")),
        );
    }

    println!("\n提示: 本工具只读。禁用启动项请用「任务管理器 → 启动应用」或 msconfig，");
    println!("或在 DiskPilot 主程序里走带确认的管理流程。删除注册表项有风险，不建议手动删。");
}
