//! 检查更新：查询 GitHub Releases 最新 tag，与本地版本号比较。
//!
//! 本项目发布流程是「打 v* tag → GitHub Actions 出 Windows 安装包」，
//! 尚无签名/自动更新通道，所以这里只做「有新版 + 跳转下载页」的提示，
//! 不做静默升级。

use serde::Serialize;

/// 当前仓库（workspace Cargo.toml 的 repository）。
const REPO: &str = "lxfebd/diskpilot";

#[derive(Serialize)]
pub(crate) struct UpdateInfo {
    /// 本地版本（如 0.1.2）。
    pub(crate) current: String,
    /// 远端最新版本（去掉 tag 前缀 v）。
    pub(crate) latest: String,
    /// latest > current 时为 true。
    pub(crate) available: bool,
    /// 跳转的下载页。
    pub(crate) url: String,
    /// release notes 摘要（截断到 2000 字符，避免塞满设置面板）。
    pub(crate) notes: String,
}

/// 查询 GitHub Releases 最新版。网络失败 / 仓库无 release 时返回可读错误。
#[tauri::command]
pub(crate) async fn check_update() -> Result<UpdateInfo, String> {
    // 复用进程级共享客户端：带系统代理解析（企业/校园网必须走代理才能出网）。
    let client = crate::shared_http();
    let resp = client
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .send()
        .await
        .map_err(|e| format!("请求 GitHub 失败（请检查网络）：{e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("仓库还没有发布过正式版本".into());
    }
    if !resp.status().is_success() {
        return Err(format!(
            "GitHub 返回 {}（匿名限流时稍后再试）",
            resp.status()
        ));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析返回数据失败: {e}"))?;
    let tag = body["tag_name"].as_str().unwrap_or("").to_string();
    let latest = tag.trim_start_matches('v').to_string();
    let current = env!("CARGO_PKG_VERSION").to_string();
    let url = body["html_url"]
        .as_str()
        .unwrap_or("https://github.com/lxfebd/diskpilot/releases")
        .to_string();
    let notes: String = body["body"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(2000)
        .collect();
    Ok(UpdateInfo {
        current: current.clone(),
        available: version_cmp(&latest, &current) == std::cmp::Ordering::Greater,
        latest,
        url,
        notes,
    })
}

/// 轻量版本比较：按 `.` 分段、逐段数字比较；`-`/`+` 后的预发布后缀语义为
/// 「低于同数字段的正式版」（`0.3.0` > `0.3.0-beta.1`；同为预发布时按后缀
/// 数字段比较，`beta.2` > `beta.1`）。够用于「有没有新版」，不做语义化版本细则。
pub(crate) fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    // 数字段只看 `-`/`+` 之前的正式版本号——预发布后缀不得混入数值排位
    //（否则 `0.3.0-beta.1` 的尾部 `1` 会被当成比 `0.3.0` 更高的数字段）。
    let segs = |s: &str| -> Vec<u64> {
        s.split(['-', '+'])
            .next()
            .unwrap_or(s)
            .split('.')
            .map(|p| {
                p.trim()
                    .to_ascii_lowercase()
                    .trim_start_matches('v')
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    let prerelease = |s: &str| s.split(['-', '+']).nth(1).map(|x| x.to_string());
    let (sa, sb) = (segs(a), segs(b));
    for i in 0..sa.len().max(sb.len()) {
        let (x, y) = (
            sa.get(i).copied().unwrap_or(0),
            sb.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x.cmp(&y);
        }
    }
    // 数字段全等：预发布后缀（有 `-`/`+`）< 正式版（无后缀）。
    match (prerelease(a), prerelease(b)) {
        (None, None) => std::cmp::Ordering::Equal,
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(pa), Some(pb)) => {
            // 同为预发布：后缀逐段数字比较（`beta.2` > `beta.1`，字母段按 0 计）。
            let psegs = |p: &str| -> Vec<u64> {
                p.split('.')
                    .map(|x| {
                        x.chars()
                            .take_while(|c| c.is_ascii_digit())
                            .collect::<String>()
                            .parse()
                            .unwrap_or(0)
                    })
                    .collect()
            };
            let (pa, pb) = (psegs(&pa), psegs(&pb));
            for i in 0..pa.len().max(pb.len()) {
                let (x, y) = (
                    pa.get(i).copied().unwrap_or(0),
                    pb.get(i).copied().unwrap_or(0),
                );
                if x != y {
                    return x.cmp(&y);
                }
            }
            std::cmp::Ordering::Equal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_cmp_basic() {
        assert_eq!(version_cmp("0.1.2", "0.1.2"), std::cmp::Ordering::Equal);
        assert_eq!(version_cmp("0.2.0", "0.1.9"), std::cmp::Ordering::Greater);
        assert_eq!(version_cmp("0.1.9", "0.2.0"), std::cmp::Ordering::Less);
        assert_eq!(version_cmp("1.0.0", "0.9.9"), std::cmp::Ordering::Greater);
    }

    #[test]
    fn version_cmp_v_prefix_and_prerelease() {
        // tag 常带 v 前缀；预发布后缀语义为低于同数字段的正式版
        assert_eq!(version_cmp("v0.3.0", "0.3.0"), std::cmp::Ordering::Equal);
        assert_eq!(
            version_cmp("0.3.0-beta.1", "0.3.0"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            version_cmp("0.3.0", "0.3.0-beta.1"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            version_cmp("0.3.0-beta.1", "0.2.9"),
            std::cmp::Ordering::Greater
        );
        // 同为预发布：数字段定胜负
        assert_eq!(
            version_cmp("0.3.0-beta.2", "0.3.0-beta.1"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn version_cmp_uneven_lengths() {
        assert_eq!(version_cmp("0.1", "0.1.0"), std::cmp::Ordering::Equal);
        assert_eq!(version_cmp("0.1", "0.1.1"), std::cmp::Ordering::Less);
    }
}
