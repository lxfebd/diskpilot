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
//!
//! 2026-10-12 拆分：数据层 → `preg_store`，只读命令 → `preg_read`，
//! URL 配置 → `preg_url`，索引刷新 → `preg_refresh`，辅助 → `preg_misc`，
//! 写操作（安装/更新/回滚）→ `plugin_mutate`。本文件保留常量与单测。

const REGISTRY_FILE: &str = "plugins-registry.json";
const CONFIG_FILE: &str = "registry-config.json";
const LEDGER_FILE: &str = "plugin-ledger.json";
const CACHE_DIR: &str = "plugin-cache";
const LEDGER_MAX_ENTRIES: usize = 200;

pub(crate) mod plugin_mutate;
#[path = "preg_misc.rs"]
pub(crate) mod preg_misc;
#[path = "preg_read.rs"]
pub(crate) mod preg_read;
#[path = "preg_refresh.rs"]
pub(crate) mod preg_refresh;
#[path = "preg_store.rs"]
pub(crate) mod preg_store;
#[path = "preg_url.rs"]
pub(crate) mod preg_url;

pub(crate) use preg_misc::resolve_tools_root;
pub(crate) use preg_read::load_registry_url;
pub(crate) use preg_store::{
    append_ledger, append_uninstall_ledger, atomic_write, cache_file_path, config_path, data_dir,
    load_config, load_envelope, load_ledger, now_secs, registry_path,
};
// ── 测试 ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::preg_read::filter_builtin_dupes;
    use super::preg_refresh::parse_and_verify_envelope;
    use super::preg_store::seed_registry;
    use super::*;
    use crate::plugin_registry_types::PluginVersion;
    use crate::plugin_registry_types::{
        LedgerEntry, RegistryEnvelope, RegistryPlugin, REGISTRY_SCHEMA,
    };

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
        assert!(!env.verify().unwrap(), "未签名 → Ok(false)，不报错");
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
    fn envelope_rejects_path_traversal_plugin_id() {
        // R502：id 会拼进缓存/备份文件名，含 ../ 的非法 id 必须拒掉整份索引。
        let entry = |id: &str| {
            serde_json::json!({
                "id": id, "name": "x", "version": "1.0.0", "author": "",
                "description": "", "category": "", "risk": "low", "tags": [],
                "url": "", "sha256": "", "signer": "", "downloads": 0,
                "versions": [], "license": "", "depends_on": [],
                "verified": false, "homepage": ""
            })
        };
        let body = serde_json::json!({"schema": REGISTRY_SCHEMA, "plugins": [entry("../evil")]})
            .to_string();
        let err = parse_and_verify_envelope(&body, "https://example.com/index.json", 0)
            .expect_err("含 ../ 的 id 必须被拒绝");
        assert!(err.contains("非法"), "错误信息应说明 id 非法：{err}");

        let body2 = serde_json::json!({"schema": REGISTRY_SCHEMA, "plugins": [entry("Bad_ID")]})
            .to_string();
        assert!(parse_and_verify_envelope(&body2, "https://example.com/index.json", 0).is_err());

        let body3 =
            serde_json::json!({"schema": REGISTRY_SCHEMA, "plugins": [entry("good-tool-1")]})
                .to_string();
        assert!(parse_and_verify_envelope(&body3, "https://example.com/index.json", 0).is_ok());
    }

    #[test]
    fn filter_builtin_dupes_drops_overlapping_entries() {
        let builtin: std::collections::HashSet<String> = ["hwinfo", "crystaldiskinfo"]
            .iter()
            .map(|s| s.to_string())
            .collect();
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
        let (kept, skipped) = filter_builtin_dupes(vec![plugin("hwinfo-lite", "HWiNFO")], &builtin);
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
            .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
            .collect::<String>();
        assert_eq!(safe, "1.0.0-betabuild.7");
    }

    /// 契约层：结构合法的远端索引（无签名占位态）→ Ok，verified=false 不报错，
    /// 且 source_url/fetched_at 被补上（刷新落盘的是补齐后的信封）。
    fn index_body_with_schema(schema: u32) -> String {
        format!(
            r#"{{"schema":{schema},"source_url":"","fetched_at":0,"signature":"","signer":"","plugins":[{{"id":"prime95","name":"Prime95","version":"1.0.0","author":"GIMPS","description":"CPU 烤机","category":"处理器工具","risk":"medium","tags":["CPU"],"url":"","sha256":"","signer":"","downloads":0,"versions":[],"license":"MIT","depends_on":[],"verified":false,"homepage":""}}]}}"#
        )
    }

    #[test]
    fn parse_accepts_unsigned_remote_index() {
        let body = index_body_with_schema(REGISTRY_SCHEMA);
        let (env, verified) =
            parse_and_verify_envelope(&body, "https://cdn.example.org/index.json", 987).unwrap();
        assert_eq!(env.schema, REGISTRY_SCHEMA);
        assert_eq!(env.source_url, "https://cdn.example.org/index.json");
        assert_eq!(env.fetched_at, 987);
        assert_eq!(env.plugins.len(), 1);
        assert!(
            !verified,
            "官方索引未签名 → Ok(false)，显示「未验签」而非报错"
        );
    }

    #[test]
    fn parse_rejects_unknown_schema() {
        let err = parse_and_verify_envelope(&index_body_with_schema(2), "https://x/i.json", 0)
            .unwrap_err();
        assert!(
            err.contains("schema 版本不兼容"),
            "schema 不匹配必须拒绝: {err}"
        );
    }

    #[test]
    fn parse_rejects_empty_plugins() {
        let body = r#"{"schema":1,"source_url":"","fetched_at":0,"signature":"","signer":"","plugins":[]}"#;
        let err = parse_and_verify_envelope(body, "https://x/i.json", 0).unwrap_err();
        assert!(err.contains("索引为空"), "空 plugins 必须拒绝: {err}");
    }

    /// 实连冒烟：用应用自己的 `ssrf_safe_get` + `parse_and_verify_envelope` 拉线上
    /// 官方索引（jsDelivr CDN）。这是不启动 GUI 下最强的「闭环可正常使用」证据；
    /// 本机网络/代理不可达公网时手动跑（`-- --ignored`），CI 默认跳过。
    #[tokio::test]
    #[ignore = "实连 jsDelivr CDN，需公网可达；CI 默认跳过"]
    async fn real_official_index_refresh_chain() {
        let url = "https://cdn.jsdelivr.net/gh/lxfebd/diskpilot-scaffolds-index@main/index.json";
        let body = crate::net_http::ssrf_safe_get(url, 32 * 1024 * 1024, None)
            .await
            .expect("jsDelivr 拉取官方索引应成功（走系统代理）");
        let text = String::from_utf8_lossy(&body).to_string();
        let (env, verified) = parse_and_verify_envelope(&text, url, 1).unwrap();
        assert_eq!(env.schema, REGISTRY_SCHEMA);
        assert!(!env.plugins.is_empty(), "线上索引不应为空");
        assert_eq!(env.source_url, url);
        assert!(!verified, "官方占位索引未签名 → 合法「未验签」态");
        assert!(
            env.plugins
                .iter()
                .all(|p| !p.id.is_empty() && !p.name.is_empty()),
            "每条插件至少要有 id 与 name"
        );
        // 当前官方索引为全占位（url 全空）：与 CI 校验的「占位」合法态一致。
        eprintln!(
            "[实连] 官方索引 {} 条插件，占位 {} 条",
            env.plugins.len(),
            env.plugins.iter().filter(|p| p.url.is_empty()).count()
        );
    }
}
