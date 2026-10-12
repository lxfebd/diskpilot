//! 注册表数据层（从 plugin_registry.rs 拆分，2026-10-12）。
//! 路径与原子写 / 信封·配置·台账读写。

use tauri::{AppHandle, Manager};

use super::LEDGER_MAX_ENTRIES;
use crate::plugin_registry_types::{
    LedgerEntry, RegistryConfig, RegistryEnvelope, RegistryPlugin, REGISTRY_SCHEMA,
};

// ── 路径与原子写 ───────────────────────────────────────────────────────

pub(crate) fn data_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok()
}

pub(crate) fn registry_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    data_dir(app).map(|d| d.join(super::REGISTRY_FILE))
}

pub(crate) fn config_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    data_dir(app).map(|d| d.join(super::CONFIG_FILE))
}

pub(crate) fn ledger_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    data_dir(app).map(|d| d.join(super::LEDGER_FILE))
}

/// 插件包 blob 缓存文件路径（`plugin-cache/<id>-v<version>.zip`）。
/// version 非法字符（除字母数字/`.`/`-`）一律剔除，防路径穿越。
pub(crate) fn cache_file_path(
    app: &AppHandle,
    id: &str,
    version: &str,
) -> Option<std::path::PathBuf> {
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
    data_dir(app).map(|d| d.join(super::CACHE_DIR).join(name))
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

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 内置 seed 索引：官方插件市场占位（无真实下载地址时前端禁用安装）。
/// 索引仓库后建，本轮以 URL 配置化框架落地，seed 仅在未配置索引时兜底展示。
pub(crate) fn seed_registry() -> RegistryEnvelope {
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
pub(crate) fn load_envelope(app: &AppHandle) -> (RegistryEnvelope, Option<String>) {
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

pub(crate) fn load_config(app: &AppHandle) -> RegistryConfig {
    let Some(p) = config_path(app) else {
        return RegistryConfig::default();
    };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn load_ledger(app: &AppHandle) -> Vec<LedgerEntry> {
    let Some(p) = ledger_path(app) else {
        return Vec::new();
    };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<LedgerEntry>>(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn save_ledger(app: &AppHandle, entries: &[LedgerEntry]) -> Result<(), String> {
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
pub(crate) fn append_ledger(app: &AppHandle, entry: LedgerEntry) -> Result<(), String> {
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
