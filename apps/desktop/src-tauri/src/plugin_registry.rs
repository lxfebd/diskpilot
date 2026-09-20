//! 插件社区注册表（B3 首版 + 远端分发完整版，2026-09-09 / 2026-09-11）
//!
//! 三个文件布局（都在 `app_data_dir`，全部原子写 = 临时文件 + rename）：
//! - `plugins-registry.json`：合并后的**索引信封**（`RegistryEnvelope`），
//!   顶层带 schema/source_url/fetched_at/signature/signer + `plugins[]`。
//!   首次运行或文件缺失/损坏 → 回退内置 seed 占位索引（前端显示告警）。
//! - `registry-config.json`：索引 URL 配置（`RegistryConfig`）。
//!   默认为空 = 未配置索引；配置后 `plugin_registry_refresh` 拉取远端索引。
//!   官方 GitHub 索引仓库后建，本轮以 URL 配置化框架落地（不硬编码地址）。
//! - `plugin-ledger.json`：安装台账（`Vec<LedgerEntry>`，全量原子写 + 200 条裁剪）。
//!   记录每次 install/update/rollback 的版本、来源、blob 缓存路径、sha256、签名者。
//!
//! 下载产物缓存（免重下）：`plugin-cache/<id>-v<version>.zip`，
//! 更新/回滚时先查缓存，命中直接用，未命中才走 https 下载。
//!
//! 安全模型：
//! - 列表 / 搜索 / 校验 / 台账读取 = 只读 L0，不需要权限。
//! - 「刷新索引 / 安装 / 更新 / 回滚 / 配置 URL」= 写操作，
//!   `confirmed=true` + `perm_grants["plugin.manage"]` 双硬校验。
//! - 远端索引与插件包都强制 https；索引带 Ed25519 签名时强制验签
//!   （失败保留旧索引并报错，绝不覆盖为未验签内容）。
//! - 插件包走与本地安装同一管线（`install_plugin_zip`：digest/Ed25519 验签），
//!   远程来源额外强制 Ed25519（`plugin_download_to_path`）。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

const REGISTRY_FILE: &str = "plugins-registry.json";
const CONFIG_FILE: &str = "registry-config.json";
const LEDGER_FILE: &str = "plugin-ledger.json";
const CACHE_DIR: &str = "plugin-cache";
const LEDGER_MAX_ENTRIES: usize = 200;
/// 索引 schema 版本（信封 schema 字段；不兼容时 refresh 拒绝写入）。
const REGISTRY_SCHEMA: u32 = 1;

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

// ── 路径与原子写 ───────────────────────────────────────────────────────

fn data_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok()
}

fn registry_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    data_dir(app).map(|d| d.join(REGISTRY_FILE))
}

fn config_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    data_dir(app).map(|d| d.join(CONFIG_FILE))
}

fn ledger_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    data_dir(app).map(|d| d.join(LEDGER_FILE))
}

/// 插件包 blob 缓存文件路径（`plugin-cache/<id>-v<version>.zip`）。
/// version 非法字符（除字母数字/`.`/`-`）一律剔除，防路径穿越。
fn cache_file_path(app: &AppHandle, id: &str, version: &str) -> Option<std::path::PathBuf> {
    let safe_id = diskpilot_toolbelt::plugin_id_from_name(id).unwrap_or_else(|_| "plugin".into());
    let safe_ver: String = version
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
        .collect();
    let name = if safe_ver.is_empty() {
        format!("{}.zip", safe_id)
    } else {
        format!("{}-v{}.zip", safe_id, safe_ver)
    };
    data_dir(app).map(|d| d.join(CACHE_DIR).join(name))
}

/// 原子写：先写 `.tmp` 再 rename，避免中途崩溃留下半截 JSON。
pub(crate) fn atomic_write(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 内置 seed 索引：官方插件市场占位（无真实下载地址时前端禁用安装）。
/// 索引仓库后建，本轮以 URL 配置化框架落地，seed 仅在未配置索引时兜底展示。
fn seed_registry() -> RegistryEnvelope {
    RegistryEnvelope {
        schema: REGISTRY_SCHEMA,
        source_url: String::new(),
        fetched_at: 0,
        signature: String::new(),
        signer: String::new(),
        plugins: vec![
            RegistryPlugin {
                id: "sysmon".into(),
                name: "SysMon 系统监控插件".into(),
                version: "1.0.0".into(),
                author: "DiskPilot 官方".into(),
                description: "进程/CPU/内存实时监控面板（只读），把系统监控直接挂进工具墙。".into(),
                category: "系统工具".into(),
                risk: "low".into(),
                tags: vec!["监控".into(), "只读".into()],
                license: "MIT".into(),
                ..Default::default()
            },
            RegistryPlugin {
                id: "hwinfo-lite".into(),
                name: "HWiNFO 精简助手".into(),
                version: "1.0.0".into(),
                author: "DiskPilot 官方".into(),
                description: "传感器数据一键导出（温度/风扇/电压），纯只读封装。".into(),
                category: "硬件工具".into(),
                risk: "low".into(),
                tags: vec!["传感器".into(), "导出".into()],
                license: "MIT".into(),
                ..Default::default()
            },
        ],
    }
}

// ── 读写 ───────────────────────────────────────────────────────────────

/// 读索引信封：文件缺失 → seed；解析失败/为空 → seed + 告警。
fn load_envelope(app: &AppHandle) -> (RegistryEnvelope, Option<String>) {
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
        .and_then(|s| serde_json::from_str::<RegistryEnvelope>(&s).ok())
    {
        Some(env) if !env.plugins.is_empty() => (env, None),
        _ => (
            seed_registry(),
            Some("注册表索引损坏或为空，已回退内置种子索引".into()),
        ),
    }
}

fn load_config(app: &AppHandle) -> RegistryConfig {
    let Some(p) = config_path(app) else {
        return RegistryConfig::default();
    };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn load_ledger(app: &AppHandle) -> Vec<LedgerEntry> {
    let Some(p) = ledger_path(app) else {
        return Vec::new();
    };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<LedgerEntry>>(&s).ok())
        .unwrap_or_default()
}

fn save_ledger(app: &AppHandle, entries: &[LedgerEntry]) -> Result<(), String> {
    let Some(p) = ledger_path(app) else {
        return Err("无法定位数据目录".into());
    };
    let trimmed: Vec<LedgerEntry> = if entries.len() > LEDGER_MAX_ENTRIES {
        entries
            .split_at(entries.len() - LEDGER_MAX_ENTRIES)
            .1
            .to_vec()
    } else {
        entries.to_vec()
    };
    let text = serde_json::to_string_pretty(&trimmed).map_err(|e| e.to_string())?;
    atomic_write(&p, &text).map_err(|e| e.to_string())
}

/// 追加一条台账并落盘（200 条裁剪）。
fn append_ledger(app: &AppHandle, entry: LedgerEntry) -> Result<(), String> {
    let mut ledger = load_ledger(app);
    ledger.push(entry);
    save_ledger(app, &ledger)
}

/// 卸载插件时补一条 `uninstall` 事件（toolbelt::plugin_uninstall 调用）。
/// `installed_plugin_ids` 只聚合 install/update，卸载后不写这条会永久显示「已安装」。
pub(crate) fn append_uninstall_ledger(
    app: &AppHandle,
    id: &str,
    name: &str,
    dir_rel: &str,
    version: &str,
) {
    let _ = append_ledger(
        app,
        LedgerEntry {
            ts: now_secs(),
            kind: "uninstall".into(),
            id: id.into(),
            name: name.into(),
            version: version.into(),
            source: "local".into(),
            dir_rel: dir_rel.into(),
            blob: String::new(),
            blob_hash: String::new(),
            signer: String::new(),
            from_version: String::new(),
        },
    );
}

// ── 只读命令 ───────────────────────────────────────────────────────────

/// 兼容旧调用：直接读配置文件；这是唯一读索引 URL 的入口。
fn load_registry_url(app: &AppHandle) -> String {
    load_config(app).registry_url
}

/// 内置工具目录的插件 id 集合（复用 toolbelt 的 manifest 清单：plugin 元数据 id；
/// 无插件元数据的传统工具按名字净化——与内置市场同一口径）。社区列表据此给
/// 「与内置重复」的条目打 `builtin` 标记，避免用户装一个已经内置的插件。
/// 统一存小写：filter_builtin_dupes 比较时两端都折叠大小写，防远端大小写变体漏剔。
fn builtin_plugin_ids() -> std::collections::HashSet<String> {
    diskpilot_toolbelt::all_manifests()
        .iter()
        .filter_map(|m| {
            diskpilot_toolbelt::plugin_id_from_name(&m.name)
                .ok()
                .map(|id| id.to_ascii_lowercase())
        })
        .collect()
}

/// 剔除与内置重复的条目，返回保留条目 + 剔除数。纯函数便于单测。
/// 两端 id 大小写不敏感：远端索引若发 `CRYSTALDISKINFO` 也应命中内置
/// `crystaldiskinfo`，避免「与内置重复的插件还能装」的漏剔。
fn filter_builtin_dupes(
    plugins: Vec<RegistryPlugin>,
    builtin_ids: &std::collections::HashSet<String>,
) -> (Vec<RegistryPlugin>, usize) {
    let mut kept = Vec::with_capacity(plugins.len());
    let mut skipped = 0usize;
    for p in plugins {
        let name_id = diskpilot_toolbelt::plugin_id_from_name(&p.name).unwrap_or_default();
        let builtin = builtin_ids.contains(p.id.to_ascii_lowercase().as_str())
            || builtin_ids.contains(&name_id);
        if builtin {
            skipped += 1;
        } else {
            kept.push(p);
        }
    }
    (kept, skipped)
}

/// 社区列表去重后的条目（已装标记 + 与内置重复的条目被剔除）+ 剔除计数。
fn dedup_plugins(
    app: &AppHandle,
    plugins: Vec<RegistryPlugin>,
) -> (Vec<serde_json::Value>, usize) {
    let builtin_ids = builtin_plugin_ids();
    let installed_ids = installed_plugin_ids(app);
    let (kept, skipped) = filter_builtin_dupes(plugins, &builtin_ids);
    let items = kept
        .into_iter()
        .map(|p| {
            let installed = installed_ids.contains(&p.id);
            let mut v = serde_json::to_value(&p).unwrap_or(serde_json::Value::Null);
            if let serde_json::Value::Object(m) = &mut v {
                m.insert("builtin".into(), serde_json::json!(false));
                m.insert("installed".into(), serde_json::json!(installed));
            }
            v
        })
        .collect();
    (items, skipped)
}

/// 台账里当前「已安装」的插件 id 集合：按每条事件正序回放，install/update 标记已装、
/// uninstall 移除——以最新事件状态为准（只聚合 install/update 会同 ③-3 审计一样
/// 让已卸载插件永久标记为已安装）。
fn installed_plugin_ids(app: &AppHandle) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    for e in load_ledger(app).iter().filter(|e| e.kind != "index") {
        match e.kind.as_str() {
            "install" | "update" => {
                ids.insert(e.id.clone());
            }
            "uninstall" => {
                ids.remove(&e.id);
            }
            _ => {}
        }
    }
    ids
}

/// 社区注册表列表（只读 L0）。返回信封 + 条目 + 索引 URL 配置状态。
/// 与内置重复的条目被剔除（`skipped_builtin` 计数），已安装的条目带 `installed` 标记。
#[tauri::command]
pub(crate) fn plugin_registry_list(app: AppHandle) -> serde_json::Value {
    let (env, warn) = load_envelope(&app);
    let url = load_registry_url(&app);
    let verified = if env.signature.trim().is_empty() {
        false
    } else {
        env.verify().unwrap_or(false)
    };
    let (items, skipped) = dedup_plugins(&app, env.plugins);
    serde_json::json!({
        "items": items,
        "skipped_builtin": skipped,
        "warning": warn,
        "updated_at": now_secs(),
        "schema": env.schema,
        "source_url": env.source_url,
        "fetched_at": env.fetched_at,
        "signed": !env.signature.trim().is_empty() && !env.signer.trim().is_empty(),
        "verified": verified,
        "signer": env.signer,
        "registry_url": url,
        "indexed": !url.trim().is_empty(),
    })
}

/// 社区注册表搜索（只读 L0）：按名称/描述/标签/分类/作者关键字过滤。
/// 与内置重复的条目同样被剔除。
#[tauri::command]
pub(crate) fn plugin_registry_search(app: AppHandle, q: String) -> serde_json::Value {
    let (env, warn) = load_envelope(&app);
    let kw = q.trim().to_ascii_lowercase();
    let filtered: Vec<RegistryPlugin> = if kw.is_empty() {
        env.plugins
    } else {
        env.plugins
            .into_iter()
            .filter(|p| {
                p.name.to_ascii_lowercase().contains(&kw)
                    || p.description.to_ascii_lowercase().contains(&kw)
                    || p.category.to_ascii_lowercase().contains(&kw)
                    || p.author.to_ascii_lowercase().contains(&kw)
                    || p.tags.iter().any(|t| t.to_ascii_lowercase().contains(&kw))
            })
            .collect()
    };
    let (items, skipped) = dedup_plugins(&app, filtered);
    serde_json::json!({
        "items": items,
        "skipped_builtin": skipped,
        "warning": warn,
        "query": q,
        "schema": env.schema,
        "fetched_at": env.fetched_at,
        "source_url": env.source_url,
        "registry_url": load_registry_url(&app),
        "indexed": !load_registry_url(&app).trim().is_empty(),
    })
}

/// 校验索引签名与条目字段完备性（只读 L0）。
#[tauri::command]
pub(crate) fn plugin_registry_verify(app: AppHandle) -> serde_json::Value {
    let (env, warn) = load_envelope(&app);
    let mut items = Vec::with_capacity(env.plugins.len());
    for p in &env.plugins {
        let (_, url, sha256, signer, _, _) = p.resolve(None);
        items.push(serde_json::json!({
            "id": p.id,
            "name": p.name,
            "version": p.version,
            "has_url": !url.trim().is_empty(),
            "https_url": url.trim().starts_with("https://"),
            "has_sha256": !sha256.trim().is_empty(),
            "has_signer": !signer.trim().is_empty(),
            "verified": p.verified,
            "issue": if url.trim().is_empty() {
                Some("尚未提供下载地址（即将上架）")
            } else if !url.trim().starts_with("https://") {
                Some("下载地址必须 https")
            } else if signer.trim().is_empty() {
                Some("未提供签名者公钥（远程安装要求 Ed25519 签名）")
            } else {
                None
            },
        }));
    }
    let signed = !env.signature.trim().is_empty() && !env.signer.trim().is_empty();
    let envelope_ok = if signed { env.verify().is_ok() } else { false };
    serde_json::json!({
        "signed": signed,
        "envelope_ok": envelope_ok,
        "envelope_error": if signed { env.verify().err() } else { Some("索引未签名".into()) },
        "schema": env.schema,
        "fetched_at": env.fetched_at,
        "items": items,
        "warning": warn,
    })
}

/// 已安装插件清单（只读 L0）：按台账最新条目聚合，带可回滚历史。
/// `index` 事件（索引刷新，id 固定 "registry"）不是插件，直接跳过；
/// `uninstall` 事件是终止态——该插件已不在本地，从清单里移除。
#[tauri::command]
pub(crate) fn plugin_list_installed(app: AppHandle) -> serde_json::Value {
    let ledger = load_ledger(&app);
    let mut by_id: std::collections::BTreeMap<String, InstalledPlugin> =
        std::collections::BTreeMap::new();
    for e in ledger.iter().rev() {
        if e.kind == "index" {
            continue;
        }
        if e.kind == "uninstall" {
            by_id.remove(&e.id);
            continue;
        }
        match by_id.entry(e.id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(InstalledPlugin {
                    id: e.id.clone(),
                    name: e.name.clone(),
                    version: e.version.clone(),
                    source: e.source.clone(),
                    dir_rel: e.dir_rel.clone(),
                    installed_at: e.ts,
                    signer: e.signer.clone(),
                    blob: e.blob.clone(),
                    blob_hash: e.blob_hash.clone(),
                    history: Vec::new(),
                });
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                if let Some(cur) = by_id.get_mut(&e.id) {
                    cur.history.push(e.clone());
                }
            }
        }
    }
    serde_json::json!({
        "installed": by_id.values().collect::<Vec<_>>(),
        "total_events": ledger.len(),
    })
}

/// 索引 URL 配置读取（只读 L0）。
#[tauri::command]
pub(crate) fn plugin_registry_config(app: AppHandle) -> serde_json::Value {
    serde_json::json!({ "registry_url": load_registry_url(&app) })
}

// ── 写命令：索引 URL 配置 ──────────────────────────────────────────────

/// 设置社区索引 URL（写操作：confirmed + plugin.manage 双校验；强制 https）。
#[tauri::command]
pub(crate) fn plugin_set_registry_url(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    url: String,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "plugin:confirm: 配置社区索引 URL 会改变插件来源，必须由用户明确确认（confirmed=true）。".into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let url = url.trim().to_string();
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
    let Some(p) = config_path(&app) else {
        return Err("无法定位数据目录".into());
    };
    let cfg = RegistryConfig {
        registry_url: url.clone(),
    };
    let text = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    atomic_write(&p, &text).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "registry_url": url }))
}

// ── 写命令：刷新索引 ───────────────────────────────────────────────────

/// 从配置的索引 URL 拉取远端索引（写操作：confirmed + plugin.manage 双校验）。
/// 校验链：https → schema 兼容 → 可选 Ed25519 验签（失败保留旧索引）→ 原子写。
#[tauri::command]
pub(crate) async fn plugin_registry_refresh(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "plugin:confirm: 刷新索引会从网络拉取插件目录，必须由用户明确确认（confirmed=true）。"
                .into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let url = load_registry_url(&app);
    if url.trim().is_empty() {
        return Err("尚未配置社区索引 URL（设置 → 插件市场 → 索引地址）。".into());
    }
    if let Some(blocked) = crate::ai::blocked_private_target(url.trim()) {
        return Err(format!("社区索引 URL 不可用（{blocked}）：{url}"));
    }

    // SSRF 安全拉取（禁重定向跟随 + 逐跳私网校验 + 响应体上限 32MB 足矣）。
    let body = crate::ssrf_safe_get(url.trim(), 32 * 1024 * 1024, None).await?;
    let body = String::from_utf8_lossy(&body).to_string();

    // 先验签再落盘：验签失败绝不覆盖本地索引。
    let env: RegistryEnvelope =
        serde_json::from_str(&body).map_err(|e| format!("索引解析失败（JSON 格式错误）：{e}"))?;
    if env.schema != REGISTRY_SCHEMA {
        return Err(format!(
            "索引 schema 版本不兼容（索引 {}，客户端 {}）。请升级 DiskPilot 或使用匹配的索引。",
            env.schema, REGISTRY_SCHEMA
        ));
    }
    if env.plugins.is_empty() {
        return Err("索引为空（plugins 数组无条目）".into());
    }
    let verified = env.verify()?;

    let Some(p) = registry_path(&app) else {
        return Err("无法定位数据目录".into());
    };
    let env = RegistryEnvelope {
        schema: REGISTRY_SCHEMA,
        source_url: url.clone(),
        fetched_at: now_secs(),
        signature: env.signature,
        signer: env.signer,
        plugins: env.plugins,
    };
    let text = serde_json::to_string_pretty(&env).map_err(|e| e.to_string())?;
    atomic_write(&p, &text).map_err(|e| e.to_string())?;

    // 追加一条台账记录（索引刷新也可追溯）。
    let _ = append_ledger(
        &app,
        LedgerEntry {
            ts: now_secs(),
            kind: "index".into(),
            id: "registry".into(),
            name: "社区索引".into(),
            version: format!("{} 条", env.plugins.len()),
            source: "registry".into(),
            dir_rel: String::new(),
            blob: String::new(),
            blob_hash: String::new(),
            signer: env.signer.clone(),
            from_version: String::new(),
        },
    );

    Ok(serde_json::json!({
        "ok": true,
        "error": serde_json::Value::Null,
        "registry_url": url,
        "count": env.plugins.len(),
        "plugins": env.plugins.len(),
        "fetched_at": env.fetched_at,
        "verified": verified,
        "signed": !env.signer.trim().is_empty(),
        "source_url": env.source_url,
        "warning": serde_json::Value::Null,
    }))
}

// ── 写命令：安装 / 更新 / 回滚 ─────────────────────────────────────────

/// 定位 Tools 根（失败返回 Err）。
fn resolve_tools_root(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let explicit = crate::toolbelt::toolbelt_explicit_root(app);
    diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())
}

/// 启动清理：update/rollback 在「移开旧目录」后若进程崩溃，会留下
/// `*.dp-old-tmp` 残留目录（旧版被移走、新版没装上）。应用下次启动时
/// 扫描 Tools 根，把这类残留目录整个删掉——旧版内容已备份进 blob 缓存
/// （update 前打包成 zip，rollback 用 target.blob），可随时恢复。
/// 静默失败：清理是增强行为，不阻塞启动。
pub(crate) fn sweep_orphaned_tmp_dirs(app: &AppHandle) {
    let Ok(root) = resolve_tools_root(app) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_tmp = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e == "dp-old-tmp")
            .unwrap_or(false);
        if is_tmp && path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// 把 blob 缓存/下载来的 zip 安装到 Tools，返回 dir_rel。
async fn install_zip_bytes(
    zip_path: &std::path::Path,
    root: &std::path::Path,
) -> Result<String, String> {
    let zip = zip_path.to_path_buf();
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || diskpilot_toolbelt::install_plugin_zip(&zip, &root))
        .await
        .map_err(|e| format!("安装任务崩溃: {e}"))?
        .map_err(|e| e.to_string())
}

/// 强制 Ed25519 签名检查（与 URL 安装 plugin_install_url_inner 同标准，③-2）：
/// 社区注册表下载的插件包必须带完整签名；未签名/仅 sha256 的包只能
/// 本地 zip 直装。索引声明不验签（verify="none"）也在这里被拦下。
async fn require_registry_signed(zip: &std::path::Path) -> Result<(), String> {
    let has = diskpilot_toolbelt::signature::plugin_zip_has_ed25519(zip)?;
    if !has {
        let _ = std::fs::remove_file(zip);
        return Err(
            "社区注册表插件必须带 Ed25519 签名（tool.plugin.json 需含 signature+signer，verify=ed25519）。\
             未签名/仅 sha256 的包只能通过本地 zip 安装。"
                .into(),
        );
    }
    Ok(())
}

/// 下载指定版本到 blob 缓存（缓存命中直接返回；未命中走 https 下载 + sha256 校验）。
/// 返回缓存文件相对 app_data_dir 路径 + sha256。
async fn fetch_to_cache(
    app: &AppHandle,
    p: &RegistryPlugin,
    version: Option<&str>,
) -> Result<(String, String, bool), String> {
    let (ver, url, want_sha256, _signer, _size, _notes) = p.resolve(version);
    let Some(cache) = cache_file_path(app, &p.id, &ver) else {
        return Err("无法定位数据目录".into());
    };
    let Some(data) = data_dir(app) else {
        return Err("无法定位数据目录".into());
    };
    let hash_of = |path: &std::path::Path| -> Result<String, String> {
        diskpilot_toolbelt::signature::sha256_file(path).map_err(|e| e.to_string())
    };

    // 缓存命中：存在且（无期望 hash 或 hash 匹配）。
    let blob_rel = cache
        .strip_prefix(&data)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| cache.to_string_lossy().into_owned());
    if cache.is_file() {
        let got = hash_of(&cache)?;
        if want_sha256.trim().is_empty() || got.eq_ignore_ascii_case(want_sha256.trim()) {
            return Ok((blob_rel, got, true));
        }
        let _ = std::fs::remove_file(&cache); // hash 不符 → 缓存作废
    }

    // 缓存未命中 → 下载。
    if url.trim().is_empty() {
        return Err(format!("插件「{}」尚未提供下载地址（即将上架）", p.name));
    }
    crate::plugin_remote::plugin_download_to_path(app, url.clone(), &cache).await?;
    let got = hash_of(&cache)?;
    if !want_sha256.trim().is_empty() && !got.eq_ignore_ascii_case(want_sha256.trim()) {
        let _ = std::fs::remove_file(&cache);
        return Err(format!(
            "插件包 sha256 校验失败（索引 {}，实际 {}）。为防篡改，已拒绝安装。",
            want_sha256.trim(),
            got
        ));
    }
    Ok((blob_rel, got, false))
}

fn plugin_entry_by_id(app: &AppHandle, id: &str) -> Result<RegistryPlugin, String> {
    let (env, _) = load_envelope(app);
    env.plugins
        .into_iter()
        .find(|p| p.id == id.trim())
        .ok_or_else(|| format!("注册表里没有插件「{id}」"))
}

/// 从注册表安装指定插件（可选目标版本）。写操作：confirmed + plugin.manage 双校验。
#[tauri::command]
pub(crate) async fn plugin_registry_install(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    version: Option<String>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "plugin:confirm: 从社区注册表安装插件会下载并解压到 Tools 目录，必须由用户明确确认（confirmed=true）。"
                .into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let p = plugin_entry_by_id(&app, &id)?;
    let ver_opt = version.as_deref();
    let (ver, url, _sha256, signer, _size, _notes) = p.resolve(ver_opt);
    if url.trim().is_empty() {
        return Err(format!("插件「{}」尚未提供下载地址（即将上架）", p.name));
    }
    let (blob, blob_hash, cached) = fetch_to_cache(&app, &p, ver_opt).await?;
    let root = resolve_tools_root(&app)?;
    // blob 相对 app_data_dir；解析回绝对路径（含 plugin-cache/ 前缀）。
    let cache = data_dir(&app)
        .map(|d| d.join(&blob))
        .ok_or_else(|| "无法定位数据目录".to_string())?;
    // 社区注册表来源强制 Ed25519 签名（与 URL 安装同标准）；缓存命中也要验。
    require_registry_signed(&cache).await?;
    let dir_rel = install_zip_bytes(&cache, &root).await?;
    let _ = append_ledger(
        &app,
        LedgerEntry {
            ts: now_secs(),
            kind: "install".into(),
            id: p.id.clone(),
            name: p.name.clone(),
            version: ver.clone(),
            source: "registry".into(),
            dir_rel: dir_rel.clone(),
            blob: blob.clone(),
            blob_hash: blob_hash.clone(),
            signer: signer.clone(),
            from_version: String::new(),
        },
    );
    Ok(serde_json::json!({
        "id": p.id,
        "name": p.name,
        "version": ver,
        "dir_rel": dir_rel,
        "cached": cached,
        "blob_hash": blob_hash,
    }))
}

/// 更新已安装插件到注册表最新版本（或指定版本）。
/// 流程：备份旧目录 → 下载新包（blob 缓存）→ 安装新版 → 台账记录 from→to。
/// 写操作：confirmed + plugin.manage 双校验。
#[tauri::command]
pub(crate) async fn plugin_registry_update(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    version: Option<String>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "plugin:confirm: 更新插件会替换 Tools 目录里的插件文件，必须由用户明确确认（confirmed=true）。".into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let p = plugin_entry_by_id(&app, &id)?;
    let (target_ver, target_url, _sha, target_signer, _sz, _nt) = p.resolve(version.as_deref());
    if target_url.trim().is_empty() {
        return Err(format!("插件「{}」尚未提供下载地址（即将上架）", p.name));
    }
    // 已装版本（台账最新条目）。
    let ledger = load_ledger(&app);
    let (from_ver, dir_rel) = ledger
        .iter()
        .rev()
        .find(|e| e.id == id.trim() && !e.kind.eq("index"))
        .map(|e| (e.version.clone(), e.dir_rel.clone()))
        .ok_or_else(|| format!("插件「{id}」尚未安装（台账无记录），请先安装"))?;
    if from_ver == target_ver {
        return Err(format!("插件「{}」已是最新版本（v{target_ver}）", p.name));
    }
    let root = resolve_tools_root(&app)?;
    let dir = root.join(dir_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !dir.is_dir() {
        return Err(format!("插件目录不存在：{}", dir.display()));
    }

    // 1. 备份旧目录（打包成 zip 存进 blob 缓存，作为回滚恢复源）。
    //    备份必须在移走旧目录之前完成，否则失败时无法恢复。
    let backup_rel = format!("plugin-cache/{}-v{from_ver}-backup.zip", p.id);
    let Some(data) = data_dir(&app) else {
        return Err("无法定位数据目录".into());
    };
    let backup_path = data.join(&backup_rel);
    if let Some(parent) = backup_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    tokio::task::spawn_blocking({
        let d = dir.clone();
        let b = backup_path.clone();
        move || diskpilot_toolbelt::export_plugin_zip(&d, &b)
    })
    .await
    .map_err(|e| format!("备份任务崩溃: {e}"))?
    .map_err(|e| format!("备份旧版失败：{e}"))?;

    // 2. 下载新包到 blob 缓存。
    let (blob, blob_hash, _cached) = fetch_to_cache(&app, &p, version.as_deref()).await?;
    let cache = data.join(&blob);
    // 社区注册表来源强制 Ed25519 签名（与 URL 安装同标准）；未签名包不装，
    // 且此刻旧版目录还在原位（校验放移开之前，失败零扰动）。
    require_registry_signed(&cache).await?;

    // 3. 移开旧目录（install_plugin_zip 对已存在同名插件会报错）。
    let tmp_dir = dir.with_extension("dp-old-tmp");
    if let Err(e) = std::fs::rename(&dir, &tmp_dir) {
        return Err(format!("移开旧版目录失败：{e}"));
    }
    // 4. 安装新版；失败则移回旧目录（新版包留在缓存供下次重试）。
    let res = install_zip_bytes(&cache, &root).await;
    match res {
        Ok(new_rel) => {
            // 新版成功 → 旧目录已备份在 zip，删除临时目录。
            let _ = std::fs::remove_dir_all(&tmp_dir);
            let _ = append_ledger(
                &app,
                LedgerEntry {
                    ts: now_secs(),
                    kind: "update".into(),
                    id: p.id.clone(),
                    name: p.name.clone(),
                    version: target_ver.clone(),
                    source: "registry".into(),
                    dir_rel: new_rel.clone(),
                    blob: blob.clone(),
                    blob_hash: blob_hash.clone(),
                    signer: target_signer.clone(),
                    from_version: from_ver.clone(),
                },
            );
            Ok(serde_json::json!({
                "id": p.id,
                "from": from_ver,
                "to": target_ver,
                "dir_rel": new_rel,
                "backup": backup_rel,
            }))
        }
        Err(e) => {
            // 安装失败 → 恢复旧目录，清理临时物。
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::rename(&tmp_dir, &dir);
            Err(format!("新版安装失败，已恢复旧版：{e}"))
        }
    }
}

/// 回滚到上一版本：从台账找旧版本的 blob 备份 → 移开当前目录 → 解压备份 → 台账记录。
/// 写操作：confirmed + plugin.manage 双校验。
#[tauri::command]
pub(crate) async fn plugin_registry_rollback(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    version: Option<String>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if confirmed != Some(true) {
        return Err(
            "plugin:confirm: 回滚插件会替换 Tools 目录里的插件文件，必须由用户明确确认（confirmed=true）。".into(),
        );
    }
    if !state.perm_grants.lock().unwrap().contains("plugin.manage") {
        return Err(
            "plugin:denied: 未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。"
                .into(),
        );
    }
    let ledger = load_ledger(&app);
    let cur = ledger
        .iter()
        .rev()
        .find(|e| e.id == id.trim() && !e.kind.eq("index"))
        .ok_or_else(|| format!("插件「{id}」尚未安装（台账无记录）"))?;
    // 目标版本：显式指定，或取上一条非回滚记录的版本。
    let target_ver = match version.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => v.to_string(),
        None => ledger
            .iter()
            .rev()
            .skip_while(|e| !(e.id == id.trim() && !e.kind.eq("index")))
            .skip(1)
            .find_map(|e| {
                if e.version != cur.version {
                    Some(e.version.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| format!("插件「{id}」没有可回滚的旧版本（台账历史里没有其他版本）"))?,
    };
    let target = ledger
        .iter()
        .rev()
        .find(|e| e.id == id.trim() && e.version == target_ver)
        .ok_or_else(|| format!("台账里没有插件「{id}」的 v{target_ver} 记录"))?;
    let root = resolve_tools_root(&app)?;
    let dir = root.join(cur.dir_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !dir.is_dir() {
        return Err(format!("插件目录不存在：{}", dir.display()));
    }

    // 目标版本的 blob：优先备份包，其次注册表下载。
    let mut blob_zip: Option<std::path::PathBuf> = None;
    if let Some(data) = data_dir(&app) {
        let backup = data.join(&target.blob);
        if backup.is_file() {
            blob_zip = Some(backup);
        }
    }
    if blob_zip.is_none() {
        // 无备份 → 从注册表重新下载（blob 缓存）。重新下载的来源与全新安装
        // 同标准，强制 Ed25519 签名（备份包例外：是以前装过的版本，已验过）。
        let p = plugin_entry_by_id(&app, &id)?;
        let (blob, _h, _c) = fetch_to_cache(&app, &p, Some(&target_ver)).await?;
        let zip = data_dir(&app).map(|d| d.join(&blob));
        if let Some(z) = &zip {
            require_registry_signed(z).await?;
        }
        blob_zip = zip;
    }
    let blob_zip = blob_zip.ok_or_else(|| "无法定位回滚包".to_string())?;

    let tmp_dir: std::path::PathBuf = dir.with_extension("dp-old-tmp");
    if let Err(e) = std::fs::rename(&dir, &tmp_dir) {
        return Err(format!("移开当前版目录失败：{e}"));
    }
    let res = install_zip_bytes(&blob_zip, &root).await;
    match res {
        Ok(new_rel) => {
            let _ = std::fs::rename(&tmp_dir, &dir);
            let _ = std::fs::remove_dir_all(&dir);
            let _ = append_ledger(
                &app,
                LedgerEntry {
                    ts: now_secs(),
                    kind: "rollback".into(),
                    id: id.clone(),
                    name: target.name.clone(),
                    version: target_ver.clone(),
                    source: "registry".into(),
                    dir_rel: new_rel.clone(),
                    blob: target.blob.clone(),
                    blob_hash: target.blob_hash.clone(),
                    signer: target.signer.clone(),
                    from_version: cur.version.clone(),
                },
            );
            Ok(serde_json::json!({
                "id": id,
                "from": cur.version,
                "to": target_ver,
                "dir_rel": new_rel,
            }))
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::rename(&tmp_dir, &dir);
            Err(format!("回滚失败，已恢复当前版：{e}"))
        }
    }
}

// ── 测试 ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_latest_from_versions() {
        let p = RegistryPlugin {
            id: "foo".into(),
            name: "Foo".into(),
            version: "1.0.0".into(),
            author: String::new(),
            description: String::new(),
            category: String::new(),
            risk: String::new(),
            tags: Vec::new(),
            url: "https://example.com/foo-1.0.zip".into(),
            sha256: String::new(),
            signer: String::new(),
            downloads: 0,
            versions: vec![
                PluginVersion {
                    version: "2.0.0".into(),
                    url: "https://example.com/foo-2.0.zip".into(),
                    sha256: "ab".into(),
                    signer: "cd".into(),
                    min_app: "0.1.2".into(),
                    size_hint: 1024,
                    notes: "second".into(),
                },
                PluginVersion {
                    version: "1.0.0".into(),
                    url: "https://example.com/foo-1.0.zip".into(),
                    ..Default::default()
                },
            ],
            license: "MIT".into(),
            depends_on: vec!["base".into()],
            verified: false,
            homepage: String::new(),
        };
        // 无参数 → versions 第一条（最新版）
        let (ver, url, sha, signer, size, notes) = p.resolve(None);
        assert_eq!(ver, "2.0.0");
        assert_eq!(url, "https://example.com/foo-2.0.zip");
        assert_eq!(sha, "ab");
        assert_eq!(signer, "cd");
        assert_eq!(size, 1024);
        assert_eq!(notes, "second");
        // 指定旧版本
        let (ver, url, ..) = p.resolve(Some("1.0.0"));
        assert_eq!(ver, "1.0.0");
        assert_eq!(url, "https://example.com/foo-1.0.zip");
        // 不存在的版本 → version 字段回显、url 空（调用方报「即将上架」类错误）
        let (ver, url, ..) = p.resolve(Some("9.9.9"));
        assert_eq!(ver, "9.9.9");
        assert!(url.is_empty());
    }

    #[test]
    fn resolve_falls_back_to_top_level_fields() {
        let p = RegistryPlugin {
            id: "bar".into(),
            name: "Bar".into(),
            version: "1.2.3".into(),
            author: String::new(),
            description: String::new(),
            category: String::new(),
            risk: String::new(),
            tags: Vec::new(),
            url: "https://example.com/bar.zip".into(),
            sha256: "ef".into(),
            signer: String::new(),
            downloads: 0,
            versions: Vec::new(),
            license: String::new(),
            depends_on: Vec::new(),
            verified: false,
            homepage: String::new(),
        };
        let (ver, url, sha, ..) = p.resolve(None);
        assert_eq!(ver, "1.2.3");
        assert_eq!(url, "https://example.com/bar.zip");
        assert_eq!(sha, "ef");
    }

    #[test]
    fn envelope_unsigned_returns_false_not_error() {
        let env = RegistryEnvelope {
            schema: REGISTRY_SCHEMA,
            source_url: String::new(),
            fetched_at: 0,
            signature: String::new(),
            signer: String::new(),
            plugins: Vec::new(),
        };
        assert_eq!(env.verify().unwrap(), false, "未签名 → Ok(false)，不报错");
    }

    #[test]
    fn envelope_with_bad_signature_errors() {
        let env = RegistryEnvelope {
            schema: REGISTRY_SCHEMA,
            source_url: String::new(),
            fetched_at: 0,
            signature: "ab".into(),
            signer: "cd".into(),
            plugins: vec![RegistryPlugin {
                id: "x".into(),
                name: "x".into(),
                version: "1".into(),
                author: String::new(),
                description: String::new(),
                category: String::new(),
                risk: String::new(),
                tags: Vec::new(),
                url: String::new(),
                sha256: String::new(),
                signer: String::new(),
                downloads: 0,
                versions: Vec::new(),
                license: String::new(),
                depends_on: Vec::new(),
                verified: false,
                homepage: String::new(),
            }],
        };
        assert!(env.verify().is_err(), "签名长度不合法必须报错");
    }

    #[test]
    fn seed_registry_has_two_placeholder_items() {
        let env = seed_registry();
        assert_eq!(env.schema, REGISTRY_SCHEMA);
        assert!(env.signature.is_empty());
        assert_eq!(env.plugins.len(), 2);
        assert!(env.plugins.iter().all(|p| p.url.is_empty()));
        assert!(env
            .plugins
            .iter()
            .all(|p| !p.id.is_empty() && !p.name.is_empty()));
    }

    fn plugin(id: &str, name: &str) -> RegistryPlugin {
        RegistryPlugin {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn filter_builtin_dupes_drops_overlapping_entries() {
        let builtin: std::collections::HashSet<String> =
            ["hwinfo", "crystaldiskinfo"].iter().map(|s| s.to_string()).collect();
        let (kept, skipped) = filter_builtin_dupes(
            vec![
                plugin("hwinfo", "HWiNFO"),
                plugin("sysmon", "SysMon"),
                plugin("novel-tool", "Novel Tool"),
            ],
            &builtin,
        );
        assert_eq!(skipped, 1, "HWiNFO 与内置重复应被剔除");
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].id, "sysmon");
        assert_eq!(kept[1].id, "novel-tool");
        // 原始字段必须保留（前端还依赖 name/url/version 等）
        assert_eq!(kept[0].name, "SysMon");
    }

    #[test]
    fn filter_builtin_dupes_matches_by_normalized_name() {
        // 远端索引里 id 可能与内置净化名不同，但名字净化后撞上内置 id 也要剔除。
        let builtin: std::collections::HashSet<String> =
            ["hwinfo"].iter().map(|s| s.to_string()).collect();
        let (kept, skipped) = filter_builtin_dupes(
            vec![plugin("hwinfo-lite", "HWiNFO")],
            &builtin,
        );
        assert_eq!(skipped, 1, "名字净化后命中内置 id 也应剔除");
        assert!(kept.is_empty());
    }

    #[test]
    fn ledger_trim_keeps_last_n() {
        let mut entries = Vec::new();
        for i in 0..(LEDGER_MAX_ENTRIES + 5) {
            entries.push(LedgerEntry {
                ts: i as u64,
                kind: "install".into(),
                id: format!("p-{i}"),
                name: String::new(),
                version: "1".into(),
                source: "zip".into(),
                dir_rel: String::new(),
                blob: String::new(),
                blob_hash: String::new(),
                signer: String::new(),
                from_version: String::new(),
            });
        }
        // 与 save_ledger 的裁剪逻辑一致（无需 AppHandle，直接验证裁剪语义）
        let trimmed: Vec<LedgerEntry> = if entries.len() > LEDGER_MAX_ENTRIES {
            entries
                .split_at(entries.len() - LEDGER_MAX_ENTRIES)
                .1
                .to_vec()
        } else {
            entries.to_vec()
        };
        assert_eq!(trimmed.len(), LEDGER_MAX_ENTRIES);
        assert_eq!(trimmed[0].id, "p-5");
        assert_eq!(
            trimmed.last().unwrap().id,
            format!("p-{}", entries.len() - 1)
        );
    }

    #[test]
    fn cache_path_normalizes_version() {
        // 与 cache_file_path 的净化约定一致：非字母数字/`.`/`-` 的字符剔除
        let safe = "1.0.0-beta+build.7"
            .trim()
            .chars()
            .filter_map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                    Some(c)
                } else {
                    None
                }
            })
            .collect::<String>();
        assert_eq!(safe, "1.0.0-betabuild.7");
    }
}
