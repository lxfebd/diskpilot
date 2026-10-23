//! 按 scaffold 授权自动进清单（health-butler P2-1）：用户可为某个 scaffold 设一条
//! 规则——「该 scaffold 本次可清量 ≥ 阈值时，自动进总览建议清单」。**只影响清单**，
//! 不触碰执行：真执行永远只有用户逐项确认后的 `execute_ai_plan(user_confirmed)`
//! 一条路（铁律不变）。本模块没有任何写操作，只做规则存储与"该不该自动挂上"的纯判断。
//!
//! 设计约束（对齐 cleanup/reminder 族规）：
//! - **配置持久化**：`app_data_dir()/auto-list.json`（读→合并→写；读失败静默空规则集，
//!   与 reminder 同口径——增强功能不得因配置损坏而阻塞扫描/建议主流程）。
//! - **纯函数优先**：`should_auto_list` 等判定不碰 IO，便于单测；命令层只做装配。
//! - **诚实性**：规则里点名的 scaffold 在本机 scaffolds 中不存在 → 该规则闲置而非
//!   悄悄用一个近似 id 顶上（闲置原因由 `rule_status` 报出，UI 明示）。
//! - **不放宽安全边界**：自动挂上的仍是"待用户确认的建议项"，不是待执行项。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// 一条授权规则：某 scaffold 可清量达到 `min_bytes` 时自动进清单。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct AutoListRule {
    /// scaffold id（如 `wechat`）。必须能在本机 scaffolds 里找到，否则闲置。
    pub scaffold_id: String,
    /// 触发阈值（字节）。达到即自动进清单。0 = 一有可清量就进。
    pub min_bytes: u64,
}

/// 规则集（持久化到 auto-list.json）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AutoListConfig {
    /// 总开关。false 时任何规则都不生效（与 reminder.enabled 同语义）。
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<AutoListRule>,
}

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("auto-list.json"))
}

/// 读配置。缺文件 = 从未设置 → 空配置（真空态诚实）；文件存在但读失败/解析失败 →
/// 空配置 + 静默（增强功能不阻塞主流程，与 reminder 的 config_at 同口径）。
pub(crate) fn config_at(app: &AppHandle) -> AutoListConfig {
    let Some(p) = config_path(app) else {
        return AutoListConfig::default();
    };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 写配置（读旧值合并，避免翻一个开关抹掉其他规则）。
pub(crate) fn set_config(app: &AppHandle, next: AutoListConfig) -> Result<AutoListConfig, String> {
    let Some(p) = config_path(app) else {
        return Err("无法定位数据目录".into());
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&next).map_err(|e| e.to_string())?;
    std::fs::write(&p, text).map_err(|e| e.to_string())?;
    Ok(next)
}

/// 读配置（命令层）。
#[tauri::command]
pub(crate) fn get_auto_list_config(app: AppHandle) -> AutoListConfig {
    config_at(&app)
}

/// 写配置（命令层）。total 覆写：设置页持有的就是完整规则集。
#[tauri::command]
pub(crate) fn set_auto_list_config(
    app: AppHandle,
    config: AutoListConfig,
) -> Result<AutoListConfig, String> {
    set_config(&app, config)
}

/// 一条规则连同其在本机 scaffolds 下的生效状态（UI 用它明示闲置原因）。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AutoListRuleView {
    #[serde(flatten)]
    pub rule: AutoListRule,
    /// ok / unknown_scaffold（见 `rule_status`）。
    pub status: &'static str,
}

/// 规则集 + 每条规则的状态（命令层）。`known_scaffolds` 由调用方从真实 scaffolds 传入，
/// 避免本模块反向依赖 scaffold 加载路径。
#[tauri::command]
pub(crate) fn get_auto_list_config_with_status(
    app: AppHandle,
    known_scaffolds: Vec<String>,
) -> AutoListConfigView {
    let cfg = config_at(&app);
    let rules = cfg
        .rules
        .iter()
        .map(|r| AutoListRuleView {
            status: rule_status(r, &known_scaffolds),
            rule: r.clone(),
        })
        .collect();
    AutoListConfigView {
        enabled: cfg.enabled,
        rules,
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AutoListConfigView {
    pub enabled: bool,
    pub rules: Vec<AutoListRuleView>,
}

/// 从本轮建议里挑出「已授权且过阈值」的 scaffold——**只用于主动告知用户**
/// （免去"想起来要看"），不生成任何执行计划。
/// 返回 (scaffold_id, bytes) 对，按 scaffold_id 排序保证同输入同输出。
pub(crate) fn tripped_rules(
    suggestions: &[(String, u64)],
    cfg: &AutoListConfig,
) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = suggestions
        .iter()
        .filter(|(id, bytes)| should_auto_list(id, *bytes, cfg))
        .cloned()
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 规则在已知 scaffold id 集合下的状态：
/// - `ok`：id 有效，规则生效；
/// - `unknown_scaffold`：id 不在本机 scaffolds 中 → 规则闲置（不猜近似 id）。
pub(crate) fn rule_status(rule: &AutoListRule, known: &[String]) -> &'static str {
    if known.iter().any(|k| k == &rule.scaffold_id) {
        "ok"
    } else {
        "unknown_scaffold"
    }
}

/// 纯判断：给定某 scaffold 的本轮可清量与规则集，返回是否该自动进清单。
/// - 总开关关 → false（规则再匹配也不生效）；
/// - 无规则命中 → false；
/// - 多规则命中同一 scaffold 取**最低阈值**（更宽松者优先：用户既已授权，取更低的
///   阈值才不会让"配了 5GB 却因为另一条 1GB 规则没生效"这种意外发生）。
pub(crate) fn should_auto_list(scaffold_id: &str, bytes: u64, cfg: &AutoListConfig) -> bool {
    if !cfg.enabled {
        return false;
    }
    cfg.rules
        .iter()
        .filter(|r| r.scaffold_id == scaffold_id)
        .map(|r| r.min_bytes)
        .min()
        .is_some_and(|min| bytes >= min)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(enabled: bool, rules: Vec<(&str, u64)>) -> AutoListConfig {
        AutoListConfig {
            enabled,
            rules: rules
                .into_iter()
                .map(|(id, b)| AutoListRule {
                    scaffold_id: id.into(),
                    min_bytes: b,
                })
                .collect(),
        }
    }

    #[test]
    fn disabled_config_never_auto_lists_even_when_rule_matches() {
        let c = cfg(false, vec![("wechat", 0)]);
        assert!(
            !should_auto_list("wechat", 999_999, &c),
            "总开关关 = 一律不自动"
        );
    }

    #[test]
    fn no_rule_means_no_auto_list() {
        let c = cfg(true, vec![("wechat", 0)]);
        assert!(
            !should_auto_list("chrome", 500, &c),
            "没授权过的 scaffold 不自动进"
        );
    }

    #[test]
    fn threshold_is_inclusive() {
        let c = cfg(true, vec![("wechat", 5 * 1024 * 1024)]);
        assert!(
            !should_auto_list("wechat", 5 * 1024 * 1024 - 1, &c),
            "差 1 字节不触发"
        );
        assert!(
            should_auto_list("wechat", 5 * 1024 * 1024, &c),
            "达到即触发"
        );
    }

    #[test]
    fn zero_threshold_fires_on_any_bytes() {
        let c = cfg(true, vec![("wechat", 0)]);
        assert!(
            should_auto_list("wechat", 1, &c),
            "阈值为 0 = 一有可清量就进"
        );
    }

    #[test]
    fn multiple_rules_take_the_lowest_threshold() {
        let c = cfg(true, vec![("wechat", 10), ("wechat", 3)]);
        // 5 ≥ 3 但不 ≥ 10：取最低阈值才触发，避免"配了却因另一条更严规则没生效"。
        assert!(should_auto_list("wechat", 5, &c));
        assert!(!should_auto_list("wechat", 2, &c));
    }

    #[test]
    fn rule_status_flags_unknown_scaffold_instead_of_guessing() {
        let known = vec!["wechat".to_string()];
        let ok = AutoListRule {
            scaffold_id: "wechat".into(),
            min_bytes: 0,
        };
        let ghost = AutoListRule {
            scaffold_id: "wei-chat".into(),
            min_bytes: 0,
        };
        assert_eq!(rule_status(&ok, &known), "ok");
        // 拼错/不存在的 id 必须报 unknown_scaffold，不许悄悄近似匹配。
        assert_eq!(rule_status(&ghost, &known), "unknown_scaffold");
    }

    #[test]
    fn tripped_rules_filters_and_sorts_deterministically() {
        let c = cfg(true, vec![("wechat", 5)]);
        let sugg = vec![
            ("chrome".to_string(), 999u64), // 未授权 → 剔除
            ("wechat".to_string(), 4u64),   // 未达阈值 → 剔除
        ];
        assert!(tripped_rules(&sugg, &c).is_empty(), "没命中就不该报");

        let sugg2 = vec![
            ("wechat".to_string(), 7u64),   // 命中
            ("chrome".to_string(), 999u64), // 剔除
        ];
        assert_eq!(
            tripped_rules(&sugg2, &c),
            vec![("wechat".to_string(), 7u64)]
        );
    }

    #[test]
    fn tripped_rules_disabled_config_returns_empty() {
        let c = cfg(false, vec![("wechat", 0)]);
        let sugg = vec![("wechat".to_string(), 9999u64)];
        assert!(tripped_rules(&sugg, &c).is_empty(), "总开关关 = 不主动告知");
    }
}
