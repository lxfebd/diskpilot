//! 社区 scaffold 仓库（R8，2026-09-10）：本地索引 + 一键拉取安装。
//!
//! 对齐插件社区注册表（plugin_registry.rs）模式：
//! - 索引文件 `app_data_dir/scaffolds-registry.json`（可导入替换），
//!   无文件时用内置种子索引（官方清理脚本占位，无 url 则按钮禁用）。
//! - 列表是只读 L0；安装走 `scaffold.manage`（L2）+ confirmed 双硬校验，
//!   且**强制 Ed25519 签名**：索引条目带 `signature` + `signer`（hex），
//!   签名对象 = toml 内容（与插件包验签同一算法，见 toolbelt::signature）。
//!   没有完整签名的一律拒绝（未签名脚本只能本地粘贴安装）。
//! - 下载 url 强制 https（拒绝 http / 任意协议）。
//! - 安装核心复用 lib.rs `install_scaffold_inner`：id 校验 → 运行时红线
//!   校验 → 写入用户目录 → 热重载。绝不在安装路径放宽红线。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

/// 索引条目：一份可安装的社区清理脚本。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct RegistryScaffold {
    /// 合法 kebab-case scaffold id（安装管线同一身份契约）。
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub risk: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 脚本 toml 下载地址（必须 https，否则前端禁用安装按钮）。
    #[serde(default)]
    pub url: String,
    /// 期望 sha256（hex，可选；下载后校验）。
    #[serde(default)]
    pub sha256: String,
    /// Ed25519 签名（hex，对 toml 内容签名；缺失 = 未签名，远程安装拒绝）。
    #[serde(default)]
    pub signature: String,
    /// 签名者公钥（hex，32 字节）。
    #[serde(default)]
    pub signer: String,
    /// 下载次数（展示用，本地计数）。
    #[serde(default)]
    pub downloads: u64,
}

/// 内置种子索引：官方清理脚本占位。无 url 的条目前端显示「即将上架」且禁用安装。
fn seed_registry() -> Vec<RegistryScaffold> {
    vec![
        RegistryScaffold {
            id: "qq-pc-community".into(),
            name: "QQ PC 聊天缓存".into(),
            version: "1.0.0".into(),
            author: "DiskPilot 官方".into(),
            description: "QQ 桌面版聊天图片 / 文件 / 视频缓存清理（走回收站可还原）。".into(),
            risk: "low".into(),
            tags: vec!["缓存".into(), "回收站".into()],
            url: String::new(),
            ..Default::default()
        },
        RegistryScaffold {
            id: "browser-caches".into(),
            name: "浏览器缓存清理".into(),
            version: "1.0.0".into(),
            author: "DiskPilot 官方".into(),
            description: "Edge / Chrome 等 Chromium 系浏览器缓存清理（Cookie / 登录态不碰）。"
                .into(),
            risk: "low".into(),
            tags: vec!["缓存".into(), "浏览器".into()],
            url: String::new(),
            ..Default::default()
        },
    ]
}

fn registry_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    Some(dir.join("scaffolds-registry.json"))
}

/// 读注册表索引（无文件时返回种子索引；解析失败回退种子 + 告警字段）。
fn load_registry(app: &AppHandle) -> (Vec<RegistryScaffold>, Option<String>) {
    let Some(p) = registry_path(app) else {
        return (
            seed_registry(),
            Some("无法定位数据目录，使用内置种子索引".into()),
        );
    };
    if !p.is_file() {
        return (seed_registry(), None);
    }
    match std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<RegistryScaffold>>(&s).ok())
    {
        Some(v) if !v.is_empty() => (v, None),
        _ => (
            seed_registry(),
            Some("注册表索引损坏或为空，已回退内置种子索引".into()),
        ),
    }
}

/// 社区脚本仓库列表（只读 L0，不需要权限）。
#[tauri::command]
pub(crate) fn scaffold_registry_list(app: AppHandle) -> serde_json::Value {
    let (items, warn) = load_registry(&app);
    serde_json::json!({
        "items": items,
        "warning": warn,
        "updated_at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

/// 从社区仓库安装指定 id：校验条目存在 + url 是 https + 强制 Ed25519 签名
/// → 下载 toml → 复用 install_scaffold_inner 落盘。写操作需 confirmed=true +
/// `scaffold.manage` 权限（与插件安装同款纵深防御）。
#[tauri::command]
pub(crate) async fn scaffold_registry_install(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    confirmed: Option<bool>,
) -> Result<String, String> {
    if confirmed != Some(true) {
        return Err(
            "scaffold:confirm: 从社区仓库下载并安装清理脚本会写入用户脚本目录，必须由用户明确确认（confirmed=true）。"
                .into(),
        );
    }
    if !state
        .perm_grants
        .lock()
        .unwrap()
        .contains("scaffold.manage")
    {
        return Err(
            "scaffold:denied: 未开启「管理社区清理脚本」权限（scaffold.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }

    let (items, _) = load_registry(&app);
    let item = items
        .iter()
        .find(|p| p.id == id.trim())
        .ok_or_else(|| format!("社区仓库里没有脚本「{id}」"))?;
    if item.url.trim().is_empty() {
        return Err(format!("脚本「{}」尚未提供下载地址（即将上架）", item.name));
    }
    let parsed =
        url::Url::parse(item.url.trim()).map_err(|e| format!("脚本下载地址不是合法 URL：{e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!(
            "脚本「{}」的下载地址必须使用 https（当前：{}）。为防中间人篡改，http 与自定义协议一律拒绝。",
            item.name,
            parsed.scheme()
        ));
    }

    // SSRF 安全下载（禁重定向跟随 + 逐跳私网校验 + 响应体上限 1MB：脚本 toml 远小于此）。
    let body = crate::ssrf_safe_get(item.url.trim(), 1024 * 1024, None).await?;
    let toml = String::from_utf8_lossy(&body).to_string();
    if toml.trim().is_empty() {
        return Err("下载到的脚本内容为空".into());
    }

    // 可选 sha256 校验（索引声明了才查）。
    if !item.sha256.trim().is_empty() {
        let actual = diskpilot_toolbelt::signature::sha256_hex(&[(
            "scaffold.toml".to_string(),
            toml.as_bytes().to_vec(),
        )]);
        let expect = item.sha256.trim().to_ascii_lowercase();
        if actual != expect {
            return Err(format!(
                "脚本 sha256 校验失败：期望 {expect}，实际 {actual}（内容与索引不符，可能被篡改）"
            ));
        }
    }

    // 强制 Ed25519 签名：签名对象 = toml 内容（单文件 entries）。
    // 索引里没有完整 signature+signer = 未签名 → 远程安装一律拒绝。
    if item.signature.trim().is_empty() || item.signer.trim().is_empty() {
        return Err(
            "社区脚本必须带 Ed25519 签名（索引条目需含 signature + signer）。\
             未签名脚本只能通过本地粘贴 toml 安装。"
                .into(),
        );
    }
    diskpilot_toolbelt::signature::verify_ed25519(
        &[("scaffold.toml".to_string(), toml.as_bytes().to_vec())],
        &item.signature,
        &item.signer,
    )
    .map_err(|e| format!("脚本签名校验失败：{e}"))?;

    // 复用安装核心（id / 红线校验 + 写入 + 热重载），不放宽任何一步。
    crate::install_scaffold_inner(&app, &state, toml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_entries_have_no_download_url_yet() {
        let seed = seed_registry();
        assert_eq!(seed.len(), 2);
        for s in &seed {
            assert!(!s.id.is_empty());
            assert!(s.url.trim().is_empty(), "种子索引尚未提供下载地址");
        }
    }

    #[test]
    fn load_registry_falls_back_to_seed_when_missing() {
        // 无 app handle：用临时目录代替 app_data_dir 不现实（AppHandle 依赖
        // tauri runtime）。这里直接验证 seed 形状 + 字段默认值契约。
        let seed = seed_registry();
        let first = &seed[0];
        assert_eq!(first.signature, "");
        assert_eq!(first.signer, "");
        assert_eq!(first.sha256, "");
        assert_eq!(first.downloads, 0);
    }

    #[test]
    fn seed_ids_are_valid_kebab_case() {
        for s in seed_registry() {
            assert!(
                !s.id.is_empty()
                    && s.id
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "seed id `{}` 不是合法 kebab-case",
                s.id
            );
        }
    }
}
