//! PowerShell 执行器：跑一段只读 PowerShell/CIM 脚本并捕获 stdout。
//!
//! 移植自主项目 `apps/desktop/src-tauri/src/hw.rs::ps_capture`（零耦合纯函数）：
//! - CREATE_NO_WINDOW 语义（stdin/stderr null）、try_wait 轮询超时强杀；
//! - 中文 Windows 上控制台输出是 GBK，UTF-8 失败时按 GBK 兜底解码（encoding_rs）；
//! - 脚本首行强制 `[Console]::OutputEncoding = UTF8`，根治中文乱码（GBK 兜底降为二阶保险）。
//!
//! 硬件/网络/服务/事件查询全走 CIM/PowerShell，**不需要管理员权限**（个别除外，见工具描述）。
//!
//! **跨平台**：`ps_capture` / `ps_capture_timeout` 仅在 Windows 有真实现（PowerShell.exe）；
//! 非 Windows 构建返回明确错误，不 panic、不 spawn 失败命令。各消费模块（hw/net/sys/sec/…）
//! 自身已有 `#[cfg(not(windows))]` 降级分支，运行时不会走到这里的非 Windows 实现——
//! 它只为让那些模块顶层的 `use crate::ps::ps_capture` 能在 macOS/Linux 上通过编译。
//! `json_array_of` 是纯文本函数，全平台可用。

use std::io::Read;
use std::process::Stdio;
use std::time::Duration;

#[cfg(windows)]
pub fn ps_capture(script: &str) -> Result<String, String> {
    ps_capture_timeout(script, Duration::from_secs(30))
}

#[cfg(windows)]
pub fn ps_capture_timeout(script: &str, timeout: Duration) -> Result<String, String> {
    // 首行注入 UTF-8 输出编码，避免中文系统 GBK 乱码（GBK 兜底仍在下面保留）
    let full = format!(
        "[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false);\n{script}"
    );
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &full,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 PowerShell 失败：{e}"))?;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("等待 PowerShell 失败：{e}"));
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("PowerShell 查询超时（{}s）", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(60));
    };
    let mut raw = Vec::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = so.read_to_end(&mut raw);
    }
    // 把 stderr 也读空，避免管道 buffer 残留导致句柄泄漏
    if let Some(mut se) = child.stderr.take() {
        let mut sink = Vec::new();
        let _ = se.read_to_end(&mut sink);
    }
    if !status.success() && raw.is_empty() {
        return Err(format!("PowerShell 退出码 {:?}", status.code()));
    }
    match std::str::from_utf8(&raw) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => {
            #[cfg(windows)]
            {
                let (cow, _, _) = encoding_rs::GBK.decode(&raw);
                Ok(cow.into_owned())
            }
            #[cfg(not(windows))]
            Ok(String::from_utf8_lossy(&raw).into_owned())
        }
    }
}

/// 非 Windows 平台：PowerShell 不存在，返回明确错误（消费模块的降级分支负责解释）。
#[cfg(not(windows))]
pub fn ps_capture(_script: &str) -> Result<String, String> {
    Err("当前平台不是 Windows，PowerShell 查询不可用".into())
}

/// 非 Windows 平台：PowerShell 不存在，返回明确错误。
#[cfg(not(windows))]
pub fn ps_capture_timeout(_script: &str, _timeout: Duration) -> Result<String, String> {
    Err("当前平台不是 Windows，PowerShell 查询不可用".into())
}

/// `ConvertTo-Json` 单项数组折叠成对象的坑：PS 里 `@(Get-CimInstance ...)` 只有 1 条时
/// 序列化出来是 `{...}` 而不是 `[{...}]`。本函数把文本包成 JSON 数组（若已是数组则原样）。
/// 用法：脚本末尾 `$out | ConvertTo-Json -Depth 4 -Compress`，结果再经本函数包数组。
pub fn json_array_of(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return "[]".into();
    }
    if t.starts_with('[') {
        return t.to_string();
    }
    format!("[{t}]")
}
