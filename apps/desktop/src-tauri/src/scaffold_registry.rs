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
//! - 安装核心复用 `scaffold_cmds::install_scaffold_inner`：id 校验 → 运行时红线
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

// ── 远端索引信封（R497）─────────────────────────────────────────────────
// 对齐插件市场（plugin_registry_types::RegistryEnvelope）：顶层 schema /
// source_url / fetched_at / signature / signer + `scaffolds[]`。
/// 索引 schema 版本（不兼容时 refresh 拒绝写入）。
pub(crate) const SCAFFOLD_REGISTRY_SCHEMA: u32 = 1;
/// 官方社区清理脚本索引地址（与 docs/index.example.json 的 source_url 同源；
/// 用户也可在刷新时传自定义 https 索引 URL 覆盖）。
pub(crate) const OFFICIAL_SCAFFOLD_INDEX_URL: &str =
    "https://raw.githubusercontent.com/lxfebd/diskpilot-scaffolds-index/main/index.json";

fn default_schema() -> u32 {
    SCAFFOLD_REGISTRY_SCHEMA
}

/// 社区脚本索引信封：`scaffolds-registry.json` 的远端形态。
/// 本地旧格式（裸 `Vec<RegistryScaffold>` 数组）仍可读（见 load_registry）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ScaffoldEnvelope {
    #[serde(default = "default_schema")]
    pub schema: u32,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub fetched_at: u64,
    /// 对 `scaffolds` 数组规范 JSON（`serde_json::to_string`，无 pretty）的 Ed25519 签名。
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub signer: String,
    pub scaffolds: Vec<RegistryScaffold>,
}

impl ScaffoldEnvelope {
    /// 校验索引签名：无 signature/signer → Ok(false)（未签名索引，展示「未验签」）；
    /// 有签名则强制验签，失败 Err（调用方保留旧索引，永不静默放行）。
    pub fn verify(&self) -> Result<bool, String> {
        let sig = self.signature.trim();
        let key = self.signer.trim();
        if sig.is_empty() || key.is_empty() {
            return Ok(false);
        }
        let payload = serde_json::to_string(&self.scaffolds).map_err(|e| e.to_string())?;
        diskpilot_toolbelt::signature::verify_ed25519(
            &[(String::new(), payload.into_bytes())],
            sig,
            key,
        )
        .map_err(|e| format!("索引签名校验失败：{e}"))?;
        Ok(true)
    }
}

/// 解析 + 校验远端索引信封（纯函数，便于单测）：JSON → schema → 非空 →
/// 条目 id 合法性 → 可选验签 → 补 source_url/fetched_at。返回 (信封, verified)。
pub(crate) fn parse_scaffold_envelope(
    body: &str,
    source_url: &str,
    fetched_at: u64,
) -> Result<(ScaffoldEnvelope, bool), String> {
    let env: ScaffoldEnvelope =
        serde_json::from_str(body).map_err(|e| format!("索引解析失败（JSON 格式错误）：{e}"))?;
    if env.schema != SCAFFOLD_REGISTRY_SCHEMA {
        return Err(format!(
            "索引 schema 版本不兼容（索引 {}，客户端 {}）。请升级 DiskPilot 或使用匹配的索引。",
            env.schema, SCAFFOLD_REGISTRY_SCHEMA
        ));
    }
    if env.scaffolds.is_empty() {
        return Err("索引为空（scaffolds 数组无条目）".into());
    }
    // 条目 id 必须是合法 kebab-case（与安装管线同一身份契约，防路径穿越）。
    for s in &env.scaffolds {
        if !crate::scaffold_cmds::is_valid_scaffold_id(&s.id) {
            return Err(format!(
                "索引条目 id 非法（必须是小写字母/数字/连字符）：{}",
                s.id
            ));
        }
    }
    let verified = env.verify()?;
    Ok((
        ScaffoldEnvelope {
            schema: SCAFFOLD_REGISTRY_SCHEMA,
            source_url: source_url.to_string(),
            fetched_at,
            signature: env.signature,
            signer: env.signer,
            scaffolds: env.scaffolds,
        },
        verified,
    ))
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

/// 读注册表索引的完整结果（条目 + 来源元信息 + 告警）。
struct LoadedRegistry {
    items: Vec<RegistryScaffold>,
    warning: Option<String>,
    source_url: String,
    fetched_at: u64,
    verified: bool,
}

/// 读注册表索引（R497 起支持信封格式；兼容旧的裸数组格式；
/// 无文件时返回种子索引；解析失败回退种子 + 告警字段）。
fn load_registry(app: &AppHandle) -> LoadedRegistry {
    let seed = |warning: Option<String>| LoadedRegistry {
        items: seed_registry(),
        warning,
        source_url: String::new(),
        fetched_at: 0,
        verified: false,
    };
    let Some(p) = registry_path(app) else {
        return seed(Some("无法定位数据目录，使用内置种子索引".into()));
    };
    if !p.is_file() {
        return seed(None);
    }
    let Ok(text) = std::fs::read_to_string(&p) else {
        return seed(Some("注册表索引不可读，已回退内置种子索引".into()));
    };
    // 优先按信封解析（R497 远端刷新落盘的格式）。
    if let Ok(env) = serde_json::from_str::<ScaffoldEnvelope>(&text) {
        if !env.scaffolds.is_empty() {
            let verified = env.verify().unwrap_or(false);
            return LoadedRegistry {
                items: env.scaffolds,
                warning: None,
                source_url: env.source_url,
                fetched_at: env.fetched_at,
                verified,
            };
        }
    }
    // 兼容旧格式：裸数组。
    match serde_json::from_str::<Vec<RegistryScaffold>>(&text) {
        Ok(v) if !v.is_empty() => LoadedRegistry {
            items: v,
            warning: None,
            source_url: String::new(),
            fetched_at: 0,
            verified: false,
        },
        _ => seed(Some("注册表索引损坏或为空，已回退内置种子索引".into())),
    }
}

/// 社区脚本仓库列表（只读 L0，不需要权限）。
#[tauri::command]
pub(crate) fn scaffold_registry_list(app: AppHandle) -> serde_json::Value {
    let reg = load_registry(&app);
    serde_json::json!({
        "items": reg.items,
        "warning": reg.warning,
        "source_url": reg.source_url,
        "fetched_at": reg.fetched_at,
        "verified": reg.verified,
        "updated_at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

/// 刷新社区脚本索引（R497）：从官方索引（或用户指定的 https 地址）拉取远端
/// 信封 → schema 校验 → 条目 id 校验 → 可选 Ed25519 验签 → 原子写入本地缓存。
/// 写操作：confirmed=true + `scaffold.manage` 双硬校验（与安装命令同款纵深防御）。
/// 验签失败 / 解析失败时保留旧索引不覆盖。
#[tauri::command]
pub(crate) async fn scaffold_registry_refresh(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    url: Option<String>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "scaffold:confirm: 刷新社区索引会从网络拉取脚本目录，必须由用户明确确认（confirmed=true）。"
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
    // 未传 URL 时用官方索引；自定义 URL 同样强制 https + 私网/SSRF 校验。
    let url = match url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()) {
        Some(u) => u,
        None => OFFICIAL_SCAFFOLD_INDEX_URL.to_string(),
    };
    let parsed = url::Url::parse(&url).map_err(|e| format!("索引 URL 不是合法地址：{e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!(
            "社区索引 URL 必须使用 https（当前：{}）。为防索引被中间人篡改，http 与自定义协议一律拒绝。",
            parsed.scheme()
        ));
    }
    if let Some(blocked) = crate::ai::blocked_private_target(parsed.as_str()) {
        return Err(format!("社区索引 URL 不可用（{blocked}）：{url}"));
    }

    // SSRF 安全拉取（禁重定向跟随 + 逐跳私网校验 + 8MB 上限，索引远小于此）。
    let body = crate::net_http::ssrf_safe_get(&url, 8 * 1024 * 1024, None).await?;
    let body = String::from_utf8_lossy(&body).to_string();

    let fetched_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (env, verified) = parse_scaffold_envelope(&body, &url, fetched_at)?;

    let Some(p) = registry_path(&app) else {
        return Err("无法定位数据目录".into());
    };
    let text = serde_json::to_string_pretty(&env).map_err(|e| e.to_string())?;
    crate::plugin_registry::preg_store::atomic_write(&p, &text).map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "ok": true,
        "count": env.scaffolds.len(),
        "source_url": env.source_url,
        "fetched_at": env.fetched_at,
        "verified": verified,
        "signed": !env.signer.trim().is_empty(),
    }))
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

    let reg = load_registry(&app);
    let item = reg
        .items
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
    let body = crate::net_http::ssrf_safe_get(item.url.trim(), 1024 * 1024, None).await?;
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
    crate::scaffold_cmds::install_scaffold_inner(&app, &state, toml)
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

    // ── R497：远端索引信封解析 ──────────────────────────────────────────

    fn envelope_json(schema: u32, scaffolds: &str) -> String {
        format!(
            r#"{{"schema":{schema},"source_url":"","fetched_at":0,"signature":"","signer":"","scaffolds":{scaffolds}}}"#
        )
    }

    fn one_scaffold() -> String {
        r#"[{"id":"foo-cache","name":"Foo 缓存","version":"1.0.0","author":"a","description":"d","risk":"low","tags":[],"url":"https://x/f.toml","sha256":"","signature":"","signer":"","downloads":0}]"#.to_string()
    }

    #[test]
    fn envelope_parse_rejects_unknown_schema() {
        let err =
            parse_scaffold_envelope(&envelope_json(2, &one_scaffold()), "https://x/i.json", 0)
                .unwrap_err();
        assert!(err.contains("schema"), "应报告 schema 不兼容：{err}");
    }

    #[test]
    fn envelope_parse_rejects_empty_scaffolds() {
        let err =
            parse_scaffold_envelope(&envelope_json(1, "[]"), "https://x/i.json", 0).unwrap_err();
        assert!(err.contains("为空"), "空索引应拒绝：{err}");
    }

    #[test]
    fn envelope_parse_rejects_illegal_id() {
        let bad = r#"[{"id":"Bad_ID","name":"x","version":"1","author":"a","description":"d","risk":"low"}]"#;
        let err =
            parse_scaffold_envelope(&envelope_json(1, bad), "https://x/i.json", 0).unwrap_err();
        assert!(err.contains("id 非法"), "非法 id 应拒绝：{err}");
    }

    #[test]
    fn envelope_parse_unsigned_returns_verified_false() {
        let (env, verified) =
            parse_scaffold_envelope(&envelope_json(1, &one_scaffold()), "https://x/i.json", 987)
                .unwrap();
        assert!(!verified, "未签名索引 verified=false");
        assert_eq!(env.source_url, "https://x/i.json");
        assert_eq!(env.fetched_at, 987);
        assert_eq!(env.scaffolds.len(), 1);
    }

    #[test]
    fn envelope_parse_bad_signature_errors() {
        let mut env: ScaffoldEnvelope =
            serde_json::from_str(&envelope_json(1, &one_scaffold())).unwrap();
        env.signature = "deadbeef".into();
        env.signer = "cafebabe".into();
        let body = serde_json::to_string(&env).unwrap();
        let err = parse_scaffold_envelope(&body, "https://x/i.json", 0).unwrap_err();
        assert!(err.contains("签名"), "坏签名必须 Err：{err}");
    }

    #[test]
    fn envelope_parse_valid_signature_verified_true() {
        // generate_keypair 返回 (公钥 hex, 私钥 hex)。
        let (public, secret) = diskpilot_toolbelt::signature::generate_keypair();
        let scaffolds: Vec<RegistryScaffold> = serde_json::from_str(&one_scaffold()).unwrap();
        let payload = serde_json::to_string(&scaffolds).unwrap();
        let sig = diskpilot_toolbelt::signature::sign_entries(
            &[(String::new(), payload.into_bytes())],
            &secret,
        )
        .unwrap();
        let mut env: ScaffoldEnvelope =
            serde_json::from_str(&envelope_json(1, &one_scaffold())).unwrap();
        env.signature = sig;
        env.signer = public;
        let body = serde_json::to_string(&env).unwrap();
        let (env, verified) = parse_scaffold_envelope(&body, "https://x/i.json", 1).unwrap();
        assert!(verified, "合法签名 verified=true");
        assert_eq!(env.schema, SCAFFOLD_REGISTRY_SCHEMA);
    }

    #[test]
    fn official_index_url_is_https_raw_github() {
        assert!(OFFICIAL_SCAFFOLD_INDEX_URL.starts_with("https://"));
        assert!(OFFICIAL_SCAFFOLD_INDEX_URL.ends_with("index.json"));
    }
}
