//! 清理执行领域：execute_scope / execute_ai_plan / undo。
//! 从 lib.rs 拆分（拆分时逐函数核对，逻辑不变）。
//!
//! 2026-10-12 再拆分：作用域执行 → `exec_scope`，glob 锚点+回收 → `exec_glob`，
//! AI 清理 → `exec_aiplan`，撤销列表+预检命令 → `exec_undo`，预检纯逻辑 → `exec_preflight`，
//! 撤销核心 → `exec_undo_core`。本文件保留确认门与单测。

#[path = "exec_aiplan.rs"]
pub(crate) mod exec_aiplan;
#[path = "exec_glob.rs"]
pub(crate) mod exec_glob;
#[path = "exec_preflight.rs"]
pub(crate) mod exec_preflight;
#[path = "exec_scope.rs"]
pub(crate) mod exec_scope;
#[path = "exec_undo.rs"]
pub(crate) mod exec_undo;
#[path = "exec_undo_core.rs"]
pub(crate) mod exec_undo_core;

pub(crate) use exec_glob::scope_glob_roots;

/// 人驱动写操作的确认门：`dry_run` 恒放行（预览只读，可以随便调），一旦落到
/// 磁盘就必须 `confirmed == Some(true)`。抽成函数是为了让这条铁律有测试兜底
/// （`execute_scope` / `recycle_paths` 共用；`toolbelt_recycle` 没有预览分支，
/// 单独写了同样的判断）。
///
/// 注意这里**不**校验 `cleanup.execute` 权限：那条 L2 权限是「允许 AI 自己
/// 执行清理」的钥匙（见 `execute_ai_plan`），而这几条命令是用户在看清清单后
/// 主动点的按钮。把人驱动的流程绑到默认关闭的 AI 权限上，普通用户就清不了东西。
pub(crate) fn confirm_gate(dry_run: bool, confirmed: Option<bool>) -> Result<(), &'static str> {
    if !dry_run && confirmed != Some(true) {
        return Err("必须由用户明确确认（confirmed=true）后才能真实落盘。");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::exec_undo_core::undo_core;
    use super::*;
    use diskpilot_executor::{execute, list_undo as list_undo_log, Plan};
    #[cfg(windows)]
    use diskpilot_scanner::Node;
    use std::path::PathBuf;

    /// 进程内临时目录（不同测试并发跑，各自创建独立子目录避免撞名）。
    fn tmp_sub(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("dp-executor-test-{}", std::process::id()));
        let d = base.join(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn confirm_gate_blocks_real_writes_without_confirmation() {
        // 预览（dry-run）是只读的，永远放行；真实落盘必须 confirmed == Some(true)。
        // `None` / `Some(false)` 都不算确认——绕过前端确认窗直接 invoke 要被拦下。
        assert!(confirm_gate(true, None).is_ok());
        assert!(confirm_gate(true, Some(false)).is_ok());
        assert!(confirm_gate(true, Some(true)).is_ok());
        assert!(confirm_gate(false, None).is_err());
        assert!(confirm_gate(false, Some(false)).is_err());
        assert!(confirm_gate(false, Some(true)).is_ok());
    }

    #[test]
    fn undo_core_restores_quarantined_entry() {
        let w = tmp_sub("undo-restore");
        let log = w.join("undo.jsonl");
        let qr = w.join("quarantine");
        let src = w.join("data.dat");
        std::fs::write(&src, "hello").unwrap();
        execute(
            &Plan {
                action: diskpilot_executor::Action::Quarantine,
                paths: vec![src.clone()],
                reason: "test".into(),
            },
            false,
            &log,
            &qr,
        )
        .unwrap();
        assert!(!src.exists(), "隔离后原路径应消失");
        let r = undo_core(&log, &qr, 0, src.to_string_lossy().into_owned());
        assert!(r.is_ok(), "同源同 index 应恢复成功: {r:?}");
        assert!(src.exists(), "恢复后原文件应回来");
        assert_eq!(std::fs::read_to_string(&src).unwrap(), "hello");
        // 已恢复的条目应从日志移除（remove_restored 原子重写）
        let left = list_undo_log(&log).unwrap();
        assert_eq!(left.len(), 0, "已恢复条目应清出日志: {left:?}");
    }

    #[test]
    fn undo_core_rejects_stale_source_drift() {
        let w = tmp_sub("undo-drift");
        let log = w.join("undo.jsonl");
        let qr = w.join("quarantine");
        let a = w.join("a.dat");
        let b = w.join("b.dat");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        execute(
            &Plan {
                action: diskpilot_executor::Action::Quarantine,
                paths: vec![a.clone(), b.clone()],
                reason: "test".into(),
            },
            false,
            &log,
            &qr,
        )
        .unwrap();
        // index 0 现在是 b.dat（最新在前），用户拿旧列表点 a.dat → 必须拒绝
        let r = undo_core(&log, &qr, 0, a.to_string_lossy().into_owned());
        let msg = match &r {
            Err(m) => m.clone(),
            Ok(_) => String::new(),
        };
        assert!(
            r.is_err() && msg.contains("撤销目标已变化"),
            "source 漂移必须拒绝，got: {r:?}"
        );
        assert!(!a.exists() && !b.exists(), "漂移拒绝时不能恢复任何文件");
    }

    #[test]
    fn glob_roots_static_prefix_before_wildcard() {
        // `.../.cargo/registry/**` 展开后锚到 registry 目录
        let reg = tmp_sub("anchor-registry").join(".cargo/registry");
        std::fs::create_dir_all(&reg).unwrap();
        let pat = format!("{}/**", reg.to_string_lossy().replace('\\', "/"));
        let roots = scope_glob_roots(&pat, &[]);
        assert_eq!(roots, vec![reg]);
    }

    #[test]
    fn glob_roots_alternation_keeps_all_branches() {
        // `**/AppData/Local/{npm-cache,pnpm-cache}/**`：两个分支在 detect 里
        // 都存在时，两个锚点都要返回——只取一个会漏掉另一个（npm 空、
        // pnpm 706MB 时「建议可清、执行 0b」脱节的真因）。
        let base = tmp_sub("anchor-brace");
        let npm = base.join("AppData/Local/npm-cache");
        let pnpm = base.join("AppData/Local/pnpm-cache");
        std::fs::create_dir_all(&npm).unwrap();
        std::fs::create_dir_all(&pnpm).unwrap();
        let pat = "**/AppData/Local/{npm-cache,pnpm-cache}/**";
        let detect = vec![
            npm.to_string_lossy().into_owned(),
            pnpm.to_string_lossy().into_owned(),
        ];
        let roots = scope_glob_roots(pat, &detect);
        assert!(roots.contains(&npm), "应含 npm 锚点: {roots:?}");
        assert!(roots.contains(&pnpm), "应含 pnpm 锚点: {roots:?}");
        assert_eq!(roots.len(), 2, "两个分支都要保留: {roots:?}");
    }

    #[test]
    fn glob_roots_alternation_no_dir_falls_back_to_detect() {
        // 分支目录都不存在 → 回退 detect 第一条存在的目录
        let base = tmp_sub("anchor-nodir");
        let fallback = base.join("somewhere/exists");
        std::fs::create_dir_all(&fallback).unwrap();
        let pat = "**/AppData/Local/{npm-cache,pnpm-cache}/**";
        let detect = vec![fallback.to_string_lossy().into_owned()];
        let roots = scope_glob_roots(pat, &detect);
        assert_eq!(roots, vec![fallback]);
    }

    #[test]
    fn glob_roots_bare_double_star_no_detect_returns_empty() {
        // 裸 `**` + detect 全不存在 → 空（调用方用 requested_root 兜底）
        assert!(scope_glob_roots("**/AppData/Local/npm-cache/**", &[]).is_empty());
        assert!(scope_glob_roots("**/pkgs/*", &[]).is_empty());
    }

    #[test]
    fn glob_roots_tail_detect_finds_existing_dir() {
        // `**/AppData/Local/npm-cache/**` → 锚段 AppData/Local/npm-cache，
        // detect 里带该后缀且存在者命中。
        let target = tmp_sub("tail-detect").join("AppData/Local/npm-cache");
        std::fs::create_dir_all(&target).unwrap();
        let pat = "**/AppData/Local/npm-cache/**";
        let detect = vec![
            target.to_string_lossy().into_owned(),
            "C:/nonexistent/pnpm-cache".to_string(),
        ];
        let roots = scope_glob_roots(pat, &detect);
        assert_eq!(roots, vec![target]);
    }

    /// 执行侧真实数据回归：`%TEMP%/**` 展开后是 Windows 反斜杠，修复前
    /// executor 直接用它建 globset，与 walk 内部统一正斜杠的匹配目标不匹配
    /// → 真实执行永远 0 命中（「建议可清、清理 0 文件」执行侧根因）。
    /// 修复后 expand_env(...).replace('\\', "/")，这里用真实 TEMP 文件验证。
    #[test]
    #[cfg(windows)]
    fn execute_pattern_normalized_matches_real_temp() {
        use diskpilot_scaffold::expand_env;
        use diskpilot_scanner::find_matching_files_on_tree;
        // 构造一颗含真实 TEMP 顶层文件的小树（模拟 scan_tree 缓存树）。
        let temp = std::env::var("TEMP").expect("TEMP 环境变量");
        let mut children: Vec<Node> = Vec::new();
        for entry in std::fs::read_dir(&temp).unwrap().flatten() {
            let p = entry.path();
            if p.is_file() && children.len() < 50 {
                children.push(Node {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: p.to_string_lossy().into_owned(),
                    is_dir: false,
                    size: p.metadata().map(|m| m.len()).unwrap_or(0),
                    file_count: 0,
                    children: vec![],
                    scaffold_id: None,
                    top_extensions: vec![],
                    children_truncated: None,
                    mtime: None,
                });
            }
        }
        if children.is_empty() {
            eprintln!("skip: 真实 TEMP 无顶层文件");
            return;
        }
        let tree = Node {
            name: "Temp".into(),
            path: temp.clone(),
            is_dir: true,
            size: children.iter().map(|c| c.size).sum(),
            file_count: children.len() as u64,
            children,
            scaffold_id: None,
            top_extensions: vec![],
            children_truncated: None,
            mtime: None,
        };
        // 与 executor 修复一致：展开 + 正斜杠规范化。
        let pat = expand_env("%TEMP%/**").replace('\\', "/");
        let glob = globset::GlobBuilder::new(&pat)
            .literal_separator(false)
            .case_insensitive(true)
            .build()
            .unwrap();
        let mut bb = globset::GlobSetBuilder::new();
        bb.add(glob);
        let set = bb.build().unwrap();
        let hits = find_matching_files_on_tree(&tree, &set, None, None, None);
        assert!(
            !hits.is_empty(),
            "规范化后的 %TEMP%/** 必须命中真实 TEMP 文件（执行侧修复）"
        );
    }
}
