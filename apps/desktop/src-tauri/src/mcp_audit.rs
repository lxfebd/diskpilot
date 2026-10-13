//! MCP 审计日志（从 mcp.rs 拆分，2026-10-12）。
//! 追加式 mcp-audit.jsonl：audit_path / append_audit / audit_ts / audit_args_snapshot / mcp_audit_tail。

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use super::AUDIT_FILE;

pub(crate) fn audit_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(AUDIT_FILE))
}

/// 追加一条审计日志（失败静默：审计是辅助能力，不阻塞主流程）。
pub(crate) fn append_audit(app: &AppHandle, line: serde_json::Value) {
    let Some(p) = audit_path(app) else { return };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let _ = writeln!(f, "{}", serde_json::to_string(&line).unwrap_or_default());
    }
}

pub(crate) fn audit_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 参数快照：截断 + 扁平化，脱敏（不写完整值，只写键名单与总长）。
pub(crate) fn audit_args_snapshot(args: &serde_json::Value) -> serde_json::Value {
    match args {
        serde_json::Value::Object(m) => {
            let keys: Vec<String> = m.keys().cloned().collect();
            serde_json::json!({ "keys": keys, "size": args.to_string().len() })
        }
        serde_json::Value::Null => serde_json::Value::Null,
        other => serde_json::json!({ "type": other.as_str().map(|_| "string").unwrap_or("other") }),
    }
}

/// 审计日志读取（只读 L0）：返回最近 N 条。
/// 读失败必须报错：否则前端把「读失败」当「暂无审计记录」展示
/// （诚实性族规 R81 同族）；文件不存在 = 从未有过审计 = 空列表是诚实的。
#[tauri::command]
pub(crate) fn mcp_audit_tail(
    app: AppHandle,
    limit: Option<usize>,
) -> Result<Vec<serde_json::Value>, String> {
    let Some(p) = audit_path(&app) else {
        return Err("无法定位数据目录".into());
    };
    if !p.exists() {
        // 文件不存在 = 从未有过审计记录（真空态），不是读失败。
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&p).map_err(|e| format!("读取审计日志失败: {e}"))?;
    let n = limit.unwrap_or(50).clamp(1, 200);
    Ok(text
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str(l).ok())
        .take(n)
        .collect())
}
