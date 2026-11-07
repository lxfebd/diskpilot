//! 撤销核心（从 executor.rs 拆分，2026-10-12）。
//! undo 命令 + undo_core 纯逻辑（防 index 漂移 + Quarantine 恢复）。

use diskpilot_executor::{
    list_undo as list_undo_log, remove_restored, restore_quarantined, UndoEntry,
};
use tauri::State;

use crate::AppState;

/// Restores a quarantined item back to its original path. Only `Quarantine`
/// records are restorable; recycled/delete entries must be restored from the
/// OS recycle bin or are unrecoverable.
///
/// `index` 是展示列表里「最新在前」的位置；`source` 是用户当时点的那一行的
/// 原始路径。两者都校验：列表可能因其他操作在两次刷新之间变化导致 index
/// 漂移（恢复错条目），必须确认 `entries[index].source == source` 才动手。
#[tauri::command]
pub(crate) fn undo(
    state: State<'_, AppState>,
    index: usize,
    source: String,
    confirmed: Option<bool>,
) -> Result<Option<UndoEntry>, String> {
    // R505：还原是写操作（rename 回原路径 + create_dir_all），必须用户明确确认；
    // 绕过前端确认窗直接 invoke 一律拒绝。restore_quarantined 内部另有
    // destination 落隔离区校验 + source 受保护路径校验（防伪造 undo.jsonl 植入）。
    if confirmed != Some(true) {
        return Err(
            "undo:confirm: 还原会把隔离区文件移回原路径，必须由用户明确确认（confirmed=true）。"
                .into(),
        );
    }
    undo_core(&state.undo_log, &state.quarantine_root, index, source)
}

/// `undo` 的纯逻辑：可单测（不依赖 Tauri State）。
pub(crate) fn undo_core(
    undo_log: &std::path::Path,
    quarantine_root: &std::path::Path,
    index: usize,
    source: String,
) -> Result<Option<UndoEntry>, String> {
    let entries = list_undo_log(undo_log).map_err(|e| e.to_string())?;
    let entry = entries
        .get(index)
        .cloned()
        .ok_or_else(|| format!("undo index out of range: {index}"))?;
    // 防 index 漂移：条目已不是用户点的那一行则拒绝（重新刷新后再试）。
    if entry.source.to_string_lossy() != source {
        return Err("撤销目标已变化（列表可能被其他清理操作刷新），请刷新后再试。".into());
    }
    if !matches!(entry.action, diskpilot_executor::Action::Quarantine) {
        return Ok(None);
    }
    let restored_source =
        restore_quarantined(&entry, quarantine_root).map_err(|e| format!("undo failed: {e}"))?;
    remove_restored(undo_log, std::slice::from_ref(&restored_source)).map_err(|e| e.to_string())?;
    Ok(Some(entry))
}
#[cfg(test)]
mod tests {
    use super::*;
    use diskpilot_executor::Action;
    use std::fs;
    use std::path::Path;

    fn make_entry(action: Action, source: &Path, destination: Option<&Path>) -> UndoEntry {
        UndoEntry {
            timestamp: "2026-10-24T00:00:00Z".into(),
            action,
            source: source.to_path_buf(),
            destination: destination.map(|p| p.to_path_buf()),
            reason: "r376-fixture".into(),
            bytes_freed: Some(7),
        }
    }

    fn write_ledger(undo_log: &Path, entries: &[UndoEntry]) {
        let mut s = String::new();
        for e in entries {
            s.push_str(&serde_json::to_string(e).unwrap());
            s.push('\n');
        }
        fs::write(undo_log, s).unwrap();
    }

    /// 构造目录 + 一条 quarantine 台账：隔离文件 dst 已存在、src 不存在。
    fn fixture_with_quarantine(
        root: &Path,
        src_leaf: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let quarantine = root.join("quarantine");
        let src = root.join("orig").join(src_leaf);
        fs::create_dir_all(&quarantine).unwrap();
        let dst = quarantine.join(format!("1-0-{src_leaf}"));
        fs::write(&dst, "hello").unwrap();
        (src, dst)
    }

    #[test]
    fn undo_core_restores_matching_quarantine_entry() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (src, dst) = fixture_with_quarantine(&root, "note.txt");
        let log = root.join("undo.jsonl");
        write_ledger(&log, &[make_entry(Action::Quarantine, &src, Some(&dst))]);

        let got = undo_core(
            &log,
            &root.join("quarantine"),
            0,
            src.to_string_lossy().to_string(),
        )
        .unwrap();
        assert!(got.is_some(), "quarantine 条目应可恢复");
        assert_eq!(
            fs::read_to_string(&src).unwrap(),
            "hello",
            "文件应回到原路径"
        );
        assert!(!dst.exists(), "隔离目录里的副本应已移走");
        assert!(
            list_undo_log(&log).unwrap().is_empty(),
            "恢复后台账里不再留这条"
        );
    }

    #[test]
    fn undo_core_rejects_index_drift() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (src, dst) = fixture_with_quarantine(&root, "note.txt");
        let log = root.join("undo.jsonl");
        write_ledger(&log, &[make_entry(Action::Quarantine, &src, Some(&dst))]);

        let err = undo_core(
            &log,
            &root.join("quarantine"),
            0,
            root.join("other.txt").to_string_lossy().to_string(),
        )
        .expect_err("source 不匹配必须拒绝");
        assert!(err.contains("撤销目标已变化"), "drift 提示缺失：{err}");
        assert_eq!(
            fs::read_to_string(&dst).unwrap(),
            "hello",
            "拒绝时不能动隔离文件"
        );
    }

    #[test]
    fn undo_core_index_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let log = root.join("undo.jsonl");
        write_ledger(
            &log,
            &[make_entry(Action::Quarantine, &root.join("a.txt"), None)],
        );

        let err = undo_core(&log, &root.join("quarantine"), 5, "x".into())
            .expect_err("越界 index 必须报错");
        assert!(err.contains("index out of range"), "越界提示缺失：{err}");
    }

    #[test]
    fn undo_core_non_quarantine_returns_none_keeps_log() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let log = root.join("undo.jsonl");
        let src = root.join("r.txt");
        write_ledger(&log, &[make_entry(Action::Recycle, &src, None)]);

        let got = undo_core(
            &log,
            &root.join("quarantine"),
            0,
            src.to_string_lossy().to_string(),
        )
        .unwrap();
        assert!(got.is_none(), "recycle/delete 条目不可恢复，应返回 None");
        assert_eq!(list_undo_log(&log).unwrap().len(), 1, "台账应保持原样");
    }

    #[test]
    fn undo_core_missing_quarantined_file_fails() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let quarantine = root.join("quarantine");
        let src = root.join("orig").join("note.txt");
        fs::create_dir_all(&quarantine).unwrap();
        let dst = quarantine.join("gone.txt"); // 不创建
        let log = root.join("undo.jsonl");
        write_ledger(&log, &[make_entry(Action::Quarantine, &src, Some(&dst))]);

        let err = undo_core(&log, &quarantine, 0, src.to_string_lossy().to_string())
            .expect_err("隔离文件不存在必须失败");
        assert!(err.starts_with("undo failed:"), "失败原因应透传：{err}");
        assert!(!src.exists(), "src 不应存在");
    }

    #[test]
    fn undo_core_index_is_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (src_old, dst_old) = fixture_with_quarantine(&root, "old.txt");
        fs::write(&dst_old, "old").unwrap();
        let (src_new, dst_new) = fixture_with_quarantine(&root, "new.txt");
        let log = root.join("undo.jsonl");
        // 文件顺序 = 旧在前、新在后；list_undo 反转为「最新在前」，index 0 应是 new
        write_ledger(
            &log,
            &[
                make_entry(Action::Quarantine, &src_old, Some(&dst_old)),
                make_entry(Action::Quarantine, &src_new, Some(&dst_new)),
            ],
        );

        let got = undo_core(
            &log,
            &root.join("quarantine"),
            0,
            src_new.to_string_lossy().to_string(),
        )
        .unwrap();
        assert!(got.is_some());
        assert_eq!(
            fs::read_to_string(&src_new).unwrap(),
            "hello",
            "恢复的应是新条目（new.txt）"
        );
        assert!(!src_old.exists(), "旧条目不得被误恢复");
    }

    #[test]
    fn undo_core_refuses_when_original_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (src, _dst) = fixture_with_quarantine(&root, "note.txt");
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::write(&src, "already there").unwrap();
        let log = root.join("undo.jsonl");
        write_ledger(&log, &[make_entry(Action::Quarantine, &src, Some(&_dst))]);

        let err = undo_core(
            &log,
            &root.join("quarantine"),
            0,
            src.to_string_lossy().to_string(),
        )
        .expect_err("原路径已存在必须拒绝");
        assert!(
            err.contains("original path already exists"),
            "存在性守卫缺失：{err}"
        );
    }
}
