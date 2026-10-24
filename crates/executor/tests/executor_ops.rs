//! Integration tests for diskpilot-executor.
//!
//! Covers the four execution modes (dry-run / recycle / quarantine / delete),
//! the undo log lifecycle (list_undo / restore_quarantined / remove_restored),
//! and the safety red lines (no out-of-quarantine restore, no restore onto an
//! existing source, no crash on missing paths).

use diskpilot_executor::{
    execute, list_undo, remove_restored, restore_quarantined, trim_log, Action, Plan, UNDO_LOG_MAX,
};
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
fn dry_run_writes_no_log_and_touches_nothing() {
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
    // dry-run 是只读「预览/确认窗」语义：绝不写 undo.jsonl，否则「最近清理」
    // 台账会堆积无法撤销的幽灵条目（destination=None），反复预览会无限膨胀。
    assert_eq!(list_undo(&w.undo_log).unwrap().len(), 0);
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

// ============================================================================
// bytes_freed 语义（前端「已清理 X」真实展示的数据源，2026-10-13 补测）
// ============================================================================

/// dry-run 也实测每项真实字节（与真执行同一口径）：确认面板的「预估」必须
/// 与实际可清量对齐，不再恒 None。文件取 len，目录递归求和。
#[test]
fn dry_run_measures_real_bytes_for_file_and_dir() {
    let w = Workspace::new();
    let f = w.file("a.dat"); // "hello" = 5 bytes
    let d = w.subdir("d"); // inner.txt = 1 byte

    let entries = execute(
        &plan(Action::Quarantine, vec![f, d]),
        true,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].bytes_freed, Some(5), "file must report real len");
    assert_eq!(
        entries[1].bytes_freed,
        Some(1),
        "dir must report recursive sum"
    );
}

/// 真执行（quarantine）的字节统计必须在移动**之前**完成：move 成功后原路径
/// 已消失，晚算必为 0。台账（list_undo）里也必须持久化同一数值。
#[test]
fn quarantine_records_actual_bytes_freed_before_move() {
    let w = Workspace::new();
    let f = w.file("big.dat"); // 5 bytes

    let entries = execute(
        &plan(Action::Quarantine, vec![f]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    assert_eq!(entries[0].bytes_freed, Some(5));
    let logged = list_undo(&w.undo_log).unwrap();
    assert_eq!(logged.len(), 1);
    assert_eq!(
        logged[0].bytes_freed,
        Some(5),
        "undo log must persist the measured bytes"
    );
}

/// 目录删除的字节 = 递归求和（含嵌套子目录），且必须在删除前统计。
#[test]
fn delete_records_recursive_bytes_for_directory() {
    let w = Workspace::new();
    let d = w.src.join("tree");
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(d.join("one.txt"), "1234").unwrap(); // 4
    std::fs::write(d.join("sub").join("two.txt"), "56").unwrap(); // 2

    let entries = execute(
        &plan(Action::Delete, vec![d]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    assert_eq!(
        entries[0].bytes_freed,
        Some(6),
        "dir delete must report recursive sum (4 + 2)"
    );
}

/// 不存在的路径：动作照常记录（不 panic），但字节必须是 None——
/// 前端据此降级为「预估」展示，而不是谎报 0 字节已释放。
#[test]
fn missing_path_records_none_bytes_freed() {
    let w = Workspace::new();
    let ghost = w.src.join("ghost.dat");

    let entries = execute(
        &plan(Action::Delete, vec![ghost]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].bytes_freed, None);
}

// ============================================================================
// 台账保留上限（R388）
// ============================================================================

/// 预置一份「已含 N 条 delete 记录」的台账（只写台账行，不真的动文件）。
fn seed_log(path: &Path, n: usize) {
    let mut body = String::new();
    for i in 0..n {
        body.push_str(&format!(
            r#"{{"timestamp":"2026-01-01T00:00:00Z","action":"delete","source":"C:/seed/{i}.dat","reason":"seed"}}"#
        ));
        body.push('\n');
    }
    std::fs::write(path, body).unwrap();
}

#[test]
fn trim_log_keeps_newest_entries() {
    let w = Workspace::new();
    for name in ["one.dat", "two.dat", "three.dat", "four.dat", "five.dat"] {
        execute(
            &plan(Action::Delete, vec![w.file(name)]),
            false,
            &w.undo_log,
            w.quarantine_root(),
        )
        .unwrap();
    }

    let pruned = trim_log(&w.undo_log, 3).unwrap();
    assert_eq!(pruned, 2, "5 条裁到 3 条应裁掉 2 条");

    let kept = list_undo(&w.undo_log).unwrap();
    assert_eq!(kept.len(), 3);
    assert_eq!(kept[0].source, w.src.join("five.dat"), "最新的留下且在最前");
    assert_eq!(kept[2].source, w.src.join("three.dat"), "最旧的两条被裁掉");
    assert!(
        kept.iter().all(|e| e.source != w.src.join("one.dat")),
        "第一条应已被裁掉"
    );
}

#[test]
fn trim_log_is_noop_under_the_limit() {
    let w = Workspace::new();
    execute(
        &plan(Action::Delete, vec![w.file("a.dat")]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();
    execute(
        &plan(Action::Delete, vec![w.file("b.dat")]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();
    let before = std::fs::read_to_string(&w.undo_log).unwrap();

    assert_eq!(trim_log(&w.undo_log, 5).unwrap(), 0, "未超上限不裁剪");
    assert_eq!(
        std::fs::read_to_string(&w.undo_log).unwrap(),
        before,
        "未超上限时文件保持原样（不重写、不丢坏行）"
    );
    // 台账不存在 / max=0 都是安全空操作。
    assert_eq!(trim_log(&w.tmp.path().join("nope.jsonl"), 3).unwrap(), 0);
    assert_eq!(trim_log(&w.undo_log, 0).unwrap(), 0);
}

#[test]
fn trim_log_drops_unparsable_lines_when_it_rewrites() {
    let w = Workspace::new();
    let mut body = String::new();
    for i in 0..4 {
        body.push_str(&format!(
            r#"{{"timestamp":"2026-01-01T00:00:00Z","action":"delete","source":"C:/g/{i}.dat","reason":"g"}}"#
        ));
        body.push('\n');
        if i == 1 {
            body.push_str("this line is not json");
            body.push('\n');
        }
    }
    std::fs::write(&w.undo_log, body).unwrap();

    let pruned = trim_log(&w.undo_log, 2).unwrap();
    assert_eq!(pruned, 2, "4 条有效记录裁到 2 条");
    let after = std::fs::read_to_string(&w.undo_log).unwrap();
    assert!(!after.contains("not json"), "重写时坏行一并丢弃");
    assert_eq!(list_undo(&w.undo_log).unwrap().len(), 2);
}

// clippy 的 cmp_owned 建议对 PathBuf 比较 &str 不成立（PathBuf 没有这类 PartialEq），
// 这里按语义显式构造一次桩路径再做全量比对，属刻意为之。
#[test]
#[allow(clippy::cmp_owned)]
fn execute_trims_oversized_ledger_to_the_cap() {
    let w = Workspace::new();
    seed_log(&w.undo_log, UNDO_LOG_MAX + 2);

    execute(
        &plan(Action::Delete, vec![w.file("fresh.dat")]),
        false,
        &w.undo_log,
        w.quarantine_root(),
    )
    .unwrap();

    let entries = list_undo(&w.undo_log).unwrap();
    assert_eq!(entries.len(), UNDO_LOG_MAX, "执行后台账收敛到保留上限");
    assert_eq!(
        entries[0].source,
        w.src.join("fresh.dat"),
        "本次执行的记录是最新的，必须留住"
    );
    assert!(
        entries
            .iter()
            .all(|e| e.source != PathBuf::from("C:/seed/0.dat")),
        "最早的那条被裁掉"
    );
}
