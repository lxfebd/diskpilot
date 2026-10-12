//! usn.rs 单元测试（从 usn.rs 拆分，2026-10-12）。
//! USN 记录解析 / 路径解析 / 缓存树 merge / 游标有效性 / 重命名对消。

use super::*;
use crate::usn_tree::remove_node;
use std::path::PathBuf;

/// Build a synthetic USN_RECORD_V2 byte buffer for `parse_usn_records`.
/// Layout (Win32 ABI): RecordLength, Major=2, Minor=0, FileRef(8),
/// ParentRef(8), Usn(8), TimeStamp(8), Reason(4), SourceInfo(4),
/// SecurityId(4), FileAttributes(4), FileNameLength(2), FileNameOffset(2),
/// then UTF-16 name.
fn build_record(frn: u64, parent: u64, usn: i64, reason: u32, is_dir: bool, name: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    let name_u16: Vec<u16> = name.encode_utf16().collect();
    let name_bytes = name_u16.len() * 2;
    let total = 60 + name_bytes;
    let mut rec = vec![0u8; total];
    rec[0..4].copy_from_slice(&(total as u32).to_le_bytes());
    rec[4..6].copy_from_slice(&2u16.to_le_bytes()); // major version 2
    rec[6..8].copy_from_slice(&0u16.to_le_bytes());
    rec[8..16].copy_from_slice(&frn.to_le_bytes());
    rec[16..24].copy_from_slice(&parent.to_le_bytes());
    rec[24..32].copy_from_slice(&usn.to_le_bytes());
    rec[32..40].copy_from_slice(&0u64.to_le_bytes()); // timestamp
    rec[40..44].copy_from_slice(&reason.to_le_bytes());
    rec[44..48].copy_from_slice(&0u32.to_le_bytes()); // source info
    rec[48..52].copy_from_slice(&0u32.to_le_bytes()); // security id
    rec[52..56].copy_from_slice(&(if is_dir { 0x10u32 } else { 0u32 }).to_le_bytes());
    rec[56..58].copy_from_slice(&(name_bytes as u16).to_le_bytes());
    rec[58..60].copy_from_slice(&60u16.to_le_bytes());
    for (i, u) in name_u16.iter().enumerate() {
        rec[60 + i * 2..62 + i * 2].copy_from_slice(&u.to_le_bytes());
    }
    buf.extend_from_slice(&rec);
    buf
}

fn dummy_change(frn: u64, parent: u64, name: &str, reason: u32, is_dir: bool) -> UsnChange {
    UsnChange {
        frn,
        parent_frn: parent,
        name: name.to_string(),
        is_dir,
        reason,
        usn: frn as i64,
    }
}

#[test]
fn parse_usn_records_roundtrip() {
    let mut buf = build_record(100, 50, 7, 0x100 | 1, false, "report.pdf");
    buf.extend_from_slice(&build_record(200, 100, 8, 0x200, true, "sub"));
    let recs = parse_usn_records(&buf);
    assert_eq!(recs.len(), 2, "两个记录都应解析出");
    assert_eq!(recs[0].frn, 100);
    assert_eq!(recs[0].parent_frn, 50);
    assert_eq!(recs[0].usn, 7);
    assert_eq!(recs[0].reason, 0x100 | 1);
    assert!(!recs[0].is_dir);
    assert_eq!(recs[0].name, "report.pdf");
    assert_eq!(recs[1].frn, 200);
    assert!(recs[1].is_dir);
    assert_eq!(recs[1].name, "sub");
}

#[test]
fn parse_usn_records_stops_on_zero() {
    // A zero-length record terminates the batch.
    let mut buf = build_record(1, 2, 1, 0x100, false, "a.txt");
    buf.extend_from_slice(&[0u8; 8]);
    let recs = parse_usn_records(&buf);
    assert_eq!(recs.len(), 1);
}

#[test]
fn resolve_paths_walks_parent_chain() {
    let mut table = std::collections::HashMap::new();
    // FRN → (parent, name)
    table.insert(10, (5, "Users".to_string()));
    table.insert(20, (10, "alice".to_string()));
    table.insert(30, (20, "Documents".to_string()));
    table.insert(40, (30, "report.txt".to_string()));
    let root = PathBuf::from("C:\\");
    let ch = dummy_change(40, 30, "report.txt", 1, false);
    let resolved = resolve_paths(&[ch], &table, &root);
    assert_eq!(resolved.len(), 1);
    let p = &resolved[0].path;
    assert_eq!(p, &PathBuf::from("C:\\Users\\alice\\Documents\\report.txt"));
}

#[test]
fn merge_changes_updates_tree_aggregates() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    let mut tree = crate::Node {
        name: root.file_name().unwrap().to_string_lossy().to_string(),
        path: root.to_string_lossy().to_string(),
        is_dir: true,
        size: 0,
        file_count: 0,
        children: vec![],
        scaffold_id: None,
        top_extensions: vec![],
        children_truncated: None,
        mtime: None,
    };
    // A create + data change under root/dir.
    let d = root.join("merge_updates_dir");
    let _ = std::fs::create_dir_all(&d);
    let f = d.join("new.bin");
    std::fs::write(&f, vec![b'x'; 100]).unwrap();
    let resolved = vec![
        ResolvedChange {
            path: d.clone(),
            is_dir: true,
            reason: USN_REASON_FILE_CREATE,
            frn: 1,
        },
        ResolvedChange {
            path: f.clone(),
            is_dir: false,
            reason: USN_REASON_FILE_CREATE,
            frn: 2,
        },
    ];
    let applied = merge_changes(&mut tree, &resolved, &root, None);
    assert_eq!(applied, 2);
    assert_eq!(tree.file_count, 1);
    assert_eq!(tree.size, 100);
    let dir = tree
        .children
        .iter()
        .find(|c| c.name == "merge_updates_dir")
        .unwrap();
    assert!(dir.is_dir);
    assert_eq!(dir.size, 100);
    assert_eq!(dir.file_count, 1);
    // Delete the file — the tree must drop the node and roll back the size.
    let del = ResolvedChange {
        path: f.clone(),
        is_dir: false,
        reason: USN_REASON_FILE_DELETE,
        frn: 2,
    };
    let applied = merge_changes(&mut tree, &[del], &root, None);
    assert_eq!(applied, 1);
    assert_eq!(tree.size, 0);
    assert_eq!(tree.file_count, 0);
    assert_eq!(tree.children[0].children.len(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cursor_still_valid_detects_reset_and_rollover() {
    let state = JournalState {
        journal_id: 42,
        first_usn: 100,
        next_usn: 500,
        lowest_valid_usn: 100,
    };
    // Same journal, cursor inside window → valid.
    let ok = UsnCursor {
        journal_id: 42,
        next_usn: 300,
    };
    assert!(state.cursor_still_valid(&ok));
    // Journal id changed → reset.
    let reset = UsnCursor {
        journal_id: 43,
        next_usn: 300,
    };
    assert!(!state.cursor_still_valid(&reset));
    // Journal rolled over (cursor below lowest_valid) → invalid.
    let rolled = UsnCursor {
        journal_id: 42,
        next_usn: 50,
    };
    assert!(!state.cursor_still_valid(&rolled));
}

#[test]
fn is_size_relevant_filters_noise() {
    // Data change + create → relevant.
    assert!(dummy_change(1, 2, "f", 1, false).is_size_relevant());
    // Metadata change (basic info / security) → not relevant.
    assert!(!dummy_change(1, 2, "f", 0x8000, false).is_size_relevant());
    // Close-only → not relevant.
    assert!(!dummy_change(1, 2, "f", 0x8000_0000, false).is_size_relevant());
}

#[test]
fn remove_dir_rolls_back_recursive_file_count() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    let mut tree = crate::Node {
        name: root.file_name().unwrap().to_string_lossy().to_string(),
        path: root.to_string_lossy().to_string(),
        is_dir: true,
        size: 0,
        file_count: 0,
        children: vec![],
        scaffold_id: None,
        top_extensions: vec![],
        children_truncated: None,
        mtime: None,
    };
    let d = root.join("rm_dir_recursive");
    let _ = std::fs::create_dir_all(&d);
    let f1 = d.join("a.bin");
    let f2 = d.join("b.bin");
    std::fs::write(&f1, vec![b'x'; 10]).unwrap();
    std::fs::write(&f2, vec![b'x'; 20]).unwrap();
    let resolved = vec![
        ResolvedChange {
            path: d.clone(),
            is_dir: true,
            reason: USN_REASON_FILE_CREATE,
            frn: 1,
        },
        ResolvedChange {
            path: f1.clone(),
            is_dir: false,
            reason: USN_REASON_FILE_CREATE,
            frn: 2,
        },
        ResolvedChange {
            path: f2.clone(),
            is_dir: false,
            reason: USN_REASON_FILE_CREATE,
            frn: 3,
        },
    ];
    merge_changes(&mut tree, &resolved, &root, None);
    assert_eq!(tree.file_count, 2);
    assert_eq!(tree.size, 30);

    // 删除目录：file_count 是递归计数，dc 必须减子树自身计数（2），
    // 不是旧实现的 0（那样父目录 file_count 永久虚高）。
    // 目录删除时路径通常已消失，size_hint=None；remove_node 的目录分支
    // 靠缓存节点自身的 size/count 回滚。
    let applied = remove_node(&mut tree, d.strip_prefix(&root).unwrap(), None);
    assert!(applied);
    assert_eq!(tree.file_count, 0, "目录删除应回滚递归计数");
    assert_eq!(tree.size, 0);
    assert_eq!(tree.children.len(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn remove_below_top_k_still_rolls_back_aggregate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    let mut tree = crate::Node {
        name: root.file_name().unwrap().to_string_lossy().to_string(),
        path: root.to_string_lossy().to_string(),
        is_dir: true,
        size: 0,
        file_count: 0,
        children: vec![],
        scaffold_id: None,
        top_extensions: vec![],
        children_truncated: None,
        mtime: None,
    };
    let d = root.join("rm_topk_dir");
    let _ = std::fs::create_dir_all(&d);
    let big = d.join("big.bin");
    let small = d.join("small.bin");
    std::fs::write(&big, vec![b'x'; 100]).unwrap();
    std::fs::write(&small, vec![b'x'; 5]).unwrap();
    // keep_files=1 → 只有 big.bin 物化为 child，small.bin 未物化但聚合计入。
    merge_changes(
        &mut tree,
        &[
            ResolvedChange {
                path: d.clone(),
                is_dir: true,
                reason: USN_REASON_FILE_CREATE,
                frn: 1,
            },
            ResolvedChange {
                path: big.clone(),
                is_dir: false,
                reason: USN_REASON_FILE_CREATE,
                frn: 2,
            },
            ResolvedChange {
                path: small.clone(),
                is_dir: false,
                reason: USN_REASON_FILE_CREATE,
                frn: 3,
            },
        ],
        &root,
        Some(1),
    );
    assert_eq!(tree.size, 105);
    assert_eq!(tree.file_count, 2);

    // 删除未物化的小文件：stat 给出 size hint（5），聚合必须回滚到 100/1。
    let del = ResolvedChange {
        path: small.clone(),
        is_dir: false,
        reason: USN_REASON_FILE_DELETE,
        frn: 3,
    };
    let applied = merge_changes(&mut tree, &[del], &root, Some(1));
    assert_eq!(applied, 1);
    assert_eq!(tree.size, 100, "below top-K 删除也应回滚聚合 size");
    assert_eq!(tree.file_count, 1, "below top-K 删除也应回滚 file_count");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn rename_old_removes_ghost_node() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    let mut tree = crate::Node {
        name: root.file_name().unwrap().to_string_lossy().to_string(),
        path: root.to_string_lossy().to_string(),
        is_dir: true,
        size: 0,
        file_count: 0,
        children: vec![],
        scaffold_id: None,
        top_extensions: vec![],
        children_truncated: None,
        mtime: None,
    };
    let d = root.join("rename_ghost_dir");
    let _ = std::fs::create_dir_all(&d);
    let f = d.join("old.bin");
    std::fs::write(&f, vec![b'x'; 50]).unwrap();
    merge_changes(
        &mut tree,
        &[
            ResolvedChange {
                path: d.clone(),
                is_dir: true,
                reason: USN_REASON_FILE_CREATE,
                frn: 1,
            },
            ResolvedChange {
                path: f.clone(),
                is_dir: false,
                reason: USN_REASON_FILE_CREATE,
                frn: 2,
            },
        ],
        &root,
        None,
    );
    assert_eq!(tree.size, 50);
    assert_eq!(tree.file_count, 1);

    // 重命名（RENAME_OLD_NAME）：旧路径已不存在，旧节点应被摘除，聚合回滚。
    std::fs::rename(&f, d.join("new.bin")).unwrap();
    let ren = ResolvedChange {
        path: f.clone(),
        is_dir: false,
        reason: USN_REASON_RENAME_OLD_NAME,
        frn: 2,
    };
    let applied = merge_changes(&mut tree, &[ren], &root, None);
    assert_eq!(applied, 1);
    assert_eq!(tree.size, 0, "重命名旧路径应摘掉 ghost 节点");
    assert_eq!(tree.file_count, 0);
    assert_eq!(tree.children[0].children.len(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn rename_old_pair_moves_subtree() {
    // 目录整体改名（sub → new_dir）：USN 对同一 FRN 成对吐出 RENAME_OLD_NAME
    // + RENAME_NEW_NAME。旧实现只摘旧空壳，子树不会掉到新路径 → 新路径只剩
    // size=0 空目录。这里验证配对搬移：子树整体搬家、聚合沿祖先链滚动净零。
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    let mut tree = crate::Node {
        name: root.file_name().unwrap().to_string_lossy().to_string(),
        path: root.to_string_lossy().to_string(),
        is_dir: true,
        size: 0,
        file_count: 0,
        children: vec![],
        scaffold_id: None,
        top_extensions: vec![],
        children_truncated: None,
        mtime: None,
    };
    let d = root.join("rename_move_dir");
    // 先清残留再建沙箱：失败运行会留下 new_dir 等残骸，下一次 rename 直接
    // DirectoryNotEmpty（并行跑多个测试时残留更容易发生）。
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::create_dir_all(&d);
    let sub = d.join("sub");
    let _ = std::fs::create_dir_all(&sub);
    let f = sub.join("old.bin");
    std::fs::write(&f, vec![b'x'; 50]).unwrap();
    // 两条 create 事件建树：dir + old.bin（apply_upsert 途经 parent 链时
    // 会自动补建 sub 目录节点）。
    merge_changes(
        &mut tree,
        &[
            ResolvedChange {
                path: d.clone(),
                is_dir: true,
                reason: USN_REASON_FILE_CREATE,
                frn: 10,
            },
            ResolvedChange {
                path: f.clone(),
                is_dir: false,
                reason: USN_REASON_FILE_CREATE,
                frn: 20,
            },
        ],
        &root,
        None,
    );
    assert_eq!(tree.size, 50);
    assert_eq!(tree.file_count, 1);
    assert_eq!(
        tree.children[0].children[0].name, "sub",
        "树应已建好 sub 目录节点"
    );

    // 在真实 fs 上把整个 sub 目录搬走（USN 事件无法凭空产生，只能手工
    // 构造 ResolvedChange；new 路径需要真实存在，否则配对后 stat 失败）。
    let new_dir = d.join("new_dir");
    std::fs::rename(&sub, &new_dir).unwrap();
    let resolved = vec![
        ResolvedChange {
            path: sub.clone(),
            is_dir: true,
            reason: USN_REASON_RENAME_OLD_NAME,
            frn: 1,
        },
        ResolvedChange {
            path: new_dir.clone(),
            is_dir: true,
            reason: USN_REASON_RENAME_NEW_NAME,
            frn: 1,
        },
    ];
    let applied = merge_changes(&mut tree, &resolved, &root, None);
    assert_eq!(applied, 2);

    // 新路径下子树完整：new_dir/old.bin 存在且 size=50。
    let dir = tree
        .children
        .iter()
        .find(|c| c.name == "rename_move_dir")
        .unwrap();
    assert_eq!(dir.children.len(), 1, "旧 sub 摘走后 dir 下只剩 new_dir");
    let new_dir_node = dir.children.iter().find(|c| c.name == "new_dir").unwrap();
    assert!(new_dir_node.is_dir);
    assert_eq!(new_dir_node.size, 50);
    assert_eq!(new_dir_node.file_count, 1);
    let bin = new_dir_node
        .children
        .iter()
        .find(|c| c.name == "old.bin")
        .unwrap();
    assert_eq!(bin.size, 50, "搬移后子树内容必须完整（不能只剩空目录壳）");
    assert_eq!(bin.path, new_dir.join("old.bin").to_string_lossy());

    // 旧路径下 sub 节点不存在。
    assert!(
        dir.children.iter().all(|c| c.name != "sub"),
        "旧路径 sub 节点应已消失"
    );
    // 根聚合不变：搬移是净零操作。
    assert_eq!(tree.size, 50, "搬移后根 size 不变");
    assert_eq!(tree.file_count, 1, "搬移后根 file_count 不变");
    let _ = std::fs::remove_dir_all(&d);
}
