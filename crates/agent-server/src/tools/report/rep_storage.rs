//! 数据存储域（从 report.rs 拆分，2026-10-12）。
//! data_dir / append_jsonl / read_jsonl / trim_jsonl / ts_secs。

use std::io::Write;
use std::path::PathBuf;

/// 数据目录：优先非空环境变量 DISKPILOT_DATA_DIR（R482：主进程注入的统一数据根，
/// 与主项目 app_data_dir 同根）；未设置时回退 %LOCALAPPDATA%/diskpilot/agent-server/
///（旧目录保留不迁移）。创建失败给 None（静默容错）。
pub(crate) fn data_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("DISKPILOT_DATA_DIR") {
        if !dir.trim().is_empty() {
            let dir = PathBuf::from(dir);
            let _ = std::fs::create_dir_all(&dir);
            return Some(dir);
        }
    }
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("TEMP"))
        .ok()?;
    let dir = PathBuf::from(base).join("diskpilot").join("agent-server");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

/// 追加一行 JSONL（失败静默）。
pub(crate) fn append_jsonl(file: &str, line: &str) {
    let Some(dir) = data_dir() else { return };
    let path = dir.join(file);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// 读 JSONL 全部行，返回 Vec<serde_json::Value>（失败/空 → []）。
pub(crate) fn read_jsonl(file: &str) -> Vec<serde_json::Value> {
    let Some(dir) = data_dir() else { return vec![] };
    let path = dir.join(file);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect()
}

/// 裁剪 JSONL 到最多 `max` 条（原子写：临时文件 + rename，失败静默保留旧文件）。
pub(crate) fn trim_jsonl(file: &str, max: usize) {
    let Some(dir) = data_dir() else { return };
    let path = dir.join(file);
    let rows = read_jsonl(file);
    if rows.len() <= max {
        return;
    }
    let tail = &rows[rows.len().saturating_sub(max)..];
    let tmp = dir.join(format!("{file}.tmp"));
    let ok = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        for r in tail {
            writeln!(f, "{r}")?;
        }
        f.sync_all()?;
        Ok(())
    })();
    if ok.is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

pub(crate) fn ts_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
