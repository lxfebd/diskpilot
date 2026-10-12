//! 插件注册表类型层：索引条目 / 信封 / 安装台账 / URL 配置。
//!
//! 从 `plugin_registry.rs` 拆分（2026-10-12）：纯数据结构 + 解析辅助，
//! 不依赖 AppHandle/文件系统。读写、命令与安装管线留在 plugin_registry.rs。

use serde::{Deserialize, Serialize};

/// 索引 schema 版本（信封 schema 字段；不兼容时 refresh 拒绝写入）。
pub(crate) const REGISTRY_SCHEMA: u32 = 1;

// ── 索引条目结构 ───────────────────────────────────────────────────────

/// 单个可安装版本（多版本列表；顶层 version/url 是 latest 的快捷字段）。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PluginVersion {
    pub version: String,
    /// 下载地址（必须 https）。
    pub url: String,
    /// 期望 sha256（hex，可选；下载后强校验）。
    #[serde(default)]
    pub sha256: String,
    /// 签名者公钥 hex（可选；与包内 signer 一致才可信）。
    #[serde(default)]
    pub signer: String,
    /// 最低 DiskPilot 版本（可选，如 "0.1.2"；不满足时前端禁用安装）。
    #[serde(default)]
    pub min_app: String,
    /// 包体积提示字节（可选，下载前展示）。
    #[serde(default)]
    pub size_hint: u64,
    /// 版本说明（可选，changelog 摘要）。
    #[serde(default)]
    pub notes: String,
}

/// 注册表索引条目。新字段全部 `#[serde(default)]`，保持对旧索引的向后兼容。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RegistryPlugin {
    /// 合法 kebab-case 插件 id（与安装管线同一身份契约）。
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub category: String,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 最新版的下载地址（必须 https，否则前端禁用安装按钮）。
    #[serde(default)]
    pub url: String,
    /// 最新版期望 sha256（hex，可选）。
    #[serde(default)]
    pub sha256: String,
    /// 最新版签名者公钥 hex（可选）。
    #[serde(default)]
    pub signer: String,
    /// 下载次数（展示用，本地计数）。
    #[serde(default)]
    pub downloads: u64,
    /// 多版本列表（新）：安装/更新时可选目标版本。
    #[serde(default)]
    pub versions: Vec<PluginVersion>,
    /// 许可证（新，如 "MIT" / "GPL-3.0" / "Proprietary"）。
    #[serde(default)]
    pub license: String,
    /// 依赖的插件 id 列表（新）。
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// 官方验签通过标记（新）：由索引签名验证通过后由客户端标记。
    #[serde(default)]
    pub verified: bool,
    /// 主页地址（新，展示用；非下载地址）。
    #[serde(default)]
    pub homepage: String,
}

impl RegistryPlugin {
    /// 取指定版本的下载信息；`version=None` 时取最新版（顶层字段，兼容无 versions 的旧索引）。
    pub fn resolve(&self, version: Option<&str>) -> (String, String, String, String, u64, String) {
        let want = version.map(str::trim).filter(|v| !v.is_empty());
        match want {
            Some(v) => {
                if let Some(pv) = self.versions.iter().find(|p| p.version == v) {
                    return (
                        pv.version.clone(),
                        pv.url.clone(),
                        pv.sha256.clone(),
                        pv.signer.clone(),
                        pv.size_hint,
                        pv.notes.clone(),
                    );
                }
                (
                    v.to_string(),
                    String::new(),
                    String::new(),
                    String::new(),
                    0,
                    String::new(),
                )
            }
            None => {
                // 有 versions 时取第一条（索引约定按发布顺序倒序），否则用顶层快捷字段。
                match self.versions.first() {
                    Some(pv) => (
                        pv.version.clone(),
                        pv.url.clone(),
                        pv.sha256.clone(),
                        pv.signer.clone(),
                        pv.size_hint,
                        pv.notes.clone(),
                    ),
                    None => (
                        self.version.clone(),
                        self.url.clone(),
                        self.sha256.clone(),
                        self.signer.clone(),
                        0,
                        String::new(),
                    ),
                }
            }
        }
    }
}

// ── 索引信封 ───────────────────────────────────────────────────────────

/// 索引信封：`plugins-registry.json` 顶层结构。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegistryEnvelope {
    /// 信封 schema 版本（当前 1）。
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// 索引来源 URL（本地 seed 时为空）。
    #[serde(default)]
    pub source_url: String,
    /// 拉取时间（unix 秒；seed/未配置为 0）。
    #[serde(default)]
    pub fetched_at: u64,
    /// 索引签名（hex，可选）：对 `plugins` 数组的**规范 JSON 字符串**
    /// （`serde_json::to_string(&plugins)`，无 pretty）做 Ed25519 签名。
    #[serde(default)]
    pub signature: String,
    /// 签名者公钥 hex（可选；与 signature 一起才验签）。
    #[serde(default)]
    pub signer: String,
    pub plugins: Vec<RegistryPlugin>,
}

fn default_schema() -> u32 {
    REGISTRY_SCHEMA
}

impl RegistryEnvelope {
    /// 校验签名：无 signature/signer 时返回 true（未签名索引，展示为「未验签」）；
    /// 有签名则强制验签，失败返回 Err（调用方保留旧索引）。
    pub fn verify(&self) -> Result<bool, String> {
        let sig = self.signature.trim();
        let key = self.signer.trim();
        if sig.is_empty() || key.is_empty() {
            return Ok(false);
        }
        // 签名对象 = plugins 数组的规范 JSON 字符串（serde_json::to_string 无 pretty）。
        // 与包签名模型不同：这里对「整个 plugins[] 的 JSON 文本」签名，无需路径帧。
        let payload = serde_json::to_string(&self.plugins).map_err(|e| e.to_string())?;
        diskpilot_toolbelt::signature::verify_ed25519(
            &[(String::new(), payload.into_bytes())],
            sig,
            key,
        )
        .map_err(|e| format!("索引签名校验失败：{e}"))?;
        Ok(true)
    }
}

// ── 安装台账 ───────────────────────────────────────────────────────────

/// 安装台账条目（追加式历史，全量原子写 + 200 条裁剪）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// 事件时间（unix 秒）。
    pub ts: u64,
    /// 事件类型：`install` / `update` / `rollback`。
    pub kind: String,
    pub id: String,
    pub name: String,
    pub version: String,
    /// 来源：`registry` / `url` / `zip` / `builtin`。
    pub source: String,
    /// 安装目录相对 Tools 根路径。
    pub dir_rel: String,
    /// blob 缓存文件相对 app_data_dir 路径（如 `plugin-cache/foo-v1.0.0.zip`；空 = 无缓存）。
    #[serde(default)]
    pub blob: String,
    /// blob 的 sha256（hex）。
    #[serde(default)]
    pub blob_hash: String,
    /// 签名者公钥 hex（空 = 未签名，本地安装）。
    #[serde(default)]
    pub signer: String,
    /// 更新/回滚的目标来源版本（`update`/`rollback` 时非空）。
    #[serde(default)]
    pub from_version: String,
}

/// 台账某插件的当前状态（前端「已安装」列表用）。
#[derive(Clone, Debug, Serialize)]
pub struct InstalledPlugin {
    pub id: String,
    pub name: String,
    /// 当前已装版本。
    pub version: String,
    pub source: String,
    pub dir_rel: String,
    pub installed_at: u64,
    #[serde(default)]
    pub signer: String,
    #[serde(default)]
    pub blob: String,
    #[serde(default)]
    pub blob_hash: String,
    /// 可回滚的历史版本列表（旧台账条目，按时间倒序）。
    #[serde(default)]
    pub history: Vec<LedgerEntry>,
}

// ── 索引 URL 配置 ──────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// 社区索引 URL（必须 https；空 = 未配置索引）。
    #[serde(default)]
    pub registry_url: String,
}
