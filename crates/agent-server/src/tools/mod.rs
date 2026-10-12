//! MCP 工具路由（rmcp 1.8 API）：
//!
//! - 每个工具 = 一个参数 struct（derive Deserialize + JsonSchema）
//! - 每个工具 = 一个带 `#[tool(description)]` 的方法
//!
//! 纯逻辑在子模块（disk/files/process/system/apps），这里只做「参数 →
//! 纯函数 → 格式化结果」薄壳。

pub mod apps;
pub mod bench;
pub mod bsod;
pub mod cleanup;
pub mod disk;
pub mod diskx;
pub mod envx;
pub mod files;
pub mod hw;
pub mod hw_battery;
pub mod hw_common;
pub mod hw_cpu;
pub mod hw_disk;
pub mod hw_gpu;
pub mod hw_mem;
pub mod hw_sys;
pub mod hw_thermal;
pub mod hwinfo;
pub mod net;
pub mod net_adapter;
pub mod net_conn;
pub mod net_share;
pub mod net_speed;
pub mod net_status;
pub mod net_util;
pub mod net_wifi;
pub mod process;
pub mod report;
pub mod sec;
pub mod selfheal;
pub mod steam;
pub mod superio;
pub mod sys;
pub mod system;
pub mod toolbelt;

mod lenient;
// 工具分发 impl 按域拆分（2026-10-12）：各域独立的 `#[tool_router]` 块，
// router 在下方 `tool_router()` 合并。tool_sys 再按方法群拆为
// tool_sys / tool_sys_services / tool_sys_misc 三个平级块（2026-10-12）。
mod tool_disk;
mod tool_hw;
mod tool_hw_bench;
mod tool_hw_core;
mod tool_hw_ext;
mod tool_hw_report;
mod tool_misc;
mod tool_sys;
mod tool_sys_apps;
mod tool_sys_misc;
mod tool_sys_services;

use rmcp::{
    handler::server::ServerHandler,
    model::{AnnotateAble, CallToolResult, RawContent, ServerCapabilities, ServerInfo},
    tool_handler,
};
use std::sync::Mutex;

fn text_result(s: String) -> CallToolResult {
    CallToolResult::success(vec![RawContent::text(s).no_annotation()])
}

fn tool_error(msg: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![RawContent::text(msg.into()).no_annotation()])
}

/// 统一「阻塞任务」执行壳：spawn_blocking 把同步代码移出 tokio 运行时线程
/// （避免单点卡死拖垮整个 MCP 服务）+ tokio timeout 外层兜底（任何阻塞调用
/// 最长 secs 秒，挂起即返回错误不 hang 死 AI 调用链路）。
async fn blocking_call<T>(
    secs: u64,
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
    label: &str,
) -> CallToolResult
where
    T: std::fmt::Display + Send + 'static,
{
    match tokio::time::timeout(
        std::time::Duration::from_secs(secs),
        tokio::task::spawn_blocking(f),
    )
    .await
    {
        Ok(Ok(Ok(v))) => text_result(v.to_string()),
        Ok(Ok(Err(e))) => tool_error(e),
        Ok(Err(_)) => tool_error(format!("{label}线程异常退出")),
        Err(_) => tool_error(format!("{label}执行超时（>{secs}s），已中断防止挂起")),
    }
}

/// 字节数人类可读（与主项目 formatBytes 同语义）。
pub fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// 统一目录遍历器：files/diskx 等模块复刻过同一份 jwalk 配置，收敛到这里。
pub(crate) fn parallel_walker(root: &std::path::Path) -> jwalk::WalkDir {
    jwalk::WalkDir::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .max_depth(48)
}

/// JSON 取值转字符串：hw/net/sec/sys 四个模块各复刻了一份，收敛到这里。
pub(crate) fn v_str(v: &serde_json::Value, k: &str) -> String {
    match v.get(k) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// 读 REG_SZ / REG_EXPAND_SZ 字符串值（`RegQueryValueExW` 手动处理 `len` + NUL 截断）。
/// `REG_EXPAND_SZ` 的 `%VAR%` 由调用侧再展开，这里原样返回。
/// apps.rs / sys.rs 各复刻了一份 `read_str` / `read_str_val`，收敛到这里。
#[cfg(windows)]
pub(crate) fn reg_read_str(
    key: windows_sys::Win32::System::Registry::HKEY,
    value: &[u16],
) -> String {
    use windows_sys::Win32::System::Registry::RegQueryValueExW;
    let mut buf = [0u16; 2048];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegQueryValueExW(
            key,
            value.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        )
    };
    if r != 0 || len < 2 {
        return String::new();
    }
    // len 是字节数（含结尾 NUL）；转成 u16 计数并去结尾 NUL。
    let n = ((len as usize) / 2 - 1).min(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

/// 读 REG_DWORD。缺失返回 `None`。
/// apps.rs / sys.rs 各复刻了一份 `read_dword` / `read_dword_val`，收敛到这里。
#[cfg(windows)]
pub(crate) fn reg_read_dword(
    key: windows_sys::Win32::System::Registry::HKEY,
    value: &[u16],
) -> Option<u32> {
    use windows_sys::Win32::System::Registry::RegQueryValueExW;
    let mut buf = 0u32;
    let mut len = 4u32;
    let r = unsafe {
        RegQueryValueExW(
            key,
            value.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut buf as *mut u32 as *mut u8,
            &mut len,
        )
    };
    if r != 0 {
        return None;
    }
    Some(buf)
}

// ── 静态硬件 TTL 缓存（对齐主进程 hw.rs 三层） ──────────────────────────
// 主进程：HW_SNAPSHOT_TTL=300s / HW_DISK_TTL=300s / THERMAL_TTL=10s。
// agent-server 是常驻进程（无共享状态的历史原因在 agent.rs:28），
// 静态硬件（CPU/内存/主板/BIOS）5 分钟内直接复用，不再每次重跑 PowerShell。

/// 静态硬件（CPU/内存/主板）TTL：基本不变，5 分钟复用。
pub(crate) const HW_STATIC_TTL: std::time::Duration = std::time::Duration::from_secs(300);
/// 磁盘健康（SMART）TTL：通电时间/磨损不会秒变，5 分钟复用。
pub(crate) const HW_DISK_TTL: std::time::Duration = std::time::Duration::from_secs(300);

/// 通用 TTL 缓存槽：存 `(采集时刻, 结果文本)`，TTL 内命中直接复用。
struct TtlSlot {
    at: std::time::Instant,
    value: String,
}
type TtlCache = Mutex<Option<TtlSlot>>;

/// 创建/复用静态硬件缓存槽（与主进程 `hw_snapshot_lock` 同手法）。
fn ttl_slot(cell: &'static std::sync::OnceLock<TtlCache>) -> &'static TtlCache {
    cell.get_or_init(|| Mutex::new(None))
}

/// 静态硬件 TTL 缓存：TTL 内直接返回缓存，过期则跑 `f` 并把结果写回缓存。
/// 错误不缓存（采集失败下次再试），`None` 值（读不到）也缓存——对齐主进程
/// `sample_thermal_c` 的「不可读也缓存」行为，避免每轮白 spawn。
fn ttl_cached(
    cache: &'static std::sync::OnceLock<TtlCache>,
    ttl: std::time::Duration,
    f: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    let now = std::time::Instant::now();
    let guard = ttl_slot(cache);
    if let Ok(slot) = guard.lock() {
        if let Some(entry) = slot.as_ref() {
            if now.duration_since(entry.at) < ttl {
                return Ok(entry.value.clone());
            }
        }
    }
    let v = f()?;
    if let Ok(mut slot) = guard.lock() {
        *slot = Some(TtlSlot {
            at: std::time::Instant::now(),
            value: v.clone(),
        });
    }
    Ok(v)
}

// 每个静态硬件工具独立缓存槽（共用一个槽会被不同工具互相污染）：
// hw_cpu / hw_memory / hw_motherboard / hw_disk_smart 各一个。
static HW_CPU_CACHE: std::sync::OnceLock<TtlCache> = std::sync::OnceLock::new();
static HW_MEM_CACHE: std::sync::OnceLock<TtlCache> = std::sync::OnceLock::new();
static HW_MB_CACHE: std::sync::OnceLock<TtlCache> = std::sync::OnceLock::new();
static HW_DISK_CACHE: std::sync::OnceLock<TtlCache> = std::sync::OnceLock::new();

#[derive(Debug, Clone)]
pub struct AgentServer;

impl AgentServer {
    pub fn new() -> Self {
        Self
    }

    /// 合并各域 router：disk/sys/hw/misc 各自的 `#[tool_router]` 块生成独立
    /// 关联函数，这里 `+` 组合（ToolRouter 实现 Add）。`#[tool_handler]` 默认
    /// 引用 `Self::tool_router()`，保持工具分发行为与拆分前完全一致。
    fn tool_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::tool_router_disk()
            + Self::tool_router_sys()
            + Self::tool_router_sys_apps()
            + Self::tool_router_sys_services()
            + Self::tool_router_sys_misc()
            + Self::tool_router_hw_core()
            + Self::tool_router_hw_ext()
            + Self::tool_router_hw_bench()
            + Self::tool_router_hw_report()
            + Self::tool_router_misc()
    }
}

impl Default for AgentServer {
    fn default() -> Self {
        Self::new()
    }
}
// 参数 struct 全部在 params.rs（纯数据定义层，2026-10-12 拆分），
// 由各域 tool_*.rs 子模块 `use crate::tools::params::*;` 引入。
mod params;

// ── ServerHandler（#[tool_handler] 自动生成 call_tool / list_tools） ────

#[tool_handler]
impl ServerHandler for AgentServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "DiskPilot 的 AI 可操作工具集：只读磁盘/系统/文件/进程/程序清单查询。\
             所有路径参数受安全守卫保护（盘根/系统目录/主目录根被拒绝）。\
             回答磁盘空间、系统状态、目录内容、进程、已装软件问题时先用这些工具拿真实数据。",
        )
    }
}
