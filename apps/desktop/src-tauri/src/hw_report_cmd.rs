//! 硬件报告/电源计划命令域（从 hw.rs 拆分，2026-10-12）。
//! hw_report / HwReportOut / power_plan。

use std::time::Duration;

use tauri::AppHandle;

use super::hw_core::{hw_info_cached, ps_capture};
use super::hw_disk::hw_disk_cached;
use crate::hw_report::{build_hw_markdown, html_wrap};

/// 生成硬件检测报告（markdown / json / html），可选保存到用户指定路径。
/// 只读：不修改任何硬件设置。保存路径白名单 .md/.html/.json/.txt，
/// 拒绝覆盖已有文件、拒绝写入系统目录。
#[tauri::command]
pub(crate) async fn hw_report(
    app: AppHandle,
    format: Option<String>,
    save_path: Option<String>,
) -> Result<HwReportOut, String> {
    tokio::task::spawn_blocking(move || {
        // 复用 TTL 缓存：5 分钟内再次导出不重跑 30-60s PowerShell，也不重复
        // 启动 CrystalDiskInfo；冷启动时才各自采集一次并落缓存。
        let info = hw_info_cached(false)?;
        // 磁盘健康读取失败时如实带进报告正文（错误原因），而不是静默当 None。
        let (health, health_note) = match hw_disk_cached(&app, false) {
            Ok(v) => (Some(v), String::new()),
            Err(e) => (None, format!("（硬盘健康读取失败：{e}）")),
        };

        let fmt = format.unwrap_or_else(|| "markdown".to_string());
        let (content, _ext) = match fmt.as_str() {
            "json" => (
                serde_json::to_string_pretty(
                    &serde_json::json!({ "info": info, "health": health, "note": health_note }),
                )
                .unwrap_or_default(),
                "json",
            ),
            "html" => {
                let md = build_hw_markdown(&info, health.as_ref(), &health_note);
                (html_wrap(&md), "html")
            }
            _ => (
                build_hw_markdown(&info, health.as_ref(), &health_note),
                "md",
            ),
        };

        // 报告生成成功后自动归档一份快照（增强功能：失败不影响报告返回）。
        // 归档走独立模块，绝不重跑采集、绝不阻塞主流程。
        crate::hw_history::archive_snapshot(&app, &info, &health, &health_note);

        let mut saved: Option<String> = None;
        let note;
        if let Some(sp) = save_path.filter(|s| !s.trim().is_empty()) {
            let p = std::path::PathBuf::from(sp.trim());
            if !p.is_absolute() {
                return Err("保存路径必须是绝对路径".into());
            }
            let ext_ok = matches!(
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .as_deref(),
                Some("md") | Some("markdown") | Some("html") | Some("json") | Some("txt"),
            );
            if !ext_ok {
                return Err("报告只能保存为 .md / .html / .json / .txt".into());
            }
            let lower = p.to_string_lossy().to_lowercase();
            let sys_root = std::env::var("SystemRoot")
                .unwrap_or_else(|_| "C:\\Windows".to_string())
                .to_lowercase();
            if lower.starts_with(&sys_root)
                || lower.contains("system32")
                || lower.contains("windows\\system")
            {
                return Err("拒绝写入系统目录".into());
            }
            let parent = p.parent().ok_or_else(|| "保存路径没有父目录".to_string())?;
            if !parent.is_dir() {
                return Err(format!("目录不存在：{}", parent.display()));
            }
            if p.exists() {
                return Err(format!(
                    "文件已存在：{} —— 报告不会覆盖已有文件，请换个文件名。",
                    p.display()
                ));
            }
            std::fs::write(&p, &content).map_err(|e| format!("写入失败：{e}"))?;
            saved = Some(p.display().to_string());
            note = format!("报告已保存到 {}", p.display());
        } else {
            note = "未指定保存路径，仅返回内容预览。".to_string();
        }

        Ok(HwReportOut {
            format: fmt,
            markdown: content,
            saved_path: saved,
            note,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize)]
pub(crate) struct HwReportOut {
    format: String,
    markdown: String,
    saved_path: Option<String>,
    note: String,
}

/// 调整电源计划（L2 授权操作）：每次仍需 user_confirmed=true。
/// 只支持 balanced / high_performance / power_saver 三个已知方案，
/// 其余按 GUID 原样透传。
#[tauri::command]
pub(crate) async fn power_plan(
    set_scheme: Option<String>,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    if let Some(scheme) = set_scheme.as_deref() {
        if confirmed != Some(true) {
            return Err(
                "调整电源计划需要用户明确确认（confirmed=true）。请先在确认面板中让用户确认。"
                    .into(),
            );
        }
        let scheme_lower = scheme.to_ascii_lowercase();
        // GUID 透传只放宽给「标准 8-4-4-4-12 十六进制格式」：
        // 长度 36 + 连字符位置 + 全十六进制，杜绝把任意字符串塞进 powercfg。
        let is_std_guid = |g: &str| {
            if g.len() != 36 {
                return false;
            }
            let bytes = g.as_bytes();
            for (i, &b) in bytes.iter().enumerate() {
                if [8, 13, 18, 23].contains(&i) {
                    if b != b'-' {
                        return false;
                    }
                } else if !(b.is_ascii_hexdigit()) {
                    return false;
                }
            }
            true
        };
        let guid = match scheme_lower.as_str() {
            "balanced" | "平衡" => "381b4222-f694-41f0-9685-ff5bb260df2e",
            "high_performance" | "高性能" => "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c",
            "power_saver" | "节能" => "a1841308-3541-4fab-bc81-f71556f20b4a",
            g if is_std_guid(g) => g,
            _ => {
                return Err(format!(
                    "未知的电源计划：{scheme}。可用 balanced / high_performance / power_saver"
                ))
            }
        };
        ps_capture(
            &format!("powercfg /setactive {guid}"),
            Duration::from_secs(15),
        )
        .map_err(|e| e.to_string())?;
    }
    let list = ps_capture("powercfg /list", Duration::from_secs(15)).map_err(|e| e.to_string())?;
    let mut schemes: Vec<String> = Vec::new();
    let mut active: Option<String> = None;
    for line in list.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with('*') {
            active = Some(t.trim_start_matches('*').trim().to_string());
        }
        if t.contains("GUID") {
            schemes.push(t.to_string());
        }
    }
    Ok(serde_json::json!({ "active": active, "schemes": schemes }))
}
