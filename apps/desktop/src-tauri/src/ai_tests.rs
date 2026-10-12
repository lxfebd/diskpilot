//! ai.rs 单元测试（从 ai.rs 拆分，2026-10-12）。
//! SSRF 私网判定 / 重定向语义 / CancelGuard 取消标记。

use super::ai_ssrf::{blocked_private_target, ipv6_is_private, parse_ipv4, parse_ipv6};
use super::{preserve_redirect_method, redirect_same_host};

/// 307/308 必须保留方法与请求体，301/302/303 按惯例降级为 GET。
/// 原实现把 preserve 判定内联在 ai_proxy 手动重定向环里（全部跳转都
/// 沿用首跳方法、301/302 也带 body），抽函数 + 本测试锁定语义
/// （2026-10-11 修复）。
#[test]
fn redirect_preserves_method_only_for_307_308() {
    assert!(preserve_redirect_method(307));
    assert!(preserve_redirect_method(308));
    assert!(!preserve_redirect_method(301));
    assert!(!preserve_redirect_method(302));
    assert!(!preserve_redirect_method(303));
    assert!(!preserve_redirect_method(200));
}

/// 重定向只对同主机转发调用方鉴权头（x-goog-api-key/Authorization）。
/// 跨主机（含 host 缺失）一律 false——fail-closed，密钥不外泄
/// （2026-10-11 修复：原实现跨主机无条件透传 caller 的全部头）。
#[test]
fn redirect_strips_auth_headers_cross_host() {
    let orig = Some("generativelanguage.googleapis.com");
    // 同主机（大小写不敏感）→ 转发
    assert!(redirect_same_host(
        orig,
        &reqwest::Url::parse("https://GENERATIVELANGUAGE.googleapis.com/v1beta/x").unwrap()
    ));
    assert!(redirect_same_host(
        orig,
        &reqwest::Url::parse("https://generativelanguage.googleapis.com:443/v1beta").unwrap()
    ));
    // 跨主机 → 剥头
    assert!(!redirect_same_host(
        orig,
        &reqwest::Url::parse("https://attacker.example/collect").unwrap()
    ));
    // 初始 host 缺失 / 下一跳 host 缺失 → fail-closed
    assert!(!redirect_same_host(
        None,
        &reqwest::Url::parse("https://a.example/x").unwrap()
    ));
    assert!(!redirect_same_host(
        Some("a.example"),
        &reqwest::Url::parse("data:text/plain,x").unwrap()
    ));
}

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
