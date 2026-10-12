//! 扫描后置处理域：scaffold 标记 + 深度广度截断。
//! 从 scan.rs 拆分（2026-10-12）。纯内存树操作，无 IO。

use diskpilot_scaffold::{detect_compiled, CompiledScaffold};
use diskpilot_scanner::Node;
use std::path::Path;
/// Walk `node` in place, filling `scaffold_id` for directories and truncating
/// each level's children to the same depth-based caps the old frontend
/// `tagScaffolds` applied (depth<2 → 100, depth<4 → 50, else → 20).
///
/// Order matters: we recurse into ALL children first to propagate scaffold tags
/// up, then truncate. `detect_compiled` receives the child names captured
/// during the scan so `must_have_child` checks are in-memory (no disk exists).
pub(crate) fn tag_and_truncate(node: &mut Node, compiled: &[CompiledScaffold], depth: usize) {
    if node.is_dir {
        // 内存 children 名替代磁盘 exists：must_have_child 直接在子目录列表里查，
        // 消除对每盘数十万目录的 path.join(child).exists() 磁盘 IO（C 盘实测
        // tag 阶段 229s → 毫秒级）。
        let child_names: Vec<String> = node.children.iter().map(|c| c.name.clone()).collect();
        node.scaffold_id = detect_compiled(compiled, Path::new(&node.path), Some(&child_names));
    }
    for c in &mut node.children {
        tag_and_truncate(c, compiled, depth + 1);
    }
    let cap = if depth < 2 {
        100
    } else if depth < 4 {
        50
    } else {
        20
    };
    if node.children.len() > cap {
        // 记录截断前原始子项数：前端展开时据此判断「这层被砍过」，
        // 从后端内存树按需取回完整子项（按需加载子树）。
        node.children_truncated = Some(node.children.len() as u64);
        // Partition tagged subtrees first (preserving their original size-desc
        // order), then fill remaining slots from the rest. Cap the tagged group
        // at `cap` too, so a freak case with > cap matches at one level still
        // produces a bounded tree.
        let (tagged, rest): (Vec<Node>, Vec<Node>) =
            node.children.drain(..).partition(has_scaffold_tag);
        let mut survivors: Vec<Node> = tagged.into_iter().take(cap).collect();
        let need = cap.saturating_sub(survivors.len());
        survivors.extend(rest.into_iter().take(need));
        // Restore size-desc order for display (partition mixed tagged in front).
        survivors.sort_by_key(|c| std::cmp::Reverse(c.size));
        node.children = survivors;
    }
}

/// True if `n` itself, or any descendant, has a scaffold_id assigned. Used by
/// `tag_and_truncate` to decide which subtrees must survive truncation.
fn has_scaffold_tag(n: &Node) -> bool {
    n.scaffold_id.is_some() || n.children.iter().any(has_scaffold_tag)
}
