//! AI 联网搜索：免 key 网页搜索（Bing 直连解析 → DDG-lite 兜底）。
//!
//! 从 `ai.rs` 拆分（2026-10-12）：联网层独立成模块，代理层（SSRF 防护/
//! 手动重定向）留在 ai.rs。

use std::time::Duration;

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

/// 免 key 网页搜索（Bing → DDG-lite 双通道）。AI agent 循环专用。
#[tauri::command]
pub(crate) async fn web_search(query: String) -> Result<Vec<SearchHit>, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("搜索词为空".to_string());
    }
    if query.chars().count() > 500 {
        return Err("搜索词过长（上限 500 字符）".to_string());
    }
    let client = crate::net_http::shared_http();
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
