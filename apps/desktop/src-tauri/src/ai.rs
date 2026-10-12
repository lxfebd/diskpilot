//! AI 请求代理（Tauri 命令层）：SSRF 防护 / 手动重定向 / 取消。
//! 联网搜索（web_search）已拆到 `ai_net` 模块。
//!
//! 2026-10-12 再拆分：SSRF 私网判定 → `ai_ssrf`。本文件保留代理命令与单测。

use std::collections::HashMap;
use std::time::Duration;

use crate::net_http::http_no_redirect;

#[path = "ai_ssrf.rs"]
pub(crate) mod ai_ssrf;

pub(crate) use ai_ssrf::blocked_private_target;

// ── AI 请求代理 ─────────────────────────────────────────────────────────
// 生产模式页面 origin 是 tauri.localhost，前端直接 fetch 外部 AI API 会被
// WebView CORS 拦截（报 TypeError: Failed to fetch，但 PowerShell 直连同一
// 地址却能通——网络没问题，纯粹是浏览器跨域限制）。这里把 AI 的 HTTP 请求
// 转发给后端 reqwest 执行（服务端无 CORS 概念），并按 web_search 相同逻辑
// 走系统代理。只透传 caller 显式提供的头，绝不过度信任任意 URL 行为：
// 仅当 url 以 http:// 或 https:// 开头才放行。

#[derive(serde::Serialize)]
pub(crate) struct AiProxyResponse {
    status: u16,
    body: String,
}

#[tauri::command]
pub(crate) async fn ai_proxy(
    state: tauri::State<'_, crate::AppState>,
    url: String,
    method: Option<String>,
    headers: Option<HashMap<String, String>>,
    body: Option<String>,
    timeout_secs: Option<u64>,
    cancel_key: Option<String>,
) -> Result<AiProxyResponse, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("不安全的代理目标 URL".into());
    }
    // SSRF 防护：只允许公网 HTTP(S) 目标。环回/私网/链路本地地址（内网
    // 管理口、云元数据 169.254.169.254 等）一律拒绝，防止前端被 XSS 后
    // 借本进程向内网发起请求。
    if let Some(blocked) = blocked_private_target(&url) {
        return Err(format!("代理目标不可用（{blocked}）：{url}"));
    }
    // 共享 client 默认 12s 超时对 AI 生成不够，用请求级 timeout 覆盖；
    // 上限 300s 防住无限挂起。
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(60).min(300));
    // 用禁重定向的专用 client：默认跟随会把「首跳合法公网 → 302 到内网」的
    // SSRF 绕过漏掉，这里关掉后手动跟随并逐个校验跳转目标。
    let client = http_no_redirect();

    // 用户停止：注册一个 cancel flag，前端 ai_cancel(cancel_key) 置 true，
    // 发送循环轮询到 true 即中断返回「已取消」。用 Drop 守卫保证任何返回
    // 路径（成功/超时/响应过大/重定向过多）都会移除 key，表不膨胀。
    let cancel = if let Some(key) = &cancel_key {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        state
            .ai_cancels
            .lock()
            .unwrap()
            .insert(key.clone(), std::sync::Arc::clone(&flag));
        Some(std::sync::Arc::new(CancelGuard {
            state: &state,
            key: key.clone(),
            flag: std::sync::Arc::clone(&flag),
        }))
    } else {
        None
    };
    let cancelled = || {
        cancel
            .as_ref()
            .map(|g| g.flag.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(false)
    };

    // 307/308 重定向要求保留方法——只解析一次，重定向环内复用。
    let http_method = reqwest::Method::from_bytes(method.as_deref().unwrap_or("POST").as_bytes())
        .map_err(|e| format!("非法 HTTP 方法: {e}"))?;
    // 初始请求主机：重定向只对同主机转发鉴权头（防跨主机泄密，见下方注释）。
    let orig_host = reqwest::Url::parse(&url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()));
    let mut req = client.request(http_method.clone(), &url).timeout(timeout);
    if let Some(h) = &headers {
        for (k, v) in h {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                req = req.header(name, value);
            }
        }
    }
    if let Some(b) = &body {
        req = req.body(b.clone());
    }
    // 发送前先查一次取消（防止用户在请求刚发起就点停止）
    if cancelled() {
        return Err("AI 请求已取消（用户点击停止）".into());
    }
    let resp = tokio::select! {
        r = req.send() => r,
        _ = async { while !cancelled() { tokio::time::sleep(Duration::from_millis(50)).await; } } => {
            return Err("AI 请求已取消（用户点击停止）".into());
        }
    }
    .map_err(|e| format!("AI 请求失败: {e}"))?;
    let mut resp = resp;
    // 手动跟随重定向：每个 Location 都过一遍私网校验，最多 5 跳防环。
    let mut hops = 0;
    while [301u16, 302, 303, 307, 308].contains(&resp.status().as_u16()) {
        hops += 1;
        if hops > 5 {
            return Err("AI 请求重定向次数过多（超过 5 跳）".to_string());
        }
        let loc = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| "AI 请求重定向缺少 Location 头".to_string())?
            .to_string();
        // Location 可能是相对路径，用当前 URL 拼接成绝对地址再校验。
        let next = resp
            .url()
            .join(&loc)
            .map_err(|e| format!("重定向目标不合法: {e}"))?;
        if let Some(blocked) = blocked_private_target(next.as_str()) {
            return Err(format!("AI 重定向目标不可用（{blocked}）：{}", next));
        }
        let hop_status = resp.status().as_u16();
        // 307/308 语义要求保留原方法与请求体；301/302/303 改写为 GET。
        let preserve_method = preserve_redirect_method(hop_status);
        let r2 = if preserve_method {
            client.request(http_method.clone(), next.as_str())
        } else {
            client.request(reqwest::Method::GET, next.as_str())
        };
        let mut r2 = r2.timeout(timeout);
        if preserve_method {
            if let Some(b) = &body {
                r2 = r2.body(b.clone());
            }
        }
        // 调用方的头（x-goog-api-key / Authorization 等鉴权头）只转发给
        // **最初请求的同一主机**（见 redirect_same_host 注释）。
        if redirect_same_host(orig_host.as_deref(), &next) {
            if let Some(h) = &headers {
                for (k, v) in h {
                    if let (Ok(name), Ok(value)) = (
                        reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                        reqwest::header::HeaderValue::from_str(v),
                    ) {
                        r2 = r2.header(name, value);
                    }
                }
            }
        } else {
            tracing::warn!(
                "ai_proxy 重定向跨主机（{} → {}），已丢弃调用方鉴权头",
                resp.url().host_str().unwrap_or("?"),
                next.host_str().unwrap_or("?")
            );
        }
        resp = r2.send().await.map_err(|e| format!("AI 请求失败: {e}"))?;
    }
    let status = resp.status().as_u16();
    // 响应体上限 10 MB：**流式边读边截**——原实现 `resp.bytes()` 先全量缓冲、
    // 读完才比大小，恶意/异常服务器可以用无限流先打爆内存再触发检查（与
    // web_search 的 fetch_limited 同一类坑，2026-10-11 修复）。超过上限立刻
    // 返回错误，任何时刻本进程缓冲不超过 10 MB + 单 chunk。
    const AI_MAX_BODY: usize = 10 * 1024 * 1024;
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("AI 响应读取失败: {e}"))?
    {
        if bytes.len() + chunk.len() > AI_MAX_BODY {
            return Err(format!(
                "AI 响应体过大（读取已超 {AI_MAX_BODY} bytes，上限 10 MB）"
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = String::from_utf8_lossy(&bytes).to_string();
    Ok(AiProxyResponse { status, body: text })
}

/// 判断该 30x 跳转是否必须保留原方法与请求体（RFC 9110：307/308 保留，
/// 301/302/303 按惯例改写为 GET）。抽纯函数便于单测锁定语义。
fn preserve_redirect_method(status: u16) -> bool {
    matches!(status, 307 | 308)
}

/// 重定向的下一跳是否与**最初请求同一主机**——只有同主机才转发调用方的头。
/// 跨主机 30x（含开放重定向 / 被劫持的 Location）会把 x-goog-api-key /
/// Authorization 等鉴权头送到任意外部服务器；reqwest 自带重定向在跨主机时
/// 同样会剥敏感头，这里关了自动重定向手动跟，必须等价补上
/// （2026-10-11 修复）。任一侧 host 缺失一律按跨主机处理（fail-closed）。
fn redirect_same_host(orig_host: Option<&str>, next: &reqwest::Url) -> bool {
    match (orig_host, next.host_str()) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

/// ai_proxy 取消守卫：Drop 时从 AppState.ai_cancels 移除 key（RAII，保证
/// 任何返回路径都清理，避免取消表无限膨胀）。
struct CancelGuard<'a> {
    state: &'a tauri::State<'a, crate::AppState>,
    key: String,
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        self.state.ai_cancels.lock().unwrap().remove(&self.key);
    }
}

/// 取消进行中的 AI 代理请求（前端「停止」按钮）：按 cancel_key 置对应 flag，
/// ai_proxy 的发送循环轮询到 true 即中断返回「已取消」。key 不存在 = 请求已
/// 结束/从未发起，安全空操作。
#[tauri::command]
pub(crate) fn ai_cancel(
    state: tauri::State<'_, crate::AppState>,
    cancel_key: String,
) -> Result<(), String> {
    let map = state.ai_cancels.lock().unwrap();
    if let Some(flag) = map.get(&cancel_key) {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    } else {
        Err(format!("没有进行中的 AI 请求（key={cancel_key}）"))
    }
}

#[cfg(test)]
#[path = "ai_tests.rs"]
mod tests;
