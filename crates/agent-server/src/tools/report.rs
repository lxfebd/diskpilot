//! 一键体检报告 + 传感器趋势 + 阈值告警（report.rs）——复刻 AIDA64 的
//! Report / 日志 / 阈值告警。
//!
//! - `system_report`（L0 只读）：汇总现有只读采集（CPU/GPU/内存/主板/温度/磁盘/
//!   网络/系统）成 Markdown 体检报告，AI 拿它当「体检结论」。
//! - `sensor_trend`（L0 只读）：把每次采样的传感器快照追加进 JSONL 历史文件，
//!   可查最近 N 条趋势（同款模式：R6 space-history.jsonl 追加式 + 去重 + 裁剪）。
//! - `sensor_alert`（L0 只读）：按阈值体检传感器（CPU 温度 / 磁盘温度 / GPU 温度
//!   / 内存占用 / 磁盘剩余），超阈值输出告警。默认只读出结果，`persist=true` 时
//!   才把历史写入磁盘文件。
//!
//! 数据目录：`%LOCALAPPDATA%/diskpilot/agent-server/`（追加式 JSONL + 原子写裁剪，
//! 失败全静默，与主项目 R6 同款容错）。
//!
//! 2026-10-12 拆分：存储 → `rep_storage`，报告/趋势 → `rep_report`，
//! 阈值告警 → `rep_alert`，格式化/解析小工具 → `rep_util`。

mod rep_alert;
mod rep_report;
mod rep_storage;
mod rep_util;

pub use rep_alert::sensor_alert;
pub use rep_report::{sensor_trend, system_report};

// 回归测试引用的内部符号。
#[cfg(test)]
pub(crate) use rep_report::trend_point;
#[cfg(test)]
pub(crate) use rep_util::{extract_first_f64, extract_free_gb, fmt_uptime};

#[cfg(test)]
mod tests {
    use super::*;

    // 注：system_report / sensor_alert 会走真实采集（PowerShell/fancmd），
    // 不放单测（环境 PowerShell 可能 30s 慢），由 stdio 冒烟验证。

    #[test]
    fn extract_first_f64_works() {
        assert_eq!(extract_first_f64("CPU 温度: 56.3 ℃"), Some(56.3));
        assert_eq!(extract_first_f64("disk temp=45.0 C"), Some(45.0));
        assert_eq!(extract_first_f64("无数字"), None);
    }

    #[test]
    fn fmt_uptime_works() {
        assert_eq!(fmt_uptime(3600), "1小时 0分");
        assert_eq!(fmt_uptime(90061), "1天 1小时 1分");
        assert_eq!(fmt_uptime(120), "2分");
    }

    #[test]
    fn extract_free_gb_hits_remaining_capacity() {
        // 直接命中：剩余 + GB 单位
        assert_eq!(
            extract_free_gb("- 磁盘剩余：120.5 GB\n"),
            Some((120.5, "- 磁盘剩余：120.5 GB".to_string()))
        );
        // 已有 SMART 文本风格：磨损行「寿命剩余约 X%」必须被跳过（健康 SSD
        // 会被误判成剩余 8GB → 虚假的低容量告警），下一行才可能是真实容量
        let smart = "- 三星SSD：SSD，NVMe，512 GB，健康状态 良好（系统盘）\n  - 磨损度：8%（寿命剩余约 92%）\n  - 通电时长：1234 小时\n  - 剩余空间：80 GB\n";
        assert_eq!(
            extract_free_gb(smart),
            Some((80.0, "- 剩余空间：80 GB".to_string()))
        );
        // 「已用 X GB / 剩余 Y GB」结构：取关键词之后的剩余值，不取行首已用值
        assert_eq!(
            extract_free_gb("已用 300GB / 剩余 120GB\n"),
            Some((120.0, "已用 300GB / 剩余 120GB".to_string()))
        );
        // TB 单位换算成 GB
        assert_eq!(
            extract_free_gb("Free space: 1.5 TB\n"),
            Some((1536.0, "Free space: 1.5 TB".to_string()))
        );
        // 关键词在行尾（"900 GB free"）
        assert_eq!(
            extract_free_gb("Disk 2: 900 GB free\n"),
            Some((900.0, "Disk 2: 900 GB free".to_string()))
        );
    }

    #[test]
    fn extract_free_gb_rejects_percent_or_no_unit() {
        // 磨损百分比行：即使含「剩余」也不当作剩余容量
        assert_eq!(extract_free_gb("  - 磨损度：8%（寿命剩余约 92%）\n"), None);
        assert_eq!(extract_free_gb("寿命剩余约 92% 健康度良好\n"), None);
        // 无容量单位（GB/TB）不去猜
        assert_eq!(extract_free_gb("剩余 300\n"), None);
        assert_eq!(extract_free_gb("无剩余空间信息"), None);
    }

    #[test]
    fn trend_point_maps_all_series_and_survives_legacy_rows() {
        // 新行：三条数据齐全（ts + CPU 温度 + 内存占用）
        let full = serde_json::json!({"ts": 1730000000i64, "cpu_temp_c": 56.5, "mem_percent": 42.0, "snapshot": "…"});
        let p = trend_point(&full);
        assert_eq!(p["ts"], 1730000000i64);
        assert_eq!(p["cpu_temp_c"], 56.5);
        assert_eq!(p["mem_percent"], 42.0);

        // 旧行（mem_percent 上线前写的）：字段缺失 → null，不虚构 0%
        let legacy = serde_json::json!({"ts": 1729999999i64, "cpu_temp_c": 51.0, "snapshot": "…"});
        let p = trend_point(&legacy);
        assert_eq!(p["cpu_temp_c"], 51.0);
        assert!(p["mem_percent"].is_null(), "缺数据的内存曲线必须是 null");

        // 温度采集失败（null）与 ts 缺失（0 兜底）都不炸
        let sparse = serde_json::json!({"cpu_temp_c": null, "mem_percent": null});
        let p = trend_point(&sparse);
        assert!(p["cpu_temp_c"].is_null());
        assert!(p["mem_percent"].is_null());
        assert_eq!(p["ts"], 0i64);
    }
}
