//! Integration tests for diskpilot-executor.
//!
//! Covers the four execution modes (dry-run / recycle / quarantine / delete),
//! the undo log lifecycle (list_undo / restore_quarantined / remove_restored),
//! and the safety red lines (no out-of-quarantine restore, no restore onto an
//! existing source, no crash on missing paths).

use diskpilot_executor::{execute, list_undo, remove_restored, restore_quarantined, Action, Plan};
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

fn plan(action: Action, paths: Vec<PathBuf>) -> Plan {
    Plan {
        action,
        paths,
        reason: "test".into(),
    }
}

/// Creates a fresh temp workspace: {tmp}/src/<file/dir> + {tmp}/quarantine + {tmp}/undo.jsonl
struct Workspace {
    tmp: tempfile::TempDir,
    src: PathBuf,
    quarantine: PathBuf,
    undo_log: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        let quarantine = tmp.path().join("quarantine");
        let undo_log = tmp.path().join("undo.jsonl");
        std::fs::create_dir_all(&src).unwrap();
        Self {
            tmp,
            src,
            quarantine,
            undo_log,
        }
    }
    fn file(&self, name: &str) -> PathBuf {
        let p = self.src.join(name);
        std::fs::write(&p, "hello").unwrap();
        p
    }
    fn subdir(&self, name: &str) -> PathBuf {
        let p = self.src.join(name);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("inner.txt"), "x").unwrap();
        p
    }
}

// ============================================================================
// dry-run
// ============================================================================

#[test]
fn dry_run_writes_log_but_touches_nothing() {
    let w = Workspace::new();
    let f = w.file("a.dat");

    let entries = execute(
        &plan(Action::Quarantine, vec![f.clone()]),
        true,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    // File untouched, nothing moved into quarantine.
    assert!(f.exists());
    assert!(!w.quarantine_root().join("0-a.dat").exists());
    // Every entry is marked dry-run.
    assert!(entries.iter().all(|e| e.reason.contains("dry-run")));
    // Log was written even in dry-run mode.
    assert_eq!(list_undo(&w.undo_log).unwrap().len(), 1);
}

// ============================================================================
// quarantine + restore lifecycle
// ============================================================================

#[test]
fn quarantine_moves_file_and_restore_brings_it_back() {
    let w = Workspace::new();
    let f = w.file("important.dat");

    let entries = execute(
        &plan(Action::Quarantine, vec![f.clone()]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    assert_eq!(entries.len(), 1);
    assert!(!f.exists(), "original must be moved out");
    let entry = &entries[0];
    assert_eq!(entry.action, Action::Quarantine);
    let dst = entry
        .destination
        .as_ref()
        .expect("quarantine entry has destination");
    assert!(dst.starts_with(w.quarantine_root()));
    assert!(dst.exists(), "item must live under quarantine");

    // Restore via public API.
    let restored = restore_quarantined(entry, w.quarantine_root()).unwrap();
    assert_eq!(restored, f);
    assert!(f.exists(), "restored file must be back");
    assert!(!dst.exists(), "quarantine slot must be emptied");
}

#[test]
fn quarantine_directory_moves_whole_tree() {
    let w = Workspace::new();
    let d = w.subdir("logs");

    let entries = execute(
        &plan(Action::Quarantine, vec![d.clone()]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    assert!(!d.exists());
    let dst = entries[0].destination.as_ref().unwrap();
    assert!(
        dst.join("inner.txt").exists(),
        "directory tree must move intact"
    );
}

#[test]
fn quarantine_dedup_same_leaf_names_keep_separate_slots() {
    let w = Workspace::new();
    // Two different directories, same leaf name "cache" -> two distinct slots.
    let d1 = w.subdir("a/cache");
    let d2 = w.subdir("b/cache");

    let entries = execute(
        &plan(Action::Quarantine, vec![d1, d2]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    assert_eq!(entries.len(), 2);
    let mut slots: Vec<PathBuf> = entries
        .iter()
        .map(|e| e.destination.clone().unwrap())
        .collect();
    slots.sort();
    slots.dedup();
    assert_eq!(slots.len(), 2, "two identical leaf names must not collide");
}

// ============================================================================
// delete
// ============================================================================

#[test]
fn delete_removes_file_and_directory() {
    let w = Workspace::new();
    let f = w.file("trash.dat");
    let d = w.subdir("tempdir");

    let entries = execute(
        &plan(Action::Delete, vec![f.clone(), d.clone()]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    assert_eq!(entries.len(), 2);
    assert!(!f.exists());
    assert!(!d.exists());
    assert_eq!(entries[0].action, Action::Delete);
    assert!(entries[0].destination.is_none());
}

#[test]
fn delete_missing_path_is_recorded_without_panic() {
    let w = Workspace::new();
    let ghost = w.src.join("never-existed.dat");

    // Must not panic: missing path is logged (reported as processed) and the
    // remaining entries still land in the log.
    let entries = execute(
        &plan(Action::Delete, vec![ghost]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();
    assert_eq!(entries.len(), 1);
    assert!(w.undo_log.exists());
}

// ============================================================================
// recycle
// ============================================================================

#[test]
fn recycle_moves_file_to_os_trash() {
    let w = Workspace::new();
    let f = w.file("junk.tmp");

    let entries = execute(
        &plan(Action::Recycle, vec![f.clone()]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    assert_eq!(entries.len(), 1);
    assert!(
        !f.exists(),
        "recycled file must leave its original location"
    );
    assert_eq!(entries[0].action, Action::Recycle);
    assert!(entries[0].destination.is_none());
}

#[test]
#[cfg(windows)] // OpenOptions::share_mode 是 Windows 专属 API（独占句柄制造锁定）
fn recycle_skips_unrecyclable_but_continues_others() {
    let w = Workspace::new();
    // 一个被独占打开（share_mode=NONE）的锁定文件（recycle 会失败）+
    // 一个真实文件：失败项被跳过，真实项照常进回收站、写 undo——总览页
    // 「清理 0 B」的根因之一就是整批因单项失败 `?` 中断，这里锁定新语义。
    let locked = w.file("locked.tmp");
    let _handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0) // 拒绝一切共享：独占句柄，阻止回收/删除
        .open(&locked)
        .expect("open with share-none");
    let f = w.file("keep.tmp");

    let entries = execute(
        &plan(Action::Recycle, vec![locked.clone(), f.clone()]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    assert_eq!(entries.len(), 1, "只有能回收的项被记录");
    assert_eq!(entries[0].source, f);
    assert!(locked.exists(), "被占用文件留在原地（安全方向）");
    assert!(!f.exists(), "真实文件应进回收站");
    // 锁定项不生成错误：execute 仍返回 Ok（跳过而非中断）。
    assert!(w.undo_log.exists());
}

// ============================================================================
// undo log
// ============================================================================

#[test]
fn list_undo_returns_newest_first() {
    let w = Workspace::new();
    execute(
        &plan(Action::Delete, vec![w.file("one.dat")]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();
    execute(
        &plan(Action::Delete, vec![w.file("two.dat")]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    let all = list_undo(&w.undo_log).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].source, w.src.join("two.dat"), "newest entry first");
    assert_eq!(all[1].source, w.src.join("one.dat"));
}

#[test]
fn list_undo_on_missing_log_is_empty() {
    let w = Workspace::new();
    assert!(list_undo(&w.undo_log).unwrap().is_empty());
}

#[test]
fn remove_restored_cleans_quarantine_entries_from_log() {
    let w = Workspace::new();
    let f = w.file("keep.dat");
    let entries = execute(
        &plan(Action::Quarantine, vec![f.clone()]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();
    let restored = restore_quarantined(&entries[0], w.quarantine_root()).unwrap();

    remove_restored(&w.undo_log, std::slice::from_ref(&restored)).unwrap();

    let remaining = list_undo(&w.undo_log).unwrap();
    assert!(
        remaining.is_empty(),
        "restored entry must be removed from pending log"
    );
}

#[test]
fn remove_restored_keeps_unrelated_entries() {
    let w = Workspace::new();
    // A deletable + a restorable record.
    execute(
        &plan(Action::Delete, vec![w.file("a.dat")]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();
    let q_entries = execute(
        &plan(Action::Quarantine, vec![w.file("b.dat")]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();
    let restored = restore_quarantined(&q_entries[0], w.quarantine_root()).unwrap();

    remove_restored(&w.undo_log, std::slice::from_ref(&restored)).unwrap();

    let remaining = list_undo(&w.undo_log).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining[0].action,
        Action::Delete,
        "non-quarantine entry survives"
    );
}

// ============================================================================
// red lines — restore safety
// ============================================================================

/// RED LINE: restore must refuse a destination outside the quarantine root.
#[test]
fn restore_refuses_destination_outside_quarantine_root() {
    let w = Workspace::new();
    let outside = w.tmp.path().join("outside").join("evil.dat");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, "x").unwrap();

    let bogus = diskpilot_executor::UndoEntry {
        timestamp: "now".into(),
        action: Action::Quarantine,
        source: w.src.join("victim.dat"),
        destination: Some(outside.clone()),
        reason: "test".into(),
        bytes_freed: None,
    };

    let err = restore_quarantined(&bogus, w.quarantine_root()).unwrap_err();
    assert!(err.to_string().contains("outside quarantine"), "got: {err}");
}

/// RED LINE: restore must refuse when the original path already exists
/// (would silently overwrite user data).
#[test]
fn restore_refuses_when_source_already_exists() {
    let w = Workspace::new();
    let f = w.file("data.dat");
    let entries = execute(
        &plan(Action::Quarantine, vec![f.clone()]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();
    // Re-create the source path before restoring.
    std::fs::write(&f, "new content").unwrap();

    let err = restore_quarantined(&entries[0], w.quarantine_root()).unwrap_err();
    assert!(err.to_string().contains("already exists"), "got: {err}");
}

/// RED LINE: restore must refuse non-quarantine records.
#[test]
fn restore_refuses_non_quarantine_entry() {
    let w = Workspace::new();
    let bogus = diskpilot_executor::UndoEntry {
        timestamp: "now".into(),
        action: Action::Delete,
        source: w.src.join("gone.dat"),
        destination: None,
        reason: "test".into(),
        bytes_freed: None,
    };
    assert!(restore_quarantined(&bogus, w.quarantine_root()).is_err());
}

#[test]
fn restore_refuses_missing_quarantined_item() {
    let w = Workspace::new();
    let bogus = diskpilot_executor::UndoEntry {
        timestamp: "now".into(),
        action: Action::Quarantine,
        source: w.src.join("victim.dat"),
        destination: Some(w.quarantine.join("never-moved.dat")),
        reason: "test".into(),
        bytes_freed: None,
    };
    let err = restore_quarantined(&bogus, w.quarantine_root()).unwrap_err();
    assert!(err.to_string().contains("no longer exists"), "got: {err}");
}

// Small helper to keep tests terse.
impl Workspace {
    fn quarantine_root(&self) -> &Path {
        &self.quarantine
    }
}

/// 回归：盘根判定只拦盘根本身（C:\ 或 C:），不能把所有 Windows 嵌套
/// 绝对路径一起误杀——早期实现首段是 C: 就报 drive root，导致真机上
/// executor 所有操作被拒。
#[test]
fn protected_path_blocks_drive_roots_but_not_nested_paths() {
    use diskpilot_executor::protected_path;
    // 红线仍然拦得住：盘根、系统目录、用户主目录根
    assert!(protected_path(Path::new(r"C:\")).is_some());
    assert!(protected_path(Path::new(r"C:")).is_some());
    assert!(protected_path(Path::new(r"\\?\C:\")).is_some());
    assert!(protected_path(Path::new(r"C:\Windows")).is_some());
    assert!(protected_path(Path::new(r"C:\Users\alice")).is_some());
    // 普通嵌套绝对路径必须放行——Temp/工程目录都是这种形状
    assert!(protected_path(Path::new(
        r"C:\Users\alice\AppData\Local\Temp\.tmpABC\src\a.dat"
    ))
    .is_none());
    assert!(protected_path(Path::new(r"D:\projects\repo\target")).is_none());
    // 折叠 .. 后落在主目录根上的绕过依旧被拦
    assert!(protected_path(Path::new(r"C:\Users\alice\Documents\..")).is_some());
    // 折叠后只是 C:\Users 本身（非任何人的 home），不属红线
    assert!(protected_path(Path::new(r"C:\Users\alice\AppData\..\..")).is_none());
}

// ============================================================================
// 安全加固：canonicalize 堵 symlink/junction 指向受保护目录
// ============================================================================

/// RED LINE: 指向 Windows 系统目录的符号链接必须被 protected_path 拦下
/// （canonicalize 解析出真实目标后按目标判定）。Windows 上 junction 用
/// `cmd /c mklink /J` 创建（无需管理员），符号链接需要开发者模式。
#[test]
#[cfg_attr(not(any(unix, windows)), ignore)]
fn protected_path_blocks_symlink_pointing_into_windows() {
    use diskpilot_executor::protected_path;

    let tmp = tempfile::tempdir().expect("tempdir");
    let link_dir = tmp.path().join("link-to-system");

    #[cfg(windows)]
    let made = {
        // junction：`mklink /J <link> <target>`，target 用一个真实存在的
        // 目录。Windows 系统目录（C:\Windows）在测试机必然存在，但测试要
        // 可移植——用 tempdir 内建一个「系统目录形状」的目标再链接它。
        let target = tmp.path().join("Windows");
        std::fs::create_dir_all(&target).unwrap();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link_dir)
            .arg(&target)
            .status();
        status.map(|s| s.success()).unwrap_or(false)
    };
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(tmp.path().join("Windows"), &link_dir)
        .map(|_| true)
        .unwrap_or(false);
    #[cfg(not(any(unix, windows)))]
    let made = false;

    if !made {
        // 无权限/平台不支持创建链接时跳过——CI 与无管理员环境不因此红。
        eprintln!("skip: cannot create symlink on this platform");
        return;
    }

    // canonicalize 会把 link-to-system 解析到 target（tempdir 内的 "Windows"），
    // 首段是 temp 临时目录名而非盘符后的 "windows"……但 protected_path 对
    // tempdir 路径判定是「用户目录嵌套」，不属红线——这个测试要测的是
    // **链接解析后命中黑名单**，所以 target 得直接指向一个首段为 Windows 的
    // 真实系统目录。非 Windows 机器无此目录，测试在 unix 上改指 /etc。
    #[cfg(windows)]
    {
        // 指向真实系统目录的 junction（C:\Windows 恒存在）
        let sys_link = tmp.path().join("evil-link");
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&sys_link)
            .arg(r"C:\Windows")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            assert!(
                protected_path(&sys_link).is_some(),
                "junction -> C:\\Windows must be blocked, got: {}",
                sys_link.display()
            );
        } else {
            eprintln!("skip: cannot create junction to C:\\Windows");
        }
    }
    #[cfg(unix)]
    {
        // /etc 是系统目录；symlink 指向它必须被拦
        let sys_link = tmp.path().join("evil-link");
        if std::os::unix::fs::symlink("/etc", &sys_link).is_ok() {
            assert!(
                protected_path(&sys_link).is_some(),
                "symlink -> /etc must be blocked, got: {}",
                sys_link.display()
            );
        } else {
            eprintln!("skip: cannot create symlink to /etc");
        }
    }
}

/// RED LINE: junction/symlink 指向的**实际目标**在 execute Delete 时也会被
/// TOCTOU 复检拦下——即便路径被替换成链接也拒绝执行。
#[test]
#[cfg(windows)]
fn execute_delete_refuses_junction_pointing_at_windows() {
    let w = Workspace::new();
    let evil = w.src.join("evil-junction");
    let ok = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&evil)
        .arg(r"C:\Windows")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("skip: cannot create junction to C:\\Windows");
        return;
    }

    // 即便绕过 execute() 入口的批量校验（这里直接构造 plan 传入），
    // Delete 分支的 TOCTOU 复检也会在删除前再拦一次。两种拦截都算安全：
    // 入口校验（refusing to act on …）或 TOCTOU 复检（refusing to delete …）。
    let err = execute(
        &plan(Action::Delete, vec![evil]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("refusing to delete") || msg.contains("refusing to act on"),
        "junction -> C:\\Windows must be refused by execute, got: {msg}"
    );
}
