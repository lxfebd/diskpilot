//! scan.rs 单元测试（从 scan.rs 拆分，2026-10-12）。
//! find_subtree 命中/大小写/缺失 / tag_and_truncate 截断标记与原始计数。

use super::*;

fn node(name: &str, path: &str, is_dir: bool, children: Vec<Node>) -> Node {
    Node {
        name: name.into(),
        path: path.into(),
        is_dir,
        size: 0,
        file_count: 0,
        children,
        scaffold_id: None,
        top_extensions: Vec::new(),
        children_truncated: None,
        mtime: None,
    }
}

fn sample_tree() -> Node {
    node(
        "C:",
        "C:\\",
        true,
        vec![
            node(
                "Users",
                "C:\\Users",
                true,
                vec![node(
                    "alice",
                    "C:\\Users\\alice",
                    true,
                    vec![
                        node("doc.txt", "C:\\Users\\alice\\doc.txt", false, vec![]),
                        node(
                            "AppData",
                            "C:\\Users\\alice\\AppData",
                            true,
                            vec![node(
                                "Local",
                                "C:\\Users\\alice\\AppData\\Local",
                                true,
                                vec![],
                            )],
                        ),
                    ],
                )],
            ),
            node("Windows", "C:\\Windows", true, vec![]),
        ],
    )
}

#[test]
fn find_subtree_returns_full_subtree_for_existing_path() {
    let tree = sample_tree();
    let hit = find_subtree(&tree, "c:\\users\\alice").expect("应命中 alice");
    assert_eq!(hit.name, "alice");
    assert_eq!(hit.path, "C:\\Users\\alice");
    // 完整子树：AppData 也返回，不受截断影响
    assert_eq!(hit.children.len(), 2, "alice 下 doc.txt + AppData");
    let appdata = find_subtree(&tree, "C:/Users/alice/AppData").expect("斜杠归一化应命中");
    assert_eq!(appdata.children[0].name, "Local");
}

#[test]
fn find_subtree_is_case_insensitive_and_matches_root() {
    let tree = sample_tree();
    let hit = find_subtree(&tree, "C:\\WINDOWS").expect("大小写不敏感");
    assert_eq!(hit.name, "Windows");
    let root = find_subtree(&tree, "c:\\").expect("根命中");
    assert_eq!(root.path, "C:\\");
}

#[test]
fn find_subtree_returns_none_for_missing_path() {
    let tree = sample_tree();
    assert!(find_subtree(&tree, "C:\\nope").is_none());
    assert!(find_subtree(&tree, "C:\\Users\\bob").is_none());
}

#[test]
fn tag_and_truncate_marks_children_truncated_and_stores_original_count() {
    // 构造 130 个孩子：深度 0 的 cap 是 100，必然触发截断并记录原始数。
    let mut root = node(
        "C:",
        "C:\\",
        true,
        (0..130)
            .map(|i| node(&format!("d{i}"), &format!("C:\\d{i}"), true, vec![]))
            .collect(),
    );
    tag_and_truncate(&mut root, &[], 0);
    assert_eq!(root.children.len(), 100, "深度 0 应截到 100");
    assert_eq!(root.children_truncated, Some(130), "应记录截断前原始数 130");
    // 未截断的目录保持 None
    let mut small = node("small", "C:\\small", true, vec![]);
    tag_and_truncate(&mut small, &[], 0);
    assert_eq!(small.children_truncated, None);
}
