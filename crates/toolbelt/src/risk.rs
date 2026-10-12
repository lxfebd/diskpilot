//! 工具风险级与超时默认值。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：每个工具执行前用户需确认的最低风险级
//! （MEDIUM 起），与 toolbelt.toml 约定一致；烤机/写盘类工具延长默认超时。

use serde::Serialize;

/// 执行前需要用户在 UI 确认的最低风险级是 MEDIUM（同 toolbelt.toml 约定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// 只读/导出类，无副作用
    Low,
    /// 可逆但有副作用（烤机、整理、设备禁用…）
    Medium,
    /// 破坏性/变砖风险（刷 BIOS、分区、格式化）
    High,
}

impl Risk {
    pub fn parse(s: &str) -> Option<Risk> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Risk::Low),
            "medium" => Some(Risk::Medium),
            "high" => Some(Risk::High),
            _ => None,
        }
    }
}

impl std::fmt::Display for Risk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Risk::Low => "low",
            Risk::Medium => "medium",
            Risk::High => "high",
        };
        f.write_str(s)
    }
}

/// 每个工具的风险默认值。依据文档里的「注意事项」与副作用程度定级：
/// 工具本身静态一个级别，`toolbelt.toml` 可按目录覆盖。
pub(crate) fn default_risk(tool_name: &str) -> Risk {
    // 名字可能是 "Autoruns / autorunsc" 这种双名，统一小写包含匹配。
    let n = tool_name.to_ascii_lowercase();
    let hit = |keys: &[&str]| keys.iter().any(|k| n.contains(k));
    if hit(&["fpt", "diskgenius", "ventoy", "nvidiainspector"]) {
        Risk::High // 刷 BIOS / 分区 / 格式化目标盘 / 电压超频
    } else if hit(&[
        "prime95",
        "furmark",
        "nvidiaprofileinspector",
        "defraggler",
        "urwtest",
        "usbdeview",
        "windbg",
    ]) {
        Risk::Medium // 烤机 / 数据布局重写 / 写满测试盘 / 卸载设备 / 调试附加
    } else {
        Risk::Low
    }
}

/// 长时间运行的工具（烤机/写盘测试）默认放宽超时（秒）；其余 60s。
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const LONG_RUNNING_TIMEOUT_SECS: u64 = 1800;
pub const MIN_TIMEOUT_SECS: u64 = 5;
pub const MAX_TIMEOUT_SECS: u64 = 3600;

pub(crate) fn default_timeout(tool_name: &str) -> u64 {
    let n = tool_name.to_ascii_lowercase();
    if ["prime95", "furmark", "urwtest", "defraggler", "ventoy"]
        .iter()
        .any(|k| n.contains(k))
    {
        LONG_RUNNING_TIMEOUT_SECS
    } else {
        DEFAULT_TIMEOUT_SECS
    }
}
