//! 外链打开：`<a href="https://…" target="_blank">` 在桌面端点不动。
//!
//! Tauri/WebView2 只有在应用显式注册了 `on_new_window` 时才处理新窗口请求——
//! wry 0.55 的 `webview2/mod.rs` 里，没有 handler 就直接 `args.SetHandled(true)`
//! 把请求取消（既不开新窗、也不交给系统浏览器）。本应用没有注册过，所以免责
//! 声明里的 GitHub、设置里的更新下载、工具箱的教程/下载、插件市场的主页，点了
//! 全都没反应。这里统一交给系统默认浏览器打开，与 `steam.rs` 的 `open_steam_url`
//! 同一条通道（spawn 系统打开器，不经 shell 解释）。

/// 允许交给系统打开器处理的协议：网页、邮件、Steam 商店。
///
/// 其它协议一律拒绝——URL 由前端拼接，不做白名单等于把 webview 变成一个
/// 「任意协议执行器」（`file:`、`ms-settings:`、`javascript:` 之类）。
pub(crate) fn url_allowed(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    if u.is_empty() || u.contains('\n') || u.contains('\r') {
        return false;
    }
    ["http://", "https://", "mailto:", "steam://"]
        .iter()
        .any(|p| u.starts_with(p))
}

#[tauri::command]
pub(crate) fn open_external_url(url: String) -> Result<(), String> {
    if !url_allowed(&url) {
        return Err(format!("unsupported url: {url}"));
    }
    let url = url.trim();
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::url_allowed;

    #[test]
    fn allows_web_mail_and_steam() {
        assert!(url_allowed("https://github.com/lxfebd"));
        assert!(url_allowed("http://example.com/a?b=1#c"));
        assert!(url_allowed("mailto:3167245951@qq.com"));
        assert!(url_allowed("steam://rungameid/730"));
    }

    #[test]
    fn trims_and_ignores_case() {
        assert!(url_allowed("  HTTPS://Example.COM/x  "));
        assert!(url_allowed("MailTo:a@b.c"));
    }

    #[test]
    fn rejects_other_schemes() {
        assert!(!url_allowed(""));
        assert!(!url_allowed("   "));
        assert!(!url_allowed("file:///C:/Windows/System32/calc.exe"));
        assert!(!url_allowed("javascript:alert(1)"));
        assert!(!url_allowed("ms-settings:privacy"));
        assert!(!url_allowed("C:\\Windows\\System32\\cmd.exe"));
        assert!(!url_allowed("https://a\r\nfile:///x"));
    }
}
