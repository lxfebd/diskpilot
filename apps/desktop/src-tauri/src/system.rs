//! 系统实时状态采集 + 网络连通性探测（Tauri 命令层）。

use std::sync::Mutex;
use std::time::Duration;

use crate::shared_http;
use crate::volume_info;
#[derive(serde::Serialize, Default, Clone)]
#[allow(dead_code)] // 字段仅供前端/AI 序列化消费，Rust 侧不读
pub(crate) struct SystemProbe {
    /// CPU 总占用百分比（0-100，两次采样间空闲差推算）。
    cpu_percent: f64,
    /// 物理内存总量/已用/占用百分比（字节）。
    mem_total_bytes: u64,
    mem_used_bytes: u64,
    mem_percent: f64,
    /// 开机时长秒数（系统启动至今）。
    uptime_secs: u64,
    /// 各磁盘（盘符/总/已用/占用百分比）。
    drives: Vec<ProbeDrive>,
    /// CPU 逻辑核心数。
    cpu_cores: usize,
    /// 采集时刻的样本标签（两次采样）。
    sample_ns: u64,
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct ProbeDrive {
    path: String,
    total_bytes: u64,
    used_bytes: u64,
    percent: f64,
}

/// 读取 Windows 系统 CPU 忙闲时间（空闲差值法估算 CPU 占用）。
#[cfg(windows)]
fn cpu_busy_percent(prev: Option<(u64, u64)>) -> (f64, (u64, u64)) {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetSystemTimes;
    unsafe {
        let mut idle: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        if GetSystemTimes(&mut idle, &mut kernel, &mut user) == 0 {
            return (0.0, prev.unwrap_or((0, 0)));
        }
        let idle_us = ((idle.dwHighDateTime as u64) << 32) | idle.dwLowDateTime as u64;
        let kernel_us = ((kernel.dwHighDateTime as u64) << 32) | kernel.dwLowDateTime as u64;
        let user_us = ((user.dwHighDateTime as u64) << 32) | user.dwLowDateTime as u64;
        let busy = kernel_us.saturating_add(user_us).saturating_sub(idle_us);
        let now = (busy, idle_us);
        let pct = busy_delta_pct(prev, busy, idle_us);
        (pct, now)
    }
}

#[cfg(not(windows))]
fn cpu_busy_percent(prev: Option<(u64, u64)>) -> (f64, (u64, u64)) {
    (0.0, prev.unwrap_or((0, 0)))
}

/// 采集一次系统状态（Windows 用 Win32，其它平台尽力而为）。
fn probe_system(sample: Option<(u64, u64)>) -> SystemProbe {
    let mut out = SystemProbe::default();

    // CPU 占用：两次采样（间隔 ~500ms）求差值
    let (cpu_pct, _next_sample) = cpu_busy_percent(sample);
    out.cpu_percent = cpu_pct;
    out.cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    out.sample_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // 内存
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        unsafe {
            let mut ms: MEMORYSTATUSEX = std::mem::zeroed();
            ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut ms) != 0 {
                out.mem_total_bytes = ms.ullTotalPhys;
                out.mem_used_bytes = ms.ullTotalPhys.saturating_sub(ms.ullAvailPhys);
                out.mem_percent = ms.dwMemoryLoad as f64;
            }
        }
    }

    // 开机时长
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::GetTickCount64;
        unsafe {
            out.uptime_secs = GetTickCount64() / 1000;
        }
    }

    // 磁盘
    for letter in b'C'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if let Ok(mut info) = volume_info(root.clone()) {
            info.path = root;
            let total = info.total_bytes;
            let used = info.used_bytes;
            let percent = if total > 0 {
                (used as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            out.drives.push(ProbeDrive {
                path: info.path,
                total_bytes: total,
                used_bytes: used,
                percent,
            });
        }
    }
    out
}

/// 让 AI/用户读电脑实时状态：CPU 占用、内存、磁盘占用、开机时长。
/// 传 sample=true 时做两次采样求 CPU 差值（更准），默认单次（快）。
/// 进程级 CPU 采样基线（上次采样后的 busy/idle 状态）。跨调用保留，使后续
/// `sample=false` 的快速采样能直接用真实时间差算 CPU 占用，而不必每次
/// 都强制睡眠双采样——这是后台轮询卡顿的主要来源。
static CPU_BASELINE: Mutex<Option<(u64, u64)>> = Mutex::new(None);

#[tauri::command]
pub(crate) async fn run_system_probe(sample: bool) -> Result<SystemProbe, String> {
    tokio::task::spawn_blocking(move || {
        let prev = *CPU_BASELINE.lock().unwrap();
        let probe = match (sample, prev) {
            // 明确要求高精度：强制短采样后求差值（兼容旧语义）
            (true, _) => {
                let (_, base) = cpu_busy_percent(None);
                std::thread::sleep(Duration::from_millis(250));
                probe_system(Some(base))
            }
            // 默认快速：有基线直接用距离上次调用的真实差，零额外等待
            (false, Some(p)) => probe_system(Some(p)),
            // 首次/冷启动无基线：补一次短采样保证 CPU 值非 0
            (false, None) => {
                let (_, base) = cpu_busy_percent(None);
                std::thread::sleep(Duration::from_millis(250));
                probe_system(Some(base))
            }
        };
        // probe_system 内部取过本次状态但未回传，这里再读一次作为下次基线（代价极低）
        let (_, now) = cpu_busy_percent(None);
        *CPU_BASELINE.lock().unwrap() = Some(now);
        probe
    })
    .await
    .map_err(|e| e.to_string())
}

/// 单站点的连通性探测结果。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PingProbe {
    name: String,
    url: String,
    /// 可达与否
    ok: bool,
    /// 往返耗时（毫秒）；不可达时为 None
    latency_ms: Option<u64>,
    /// 失败原因摘要（不可达时）
    error: Option<String>,
}

/// 网络检测：对常用站点做真实 TCP 连接（HEAD 请求），走 Rust 后端而不是
/// WebView 的 fetch —— 后者受 CSP `connect-src` 限制，跨域请求会被浏览器拦掉，
/// 这就是之前「网络检测」永远显示不可达的根因。后端 reqwest 没有这个限制，
/// 结果也更贴近真实连通性（DNS + TLS + HTTP 都能真正打到）。
#[tauri::command]
pub(crate) async fn network_probe(targets: Vec<String>) -> Result<Vec<PingProbe>, String> {
    let sites: Vec<(String, String)> = targets.into_iter().filter_map(normalize_site).collect();

    if sites.is_empty() {
        return Err("没有可检测的站点".into());
    }

    let client = shared_http().clone();
    let mut results = Vec::with_capacity(sites.len());
    for (name, url) in sites {
        let t0 = std::time::Instant::now();
        // 只建立连接并拿到响应头，不下载正文；拿到任何 HTTP 响应（哪怕
        // 5xx）都算「可达」，只有 DNS/TLS/连接失败才判不可达。
        match client
            .get(&url)
            .header(reqwest::header::ACCEPT, "text/html")
            .send()
            .await
        {
            Ok(_) => results.push(PingProbe {
                name,
                url,
                ok: true,
                latency_ms: Some(t0.elapsed().as_millis() as u64),
                error: None,
            }),
            Err(e) => results.push(PingProbe {
                name,
                url,
                ok: false,
                latency_ms: None,
                error: Some(e.to_string()),
            }),
        }
    }

    Ok(results)
}

/// 由 (前次采样, 本次 busy, 本次 idle) 求 CPU 占用百分比（纯函数，可测）。
/// 样本间 busy/idle 差值比即占用率；差值全 0（同刻度）返回 0。
fn busy_delta_pct(prev: Option<(u64, u64)>, busy: u64, idle: u64) -> f64 {
    match prev {
        Some((p_busy, p_idle)) => {
            let d_busy = busy.saturating_sub(p_busy);
            let d_idle = idle.saturating_sub(p_idle);
            let d_total = d_busy.saturating_add(d_idle);
            if d_total == 0 {
                0.0
            } else {
                (d_busy as f64 / d_total as f64 * 100.0).clamp(0.0, 100.0)
            }
        }
        None => 0.0,
    }
}

/// 规范化单个站点输入：空串返回 None；缺协议补 https；name 取 host（纯函数，可测）。
fn normalize_site(t: String) -> Option<(String, String)> {
    let t = t.trim().to_string();
    if t.is_empty() {
        return None;
    }
    let url = if t.contains("://") {
        t.clone()
    } else {
        format!("https://{t}")
    };
    let name = t
        .replace("https://", "")
        .replace("http://", "")
        .split('/')
        .next()
        .unwrap_or(&t)
        .to_string();
    Some((name, url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_delta_pct_first_sample_is_zero() {
        assert_eq!(busy_delta_pct(None, 10, 5), 0.0);
    }

    #[test]
    fn busy_delta_pct_computes_busy_ratio() {
        // prev=(5,2) → 本次 (12,5)：d_busy=7, d_idle=3, d_total=10 → 70%
        let pct = busy_delta_pct(Some((5, 2)), 12, 5);
        assert!((pct - 70.0).abs() < 1e-9);
    }

    #[test]
    fn busy_delta_pct_zero_delta_is_zero() {
        assert_eq!(busy_delta_pct(Some((5, 2)), 5, 2), 0.0);
    }

    #[test]
    fn busy_delta_pct_no_idle_is_hundred() {
        // idle 不动、busy 前进 → 100%
        assert_eq!(busy_delta_pct(Some((5, 2)), 10, 2), 100.0);
    }

    #[test]
    fn busy_delta_pct_saturates_and_clamps() {
        // 计数器回绕（prev 更大）→ 差值饱和为 0 → d_total=0 → 0%
        assert_eq!(busy_delta_pct(Some((100, 100)), 5, 5), 0.0);
    }

    #[test]
    fn normalize_site_handles_variants() {
        assert_eq!(normalize_site("".to_string()), None);
        assert_eq!(normalize_site("   ".to_string()), None);
        let (name, url) = normalize_site("baidu.com".to_string()).unwrap();
        assert_eq!(name, "baidu.com");
        assert_eq!(url, "https://baidu.com");
        let (name, url) = normalize_site("https://www.baidu.com/path".to_string()).unwrap();
        assert_eq!(name, "www.baidu.com");
        assert_eq!(url, "https://www.baidu.com/path");
        let (name, url) = normalize_site("http://example.com:8080/a".to_string()).unwrap();
        assert_eq!(name, "example.com:8080");
        assert_eq!(url, "http://example.com:8080/a");
    }
}
