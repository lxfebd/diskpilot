//! 命令解析与执行（toolbelt_command_for / run / status）。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：Windows 风格切词（parse_args）、
//! toolbelt.toml 覆盖读取、命令解析（toolbelt_command_for）、进程执行与
//! 超时强杀（run / wait_with_timeout），以及前端状态查询（status /
//! grouped_index）。

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
// PathBuf 仅供 cfg(windows) 的 verbatim 前缀剥离块使用。
#[cfg(windows)]
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::catalog::{
    catalog_cache, find, CliTool, ResolvedCommand, RunOutcome, ToolOverride, ToolbeltError,
};
use crate::risk::{Risk, MAX_TIMEOUT_SECS, MIN_TIMEOUT_SECS};
use crate::root::find_tools_root;

/// Windows 风格切词：双引号内保留空格，支持 `\"` 转义。用于 AI 传原始
/// 命令行串的场景（前端直接用 Vec<String> 则不经过这里）。
pub fn parse_args(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => in_quote = !in_quote,
            '\\' if chars.peek() == Some(&'"') => {
                chars.next();
                cur.push('"');
            }
            c if c.is_whitespace() && !in_quote => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() || in_quote {
        out.push(cur);
    }
    out
}

/// 读 exe 同目录的 toolbelt.toml（不存在/解析失败 = 无覆盖，只告警）。
fn load_override(exe_dir: &Path) -> Option<ToolOverride> {
    let p = exe_dir.join("toolbelt.toml");
    let text = std::fs::read_to_string(p.clone()).ok()?;
    match toml::from_str::<ToolOverride>(&text) {
        Ok(o) => Some(o),
        Err(e) => {
            tracing_warn(&format!("toolbelt.toml 解析失败 {:?}: {}", p, e));
            None
        }
    }
}

// tauri 侧有 tracing，但这个 crate 不拉依赖——出错只留 stderr，绝不 panic。
pub(crate) fn tracing_warn(msg: &str) {
    eprintln!("[diskpilot-toolbelt] {}", msg);
}

/// **核心**：把「工具名 + 实参」解析成可执行命令（丢失重建版，语义对齐
/// tubatools 的 run_cli_tool + toolbelt.toml 覆盖机制）。
///
/// - 工具名不区分大小写，支持部分匹配（"wiztree" / "autorunsc" 都能命中）
/// - `args` 为实参列表；为空时回退到该目录 toolbelt.toml 的固定默认 `args`
/// - `timeout_secs` 钳制在 5..3600；省略用工具默认（烤机类 1800）
/// - `cwd` 固定为 exe 目录：DiskInfo.txt / cli_done.txt / 日志都落在那里
pub fn toolbelt_command_for(
    name: &str,
    args: &[String],
    tools_root: &Path,
    timeout_secs: Option<u64>,
) -> Result<ResolvedCommand, ToolbeltError> {
    let tool = find(name).ok_or_else(|| ToolbeltError::UnknownTool {
        name: name.to_string(),
        available: catalog_cache()
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(" / "),
    })?;
    let rel = tool
        .exe_rel
        .as_ref()
        .ok_or_else(|| ToolbeltError::NoExePath {
            name: tool.name.clone(),
        })?;

    // 防目录穿越：文档内的相对路径本身可信，但 overrides 场景下做最严校验。
    let exe = tools_root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let exe = exe
        .canonicalize()
        .map_err(|_| ToolbeltError::ExeMissing { path: exe })?;
    // Windows canonicalize 返回 `\\?\` verbatim 前缀路径，std::process::Command 直接
    // spawn 它报 os error 123（语法不正确）→ 剥掉前缀改用普通盘符路径（工具目录在
    // 盘根深处常规长度内，无需超长路径支持）。
    #[cfg(windows)]
    let exe = {
        let s = exe.to_string_lossy();
        if s.starts_with("\\\\?\\") {
            PathBuf::from(s.trim_start_matches("\\\\?\\"))
        } else {
            exe
        }
    };
    if !exe.is_file() {
        return Err(ToolbeltError::ExeMissing { path: exe.clone() });
    }

    let ov = load_override(exe.parent().unwrap_or(tools_root));
    let risk = ov
        .as_ref()
        .and_then(|o| o.risk.as_deref())
        .and_then(Risk::parse)
        .unwrap_or(tool.risk);
    let args = if !args.is_empty() {
        args.to_vec()
    } else {
        ov.as_ref().map(|o| o.args.clone()).unwrap_or_default()
    };
    let timeout = timeout_secs
        .or_else(|| ov.as_ref().and_then(|o| o.timeout_secs))
        .unwrap_or(tool.timeout_secs);
    let timeout = timeout.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS);

    let mut usage = tool.detail.clone();
    if let Some(u) = ov.as_ref().and_then(|o| o.usage.clone()) {
        usage.push_str("\n\n## 本地补充说明（toolbelt.toml）\n");
        usage.push_str(&u);
    }

    Ok(ResolvedCommand {
        tool: tool.name.clone(),
        category: tool.category.clone(),
        cwd: exe
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| tools_root.to_path_buf()),
        program: exe,
        args,
        risk,
        timeout_secs: timeout,
        usage,
    })
}

// ── 执行 ────────────────────────────────────────────────────────────

fn decode_bytes(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        #[cfg(windows)]
        Err(_) => {
            // 控制台中文工具（urwtest 等）输出 GBK，UTF-8 失败则按 GBK 解。
            let (cow, _, _) = encoding_rs::GBK.decode(raw);
            cow.into_owned()
        }
        #[cfg(not(windows))]
        Err(_) => String::from_utf8_lossy(raw).into_owned(),
    }
}

fn read_capped(mut r: impl Read, cap: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match r.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < cap {
                    let keep = (cap - buf.len()).min(n);
                    buf.extend_from_slice(&chunk[..keep]);
                }
            }
            Err(_) => break,
        }
    }
    buf
}

/// 执行已解析命令：捕获 stdout/stderr（各上限 1 MiB），超时强杀。
/// 控制台程序在 Windows 下带 CREATE_NO_WINDOW，避免闪黑框。
pub fn run(cmd: &ResolvedCommand) -> RunOutcome {
    let timeout = Duration::from_secs(cmd.timeout_secs);
    let mut command = Command::new(&cmd.program);
    command
        .args(&cmd.args)
        .current_dir(&cmd.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            // ERROR_ELEVATION_REQUIRED：目标工具声明必须管理员运行。
            // CLI 执行要捕获 stdout，无法用 runas 提权窗口，只能如实告知。
            let detail = if e.raw_os_error() == Some(740) {
                format!(
                    "{}（os error 740）——该工具要求管理员权限，命令行方式无法捕获其输出；请改在工具墙双击启动（会弹出 UAC 提权）",
                    cmd.program.display()
                )
            } else {
                format!("{}（{}）", cmd.program.display(), e)
            };
            return RunOutcome {
                exit_code: None,
                timed_out: false,
                stdout: String::new(),
                stderr: format!("启动失败：{detail}"),
            };
        }
    };

    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let (err_tx, err_rx) = mpsc::channel::<Vec<u8>>();
    if let Some(so) = child.stdout.take() {
        let tx = out_tx;
        thread::spawn(move || {
            let _ = tx.send(read_capped(so, 1 << 20));
        });
    }
    if let Some(se) = child.stderr.take() {
        let tx = err_tx;
        thread::spawn(move || {
            let _ = tx.send(read_capped(se, 1 << 20));
        });
    }

    let started = Instant::now();
    let (status, timed_out) = wait_with_timeout(&mut child, timeout, started);
    let stdout = out_rx
        .recv()
        .ok()
        .map(|v| decode_bytes(&v))
        .unwrap_or_default();
    let stderr = err_rx
        .recv()
        .ok()
        .map(|v| decode_bytes(&v))
        .unwrap_or_default();
    RunOutcome {
        exit_code: status.and_then(|s| s.code()),
        timed_out,
        stdout,
        stderr,
    }
}

fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
    started: Instant,
) -> (Option<std::process::ExitStatus>, bool) {
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return (Some(st), false),
            Ok(None) => {}
            Err(_) => return (None, false),
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return (None, true);
        }
        thread::sleep(Duration::from_millis(80));
    }
}

/// 前端一次性拉取的完整状态：Tools 根 + 每个工具是否就位。
#[derive(Serialize)]
pub struct ToolbeltStatus {
    pub tools_root: Option<String>,
    pub tools: Vec<ToolSpec>,
}

#[derive(Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub category: String,
    pub description: String,
    pub exe_rel: Option<String>,
    pub risk: Risk,
    pub timeout_secs: u64,
    pub installed: bool,
}

pub fn status(explicit_root: Option<&Path>) -> ToolbeltStatus {
    let root = match find_tools_root(explicit_root) {
        Ok(r) => Some(r),
        Err(e) => {
            tracing_warn(&e.to_string());
            None
        }
    };
    let tools = catalog_cache()
        .iter()
        .map(|t| ToolSpec {
            name: t.name.clone(),
            category: t.category.clone(),
            description: t.description.clone(),
            exe_rel: t.exe_rel.clone(),
            risk: t.risk,
            timeout_secs: t.timeout_secs,
            installed: root
                .as_ref()
                .zip(t.exe_rel.as_deref())
                .map(|(r, rel)| {
                    r.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
                        .is_file()
                })
                .unwrap_or(false),
        })
        .collect();
    ToolbeltStatus {
        tools_root: root.map(|p| p.display().to_string()),
        tools,
    }
}

/// 按分类聚合的索引（保持文档出现顺序，前端直接渲染分组）。
pub fn grouped_index() -> Vec<(String, Vec<CliTool>)> {
    let mut order: Vec<String> = Vec::new();
    let mut map: BTreeMap<String, Vec<CliTool>> = BTreeMap::new();
    for t in catalog_cache() {
        map.entry(t.category.clone())
            .and_modify(|v| v.push(t.clone()))
            .or_insert_with(|| {
                order.push(t.category.clone());
                vec![t.clone()]
            });
    }
    order
        .into_iter()
        .map(|k| (k.clone(), map.remove(&k).unwrap_or_default()))
        .collect()
}
