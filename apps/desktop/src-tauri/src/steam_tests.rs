//! steam.rs 单元测试（从 steam.rs 拆分，2026-10-12）。
//! 商店/创意工坊 URL 白名单 / 表单形状 / 店名提取 / 缓存冷却。

use super::*;

#[test]
fn steam_url_whitelist_builds_expected_links() {
    assert_eq!(
        build_steam_url("uninstall", 730),
        Some("steam://uninstall/730".to_string())
    );
    assert_eq!(
        build_steam_url("rungameid", 570),
        Some("steam://rungameid/570".to_string())
    );
    assert_eq!(
        build_steam_url("validate", 252490),
        Some("steam://validate/252490".to_string())
    );
    assert_eq!(build_steam_url("nav", 0), Some("steam://nav/0".to_string()));
    assert_eq!(
        build_steam_url("workshop_page", 12345),
        Some("steam://url/CommunityFilePage/12345".to_string())
    );
}

#[test]
fn steam_url_rejects_arbitrary_actions_and_malformed_input() {
    // 未在白名单的动作一律拒绝（含大小写变体与看似相似的攻击串）。
    assert_eq!(build_steam_url("Uninstall", 730), None);
    assert_eq!(build_steam_url("uninstall ", 730), None);
    assert_eq!(build_steam_url("uninstall;rm", 730), None);
    assert_eq!(build_steam_url("steam://uninstall", 1), None);
    assert_eq!(build_steam_url("", 1), None);
    assert_eq!(build_steam_url("workshop", 1), None);
}

#[test]
fn published_file_form_shape() {
    assert_eq!(
        build_published_file_form(&[]),
        vec![("itemcount".to_string(), "0".to_string())]
    );
    assert_eq!(
        build_published_file_form(&[7, 42]),
        vec![
            ("itemcount".to_string(), "2".to_string()),
            ("publishedfileids[0]".to_string(), "7".to_string()),
            ("publishedfileids[1]".to_string(), "42".to_string()),
        ]
    );
}

#[test]
fn storefront_name_extraction() {
    // 正常命中。
    let v = serde_json::json!({
        "730": { "success": true, "data": { "name": "反恐精英：全球攻势" } }
    });
    assert_eq!(
        extract_storefront_name(&v, 730),
        Some("反恐精英：全球攻势".to_string())
    );

    // success=false → None（区域锁定/下架）。
    let v = serde_json::json!({ "1": { "success": false } });
    assert_eq!(extract_storefront_name(&v, 1), None);

    // 结构缺失（无 data / 无 name / 空 name）→ None。
    assert_eq!(
        extract_storefront_name(&serde_json::json!({ "2": {} }), 2),
        None
    );
    assert_eq!(
        extract_storefront_name(
            &serde_json::json!({ "3": { "success": true, "data": {} } }),
            3
        ),
        None
    );
    assert_eq!(
        extract_storefront_name(
            &serde_json::json!({ "4": { "success": true, "data": { "name": "" } } }),
            4
        ),
        None
    );

    // appid 根本不在响应里 → None。
    assert_eq!(extract_storefront_name(&serde_json::json!({}), 999), None);
}

#[test]
fn cache_entry_usable_cooldown() {
    let now = 1_000_000u64;
    // 有中文名 → 永远可用（无论失败时间戳）。
    let named = CachedName {
        name_cn: "绝地求生".into(),
        fetched_at: now,
        failed_at: 0,
    };
    assert!(cache_entry_usable(&named, now));
    let named_stale = CachedName {
        name_cn: "绝地求生".into(),
        fetched_at: now,
        failed_at: now,
    };
    assert!(cache_entry_usable(&named_stale, now));

    // 仅失败标记：冷却期内 → 可用（跳过网络）；冷却期结束 → 不可用（重试）。
    let failed = CachedName {
        name_cn: String::new(),
        fetched_at: 0,
        failed_at: now,
    };
    assert!(cache_entry_usable(&failed, now));
    assert!(cache_entry_usable(
        &CachedName {
            name_cn: String::new(),
            fetched_at: 0,
            failed_at: now
        },
        now + FAILURE_COOLDOWN_SECS - 1
    ));
    assert!(!cache_entry_usable(
        &CachedName {
            name_cn: String::new(),
            fetched_at: 0,
            failed_at: now
        },
        now + FAILURE_COOLDOWN_SECS
    ));

    // 什么标记都没有 → 不可用（需要发起 fetch）。
    let empty = CachedName {
        name_cn: String::new(),
        fetched_at: 0,
        failed_at: 0,
    };
    assert!(!cache_entry_usable(&empty, now));
}
