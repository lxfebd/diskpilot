//! AI 联网搜索 / 请求代理（Tauri 命令层）。

use std::collections::HashMap;
use std::time::Duration;

use crate::{http_no_redirect, shared_http};

// ── AI 联网 ──────────────────────────────────────────────────────────────
// 给 AI agent 循环用的免 key 网页搜索：直连 Bing(cn) HTML 解析自然结果，
// 失败再退 DDG-lite。手动字符串扫描（不想为这一个命令引 regex crate）。
// 与上面 Steam 抓取同样走系统代理，保证代理网络下也能通。

#[derive(serde::Serialize)]
pub(crate) struct SearchHit {
    title: String,
    url: String,
    snippet: String,
}

fn html_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&#38;", "&")
        .replace("&#x2F;", "/")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(ch) = it.next() {
        if ch == '<' {
            for c in it.by_ref() {
                if c == '>' {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Find `attr` (e.g. `href="`) at/after `from`; return (value, index just
/// after the enclosing tag's `>`).
fn attr_value(s: &str, from: usize, attr: &str) -> Option<(String, usize)> {
    let i = s[from..].find(attr)? + from;
    let vstart = i + attr.len();
    let vend = s[vstart..].find('"')? + vstart;
    let gt = s[vend..].find('>')? + vend + 1;
    Some((html_decode(&s[vstart..vend]), gt))
}

/// Text from after `tag_end` up to `end_marker`.
fn text_between(s: &str, tag_end: usize, end_marker: &str) -> Option<String> {
    let rel = s[tag_end..].find(end_marker)? + tag_end;
    Some(html_decode(&strip_tags(&s[tag_end..rel])))
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}", b = b)),
        }
    }
    out
}

fn bing_parse(html: &str, max: usize) -> Vec<SearchHit> {
    let mut out = Vec::new();
    for chunk in html.split("<li class=\"b_algo").skip(1) {
        if out.len() >= max {
            break;
        }
        let h2 = match chunk.find("<h2") {
            Some(i) => i,
            None => continue,
        };
        let seg = &chunk[h2..];
        let (url, gt) = match attr_value(seg, 0, "href=\"") {
            Some(v) => v,
            None => continue,
        };
        let title = match text_between(seg, gt, "</a>") {
            Some(t) if !t.trim().is_empty() => t,
            _ => continue,
        };
        // Snippet：锚点之后第一个 <p>…</p>（b_caption 里）。拿不到就留空。
        let snippet = match seg[gt..].find("<p") {
            Some(pr) => {
                let pgt = gt + pr + seg[gt + pr..].find('>').unwrap_or(0) + 1;
                text_between(seg, pgt, "</p>").unwrap_or_default()
            }
            None => String::new(),
        };
        out.push(SearchHit {
            title,
            url,
            snippet,
        });
    }
    out
}

fn ddg_lite_parse(html: &str, max: usize) -> Vec<SearchHit> {
    let mut out = Vec::new();
    for chunk in html.split("<a rel=\"nofollow\"").skip(1) {
        if out.len() >= max {
            break;
        }
        let (url, gt) = match attr_value(chunk, 0, "href=\"") {
            Some(v) => v,
            None => continue,
        };
        let title = match text_between(chunk, gt, "</a>") {
            Some(t) if !t.trim().is_empty() => t,
            _ => continue,
        };
        // DDG-lite 的摘要是 <div class='snippet_content...>文本</div>（单引号属性，
        // 不能用 attr_value，直接找本标签的 > 再截到 </div>）。
        let snippet = match chunk.find("snippet_content") {
            Some(si) => match chunk[si..].find('>') {
                Some(k) => text_between(chunk, si + k + 1, "</div>").unwrap_or_default(),
                None => String::new(),
            },
            None => String::new(),
        };
        // DDG 的跳转链接是 /l/?uddg=…&rdg= 包装，取不出原链就留原样。
        out.push(SearchHit {
            title,
            url,
            snippet,
        });
    }
    out
}

#[tauri::command]
pub(crate) async fn web_search(query: String) -> Result<Vec<SearchHit>, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("搜索词为空".to_string());
    }
    if query.chars().count() > 500 {
        return Err("搜索词过长（上限 500 字符）".to_string());
    }
    let client = shared_http();
    // 搜索页是静态 HTML，8s 足够；超长响应只取前 2 MB（网页全文解析用不到更多）。
    const SEARCH_TIMEOUT: Duration = Duration::from_secs(8);
    const SEARCH_MAX_BYTES: usize = 2 * 1024 * 1024;

    // ① Bing（国内可直连，中文结果质量高）
    let last_err: String;
    let bing_url = format!(
        "https://www.bing.com/search?q={}&setmkt=zh-cn&count=8",
        url_encode(&query)
    );
    match client.get(&bing_url).timeout(SEARCH_TIMEOUT).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                match fetch_limited(resp, SEARCH_MAX_BYTES).await {
                    Ok(html) => {
                        let hits = bing_parse(&html, 8);
                        if !hits.is_empty() {
                            tracing::info!("web_search(bing): {} hits for {:?}", hits.len(), query);
                            return Ok(hits);
                        }
                        last_err = "Bing 返回了页面但没解析出结果".to_string();
                    }
                    Err(e) => last_err = format!("Bing 页面读取失败: {e}"),
                }
            } else {
                last_err = format!("Bing 返回 HTTP {}", resp.status());
            }
        }
        Err(e) => last_err = format!("Bing 请求失败: {e}"),
    }

    // ② DuckDuckGo lite（备用）
    let ddg_url = format!("https://lite.duckduckgo.com/?q={}", url_encode(&query));
    match client.get(&ddg_url).timeout(SEARCH_TIMEOUT).send().await {
        Ok(resp) if resp.status().is_success() => {
            let html = fetch_limited(resp, SEARCH_MAX_BYTES)
                .await
                .map_err(|e| format!("DDG 页面读取失败: {e}"))?;
            let hits = ddg_lite_parse(&html, 8);
            if hits.is_empty() {
                Err(format!("联网搜索失败：{last_err}；DDG 也没解析出结果"))
            } else {
                tracing::info!("web_search(ddg): {} hits for {:?}", hits.len(), query);
                Ok(hits)
            }
        }
        Ok(resp) => Err(format!(
            "联网搜索失败：{last_err}；DDG 返回 HTTP {}",
            resp.status()
        )),
        Err(e) => Err(format!("联网搜索失败：{last_err}；DDG 请求失败: {e}")),
    }
}

/// 读取响应体但限制大小：超过 max_bytes 截断（网页全文解析用不到更多，
/// 防止恶意/异常服务器用无限响应拖垮本进程）。
async fn fetch_limited(mut resp: reqwest::Response, max_bytes: usize) -> Result<String, String> {
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("读取响应失败: {e}"))?
    {
        if bytes.len() + chunk.len() > max_bytes {
            bytes.extend_from_slice(&chunk[..max_bytes.saturating_sub(bytes.len())]);
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

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

    let mut req = client
        .request(
            reqwest::Method::from_bytes(method.as_deref().unwrap_or("POST").as_bytes())
                .map_err(|e| format!("非法 HTTP 方法: {e}"))?,
            &url,
        )
        .timeout(timeout);
    if let Some(h) = &headers {
        for (k, v) in h {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(&v),
            ) {
                req = req.header(name, value);
            }
        }
    }
    if let Some(b) = body {
        req = req.body(b);
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
        let mut r2 = client.get(next.as_str()).timeout(timeout);
        // 跟随重定向时把 caller 的头一并带过去（AI 服务商常要求鉴权头不丢）。
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
        resp = r2.send().await.map_err(|e| format!("AI 请求失败: {e}"))?;
    }
    let status = resp.status().as_u16();
    // 响应体上限 10 MB：AI 接口的合法响应远小于此，防止恶意/异常服务器
    // 用无限响应把本进程内存打爆。
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("AI 响应读取失败: {e}"))?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(format!(
            "AI 响应体过大（{} bytes，上限 10 MB）",
            bytes.len()
        ));
    }
    let text = String::from_utf8_lossy(&bytes).to_string();
    Ok(AiProxyResponse { status, body: text })
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

/// 判断 URL 是否指向私网/环回/链路本地地址（SSRF 攻击面）。
///
/// 只按字面量解析 host：`127.0.0.1`、`localhost`、`10.x`、`172.16-31.x`、
/// `192.168.x`、`169.254.x`、`0.0.0.0`、`[::1]`、`[fc00::/7]`、`[fe80::/10]`、
/// v4-mapped `[::ffff:127.0.0.1]` 等直接拒绝；十进制/八进制/十六进制 IP 变体
/// （`2130706433`、`0177.0.0.1`、`0x7f.0.0.1`、`127.1`）也识别并拒绝。
/// 域名字面量不在私网段内的放行（不做 DNS 反查，避免代理 DNS 时被重绑定绕过）。
/// pub(crate)：插件下载（plugin_remote / plugin_registry / scaffold_registry）
/// 复用同一套私网判定，保证「所有出网请求同一把尺」。
pub(crate) fn blocked_private_target(url: &str) -> Option<&'static str> {
    let host = url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.find('/').map(|i| &rest[..i]).or(Some(rest)))
        .unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or("").trim();
    // 去端口：方括号 IPv6 取 `[...]` 内内容，普通 host 取第一个 `:` 之前
    let hostname = if host.starts_with('[') {
        host.find(']').map(|i| &host[..=i]).unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or("")
    };
    let lower = hostname
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase();
    if lower == "localhost" || lower == "::1" {
        return Some("localhost");
    }
    // IPv6 检查（含 v4-mapped / v4-compatible / 内嵌 IPv4 的形式）
    if lower.contains(':') {
        if let Some(ip) = parse_ipv6(&lower) {
            if ipv6_is_private(&ip) {
                return Some("private/loopback address");
            }
        }
        return None;
    }
    if let Some(ip) = parse_ipv4(&lower) {
        let [a, b, _, _] = ip;
        let is_private = a == 0
            || a == 10
            || (a == 127)
            || (a == 169 && b == 254)
            || (a == 172 && (16..=31).contains(&b))
            || (a == 192 && b == 168)
            || (a == 100 && (64..=127).contains(&b));
        if is_private {
            return Some("private/loopback address");
        }
    }
    None
}

/// 极简 IPv6 解析（SSRF 用）：支持 `::1`、`fc00::/7`、`fe80::/10`、
/// v4-mapped `::ffff:1.2.3.4`、内嵌 IPv4 `::127.0.0.1`。返回 16 字节
/// 大端地址；解析不出返回 None（域名/非法，交由上层处理）。
fn parse_ipv6(s: &str) -> Option<[u8; 16]> {
    let s = s.trim();
    if s.is_empty() || s.len() > 64 {
        return None;
    }
    // 每个 u16 段的 16-bit 值，含 v4 末段拆出的两个值
    let mut segs: Vec<u16> = Vec::new();
    // 是否出现过 `::`（出现过则整体必须能压缩到 ≤16 字节）
    let (head, tail) = match s.split_once("::") {
        Some((h, t)) => (h, t),
        None => (s, ""),
    };
    let has_compress = s.contains("::");
    let push_seg = |seg: &str, out: &mut Vec<u16>| -> Option<()> {
        if seg.contains('.') {
            let v4 = parse_ipv4(seg)?;
            out.push(((v4[0] as u16) << 8) | v4[1] as u16);
            out.push(((v4[2] as u16) << 8) | v4[3] as u16);
        } else {
            let v = u16::from_str_radix(seg, 16).ok()?;
            out.push(v);
        }
        Some(())
    };
    for seg in head.split(':').filter(|x| !x.is_empty()) {
        push_seg(seg, &mut segs)?;
    }
    if !tail.is_empty() {
        let mut tail_segs: Vec<u16> = Vec::new();
        for seg in tail.split(':').filter(|x| !x.is_empty()) {
            push_seg(seg, &mut tail_segs)?;
        }
        if has_compress {
            // 压缩：head 在前、tail 顶到末尾，中间补零
            let head_len = segs.len();
            let total = head_len + tail_segs.len();
            if total > 8 {
                return None;
            }
            // RFC 4291：`::` 必须至少压缩一个 16-bit 组——head+tail 凑满 8 段
            // 且不是纯 `::`（`::` 表示全零）时，压缩 0 段是非法地址。
            if total == 8 && !(head.is_empty() && tail.is_empty()) {
                return None;
            }
            // 直接构造最终段序列
            segs = {
                let mut all = vec![0u16; 8];
                if head_len > 0 {
                    all[..head_len].copy_from_slice(&segs);
                }
                let tail_start = 8 - tail_segs.len();
                if !tail_segs.is_empty() {
                    all[tail_start..].copy_from_slice(&tail_segs);
                }
                all
            };
        } else {
            // 无压缩但出现 head+tail（不会发生——无 :: 时 tail 必空）
            segs.extend(tail_segs);
        }
    } else if has_compress {
        // `::` 在末尾（如 `fe80::`）：head 后补零到 8 段
        if segs.len() > 8 {
            return None;
        }
        segs.resize(8, 0);
    }
    // 无压缩时必须是完整 8 段
    if !has_compress && segs.len() != 8 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, v) in segs.iter().take(8).enumerate() {
        out[i * 2] = (v >> 8) as u8;
        out[i * 2 + 1] = *v as u8;
    }
    Some(out)
}

/// IPv6 私网判定：::1 环回、fc00::/7（ULA）、fe80::/10（链路本地）、
/// ::ffff:0:0/96（v4-mapped）内嵌的 IPv4 再按 v4 私网规则判。
fn ipv6_is_private(ip: &[u8; 16]) -> bool {
    // ::1
    if ip[..15] == [0u8; 15] && ip[15] == 1 {
        return true;
    }
    // fc00::/7（ULA）
    if (ip[0] & 0xfe) == 0xfc {
        return true;
    }
    // fe80::/10（链路本地）
    if (ip[0] & 0xff) == 0xfe && (ip[1] & 0xc0) == 0x80 {
        return true;
    }
    // ::ffff:0:0/96 v4-mapped：内嵌 IPv4 按 v4 私网规则判
    if ip[..10] == [0u8; 10] && ip[10] == 0xff && ip[11] == 0xff {
        let [a, b, c, d] = [ip[12], ip[13], ip[14], ip[15]];
        return ipv4_is_private(a, b, c, d);
    }
    // ::/96 v4-compatible（已废弃但仍是内网 IPv4 的映射）
    if ip[..12] == [0u8; 12] {
        return ipv4_is_private(ip[12], ip[13], ip[14], ip[15]);
    }
    false
}

/// IPv4 私网判定（0.x / 10.x / 127.x / 169.254.x / 172.16-31.x / 192.168.x / 100.64-127.x）。
fn ipv4_is_private(a: u8, b: u8, _c: u8, _d: u8) -> bool {
    a == 0
        || a == 10
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 100 && (64..=127).contains(&b))
}

/// 极简 IPv4 解析（SSRF 用）：`1.2.3.4` → [1,2,3,4]，非法返回 None。
/// 同时识别 WHATWG URL 解析器接受的整型变体（与 reqwest 用的 url crate 同语义，
/// 否则 `http://2130706433/` 这类会被 url crate 归一化为 127.0.0.1 而这里放行）：
/// - 十进制整数：`2130706433` = 127.0.0.1
/// - 八进制：`0177.0.0.1`、`017700000001`
/// - 十六进制：`0x7f.0.0.1`、`0x7f000001`
/// - 短写：`127.1` = 127.0.0.1（缺段从左补 0，首段之后是低位——WHATWG 语义
///   `192.168` = 192.0.0.168，不是 192.168.0.0）
fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let s = s.trim();
    if s.is_empty() || s.len() > 32 {
        return None;
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 4 {
        return None;
    }
    let mut out = [0u8; 4];
    // 前 parts.len()-1 段按顺序放高位字节
    for idx in 0..parts.len().saturating_sub(1) {
        let v: u64 = parse_ip_part(parts[idx])?;
        if v > 255 {
            return None;
        }
        out[idx] = v as u8;
    }
    // 末段覆盖剩余低位字节（WHATWG 语义：128.1 → 128.0.0.1）
    let last = parts.last()?;
    let first_n = parts.len() - 1;
    let low_bytes = 4 - first_n;
    let v = parse_ip_part(last)?;
    if v >= 256u64.pow(low_bytes as u32) {
        return None;
    }
    for i in 0..low_bytes {
        let shift = (low_bytes - 1 - i) as u32;
        out[first_n + i] = ((v >> (8 * shift)) & 0xff) as u8;
    }
    Some(out)
}

/// 解析单个 IPv4 数字段：十进制 / 0x 十六进制 / 0 前缀八进制，上限 u32。
fn parse_ip_part(part: &str) -> Option<u64> {
    let part = part.trim();
    if part.is_empty() || part.len() > 16 {
        return None;
    }
    // 前缀 0x / 0X → 十六进制
    if let Some(hex) = part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
        if hex.is_empty() || hex.len() > 8 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        return u64::from_str_radix(hex, 16).ok();
    }
    // 前导 0（且长度 > 1）→ 八进制
    if part.len() > 1 && part.starts_with('0') {
        if !part.chars().all(|c| ('0'..='7').contains(&c)) {
            return None;
        }
        return u64::from_str_radix(part.trim_start_matches('0'), 8)
            .ok()
            .or(Some(0)); // 全 0 的情况
    }
    part.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::{blocked_private_target, ipv6_is_private, parse_ipv4, parse_ipv6};

    #[test]
    fn ssrf_blocks_loopback_and_private_targets() {
        // 环回 / localhost
        assert_eq!(
            blocked_private_target("http://127.0.0.1:8080/x"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("https://localhost:11434"),
            Some("localhost")
        );
        assert_eq!(blocked_private_target("http://[::1]/x"), Some("localhost"));
        assert_eq!(
            blocked_private_target("http://[::1]:8080/x"),
            Some("localhost")
        );
        // 私网段
        assert_eq!(
            blocked_private_target("http://10.0.0.1/x"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://172.16.0.1/x"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://172.31.255.255/x"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://192.168.1.1/x"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://169.254.169.254/latest/meta-data/"),
            Some("private/loopback address")
        );
        // 保留段 0.x
        assert_eq!(
            blocked_private_target("http://0.0.0.0/x"),
            Some("private/loopback address")
        );
        // CGNAT 100.64/10
        assert_eq!(
            blocked_private_target("http://100.64.0.1/x"),
            Some("private/loopback address")
        );
        // IPv6 私网 / ULA / 链路本地 / v4-mapped
        assert_eq!(
            blocked_private_target("http://[fc00::1]/x"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://[fe80::1]/x"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://[::ffff:127.0.0.1]/x"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://[::ffff:192.168.1.1]/x"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://[::127.0.0.1]/x"),
            Some("private/loopback address")
        );
    }

    #[test]
    fn ssrf_blocks_ip_variant_encodings() {
        // 十进制整数 / 八进制 / 十六进制 / 短写 —— WHATWG url 解析器接受的
        // 变体，reqwest 会真正连到这些地址，必须拦（否则 SSRF 可绕过）。
        assert_eq!(
            blocked_private_target("http://2130706433/"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://0177.0.0.1/"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://017700000001/"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://0x7f.0.0.1/"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://0x7f000001/"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://127.1/"),
            Some("private/loopback address")
        );
        // 变体但落在公网段 → 放行
        assert_eq!(blocked_private_target("http://8.8.8.8/"), None);
        assert_eq!(blocked_private_target("http://0x08080808/"), None);
        assert_eq!(blocked_private_target("http://134744072/"), None); // 8.8.8.8 十进制
    }

    #[test]
    fn ssrf_allows_public_targets() {
        assert_eq!(
            blocked_private_target("https://api.openai.com/v1/chat/completions"),
            None
        );
        assert_eq!(
            blocked_private_target("https://www.bing.com/search?q=a"),
            None
        );
        assert_eq!(blocked_private_target("http://8.8.8.8/dns"), None);
        assert_eq!(
            blocked_private_target("https://example.com:8443/path"),
            None
        );
        // 域名字面量不在私网段内的放行（不做 DNS 反查）
        assert_eq!(
            blocked_private_target("https://myserver.internal.example.com/"),
            None
        );
        // 公网 IPv6 放行
        assert_eq!(
            blocked_private_target("http://[2606:4700:4700::1111]/"),
            None
        );
        assert_eq!(blocked_private_target("http://[::ffff:8.8.8.8]/"), None);
    }

    #[test]
    fn ssrf_handles_userinfo_and_port_stripping() {
        // userinfo 里夹私网 IP 不应绕过 host 检测
        assert_eq!(
            blocked_private_target("http://evil@127.0.0.1/x"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://192.168.1.1:9000/x"),
            Some("private/loopback address")
        );
        assert_eq!(
            blocked_private_target("http://127.0.0.1"),
            Some("private/loopback address")
        );
    }

    #[test]
    fn parse_ipv4_accepts_valid_and_rejects_malformed() {
        assert_eq!(parse_ipv4("1.2.3.4"), Some([1, 2, 3, 4]));
        assert_eq!(parse_ipv4("8.8.8.8"), Some([8, 8, 8, 8]));
        assert_eq!(parse_ipv4("255.255.255.255"), Some([255, 255, 255, 255]));
        assert_eq!(parse_ipv4("1.2.3.4.5"), None);
        assert_eq!(parse_ipv4("256.1.1.1"), None);
        assert_eq!(parse_ipv4("a.b.c.d"), None);
        assert_eq!(parse_ipv4(""), None);
        // WHATWG 变体
        assert_eq!(parse_ipv4("2130706433"), Some([127, 0, 0, 1]));
        assert_eq!(parse_ipv4("0x7f000001"), Some([127, 0, 0, 1]));
        assert_eq!(parse_ipv4("017700000001"), Some([127, 0, 0, 1]));
        assert_eq!(parse_ipv4("127.1"), Some([127, 0, 0, 1]));
        assert_eq!(parse_ipv4("192.168"), Some([192, 0, 0, 168])); // WHATWG 短写语义
        assert_eq!(parse_ipv4("0177.0.0.1"), Some([127, 0, 0, 1])); // 八进制段
    }

    #[test]
    fn parse_ipv6_and_private_detection() {
        // 环回
        assert_eq!(
            parse_ipv6("::1"),
            Some([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])
        );
        assert!(ipv6_is_private(&parse_ipv6("::1").unwrap()));
        // ULA fc00::/7
        assert!(ipv6_is_private(&parse_ipv6("fc00::1").unwrap()));
        assert!(ipv6_is_private(&parse_ipv6("fd12:3456:789a::1").unwrap()));
        // 链路本地 fe80::/10
        assert!(ipv6_is_private(&parse_ipv6("fe80::1").unwrap()));
        assert!(ipv6_is_private(&parse_ipv6("fe80::a1").unwrap()));
        // v4-mapped / v4-compatible
        assert!(ipv6_is_private(&parse_ipv6("::ffff:127.0.0.1").unwrap()));
        assert!(ipv6_is_private(&parse_ipv6("::ffff:192.168.1.1").unwrap()));
        assert!(ipv6_is_private(&parse_ipv6("::127.0.0.1").unwrap()));
        // 公网放行
        assert!(!ipv6_is_private(
            &parse_ipv6("2606:4700:4700::1111").unwrap()
        ));
        assert!(!ipv6_is_private(&parse_ipv6("::ffff:8.8.8.8").unwrap()));
        // 非法
        assert_eq!(parse_ipv6(""), None);
        assert_eq!(parse_ipv6("g::1"), None);
        assert_eq!(parse_ipv6("::1:2:3:4:5:6:7:8"), None); // 超 8 段
    }
}
