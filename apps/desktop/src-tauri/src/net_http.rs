//! 共享 HTTP 客户端 + SSRF 安全的受控下载。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：共享 reqwest client（默认跟随重定向 /
//! 禁重定向两个变体）+ `ssrf_safe_get` 受控下载 helper（逐跳私网校验、
//! 响应体上限、进度回调）。

use std::sync::OnceLock;

/// SSRF 下载响应体上限（256 MB）。
pub(crate) const SSRF_MAX_BODY: u64 = 256 * 1024 * 1024;

static SHARED_HTTP: OnceLock<reqwest::Client> = OnceLock::new();
static SHARED_HTTP_NO_REDIRECT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client_base() -> reqwest::ClientBuilder {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut b = reqwest::Client::builder().user_agent(concat!(
        "DiskPilot/",
        env!("CARGO_PKG_VERSION"),
        " (图吧工具箱清理助手)"
    ));
    // 系统代理探测放在每次取 client 时做：改代理后首次调用即生效（懒加载），
    // 不缓存旧代理。Windows 走注册表读取；mac/linux 由 reqwest 默认尊重
    // HTTPS_PROXY 环境变量，这里不额外处理。
    #[cfg(windows)]
    {
        if let Some(proxy_str) = crate::net_proxy::system_https_proxy() {
            if let Ok(p) = reqwest::Proxy::all(&proxy_str) {
                b = b.proxy(p);
            }
        }
    }
    b
}

pub(crate) fn shared_http() -> &'static reqwest::Client {
    SHARED_HTTP.get_or_init(|| {
        // 默认 client：正常请求用，跟随重定向（plugin_registry_refresh 等场景
        // 需要）。SSRF 风险由每跳显式校验兜底（见 ssrf_safe_get）。
        http_client_base().build().expect("shared http client")
    })
}

pub(crate) fn http_no_redirect() -> &'static reqwest::Client {
    SHARED_HTTP_NO_REDIRECT.get_or_init(|| {
        http_client_base()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("no-redirect http client")
    })
}

/// SSRF 安全的 GET 下载：禁自动重定向 + 逐跳校验私网 + 响应体上限。
///
/// 插件/DiskPilot 语义区别于 AI 代理——这是「下载资产」而非「转发用户内容」，
/// 所以不做任意 host 的代理转发，只做受控 GET。默认共享 client 跟随重定向的
/// 「首跳合法公网 → 302 到内网」绕过在这里被堵死：每个 Location 都过一遍
/// `ai::blocked_private_target`（与 AI 代理同一把尺），最多 5 跳防环。
/// 返回 200 响应体；失败删除半截下载物（调用方传 out 时由调用方负责，便于
/// 渐进式进度上报——本 helper 只负责网络层，落盘由调用方做）。
///
/// `on_chunk` 可选回调（下载进度上报）。
pub(crate) async fn ssrf_safe_get(
    url: &str,
    max_bytes: u64,
    mut on_chunk: Option<Box<dyn FnMut(u64, u64) + Send>>,
) -> Result<Vec<u8>, String> {
    let parsed = url::Url::parse(url.trim()).map_err(|e| format!("URL 非法：{e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!(
            "下载地址必须使用 https（当前协议：{}）。为防中间人篡改，http 与自定义协议一律拒绝。",
            parsed.scheme()
        ));
    }
    // R505：字面量 + DNS 二次校验（域名解析到私网同样拒绝）。
    if let Some(blocked) = crate::ai::ssrf_block_reason_async(parsed.as_str()).await {
        return Err(format!("下载目标不可用（{blocked}）：{parsed}"));
    }
    let client = http_no_redirect();
    let mut url = parsed.as_str().to_string();
    let mut hops = 0;
    loop {
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("下载失败（网络错误)：{e}"))?;
        let status = resp.status();
        if status.is_redirection() {
            let loc = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| "下载重定向缺少 Location 头".to_string())?
                .to_string();
            let next = resp
                .url()
                .join(&loc)
                .map_err(|e| format!("重定向目标不合法: {e}"))?;
            hops += 1;
            if hops > 5 {
                return Err("下载重定向次数过多（超过 5 跳）".to_string());
            }
            // R505：重定向每跳同样走 DNS 二次校验。
            if let Some(blocked) = crate::ai::ssrf_block_reason_async(next.as_str()).await {
                return Err(format!("下载重定向目标不可用（{blocked}）：{next}"));
            }
            url = next.to_string();
            continue;
        }
        if !status.is_success() {
            return Err(format!(
                "下载失败：HTTP {}（{}）",
                status.as_u16(),
                status.canonical_reason().unwrap_or("")
            ));
        }
        let total = resp.content_length().unwrap_or(0);
        if total > max_bytes {
            return Err(format!(
                "响应体过大（{} bytes，上限 {} MB）",
                total,
                max_bytes / (1024 * 1024)
            ));
        }
        let mut body = Vec::new();
        let mut downloaded: u64 = 0;
        let mut stream = resp;
        while let Some(chunk) = stream.chunk().await.map_err(|e| format!("下载中断：{e}"))? {
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            if downloaded > max_bytes {
                return Err(format!(
                    "响应体超过上限（{} MB），已中断",
                    max_bytes / (1024 * 1024)
                ));
            }
            if let Some(cb) = on_chunk.as_mut() {
                cb(downloaded, total);
            }
            body.extend_from_slice(&chunk);
        }
        return Ok(body);
    }
}
