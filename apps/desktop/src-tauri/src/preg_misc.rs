//! 注册表辅助（从 plugin_registry.rs 拆分，2026-10-12）。
//! resolve_tools_root / sweep_orphaned_tmp_dirs。

use tauri::AppHandle;

// ── 写命令：安装 / 更新 / 回滚 ─────────────────────────────────────────

/// 定位 Tools 根（失败返回 Err）。
pub(crate) fn resolve_tools_root(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let explicit = crate::toolbelt::toolbelt_explicit_root(app);
    diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())
}

/// R502：台账回读路径的安全拼接。
/// 台账（plugin-ledger.json）是本地普通 JSON，`dir_rel` / `blob` 字段不可信：
/// 含 `..`、绝对路径或根/前缀成分时一律拒绝，防止越出 base 目录后被
/// rename / remove_dir_all / 任意 zip 当备份安装。
pub(crate) fn safe_join(base: &std::path::Path, rel: &str) -> Result<std::path::PathBuf, String> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err("台账路径字段为空".into());
    }
    let p = std::path::Path::new(rel);
    if p.is_absolute() {
        return Err(format!("台账路径不允许绝对路径：{rel}"));
    }
    // R505：Windows 会把段尾的空格/点规范化掉（".. " → ".."），所以不能只靠
    // components() 认字面量 ".."——逐段 trim_end_matches(['.', ' ']) 后再判。
    for seg in rel.split(['/', '\\']) {
        let t = seg.trim_end_matches(['.', ' ']);
        // 单个 "."（CurDir）合法放行；".."、".. "、"... " 这类尾点尾空格段，
        // 以及空段/含 ':' 段一律拒绝（Windows 会把 ".. " 规范化成 ".."）。
        if t == ".." || t.contains(':') || (t.is_empty() && seg != ".") {
            return Err(format!("台账路径不允许包含 .. / 空段 / 盘符段：{rel}"));
        }
    }
    for comp in p.components() {
        match comp {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            _ => return Err(format!("台账路径不允许包含 .. 或根成分：{rel}")),
        }
    }
    Ok(base.join(p))
}

/// 启动清理：update/rollback 在「移开旧目录」后若进程崩溃，会留下
/// `*.dp-old-tmp` 残留目录（旧版被移走、新版没装上）。应用下次启动时
/// 扫描 Tools 根，把这类残留目录整个删掉——旧版内容已备份进 blob 缓存
/// （update 前打包成 zip，rollback 用 target.blob），可随时恢复。
/// 静默失败：清理是增强行为，不阻塞启动。
pub(crate) fn sweep_orphaned_tmp_dirs(app: &AppHandle) {
    let Ok(root) = resolve_tools_root(app) else {
        return;
    };
    // R505：tmp 目录实际落在「分类层」（<分类>/<id>.dp-old-tmp），根一层扫不到，
    // 改为递归两层（根 + 分类目录），跳过符号链接，限深度防滥用。
    fn sweep_dir(dir: &std::path::Path, depth: u8) {
        if depth > 2 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // symlink_metadata：不跟随链接，避免清扫动作触碰根外目标。
            let Ok(md) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if md.file_type().is_symlink() {
                continue;
            }
            if md.is_dir() {
                let is_tmp = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e == "dp-old-tmp")
                    .unwrap_or(false);
                if is_tmp {
                    let _ = std::fs::remove_dir_all(&path);
                } else {
                    sweep_dir(&path, depth + 1);
                }
            }
        }
    }
    sweep_dir(&root, 0);
}

#[cfg(test)]
mod tests {
    use super::safe_join;
    use std::path::Path;

    #[test]
    fn safe_join_accepts_normal_rel() {
        let base = Path::new("C:/tools");
        assert_eq!(
            safe_join(base, "系统工具/foo").unwrap(),
            base.join("系统工具/foo")
        );
        assert_eq!(safe_join(base, "./a").unwrap(), base.join("a"));
    }

    #[test]
    fn safe_join_rejects_traversal_and_absolute() {
        let base = Path::new("C:/tools");
        for bad in [
            "../evil",
            "a/../../b",
            "..",
            "C:/Windows",
            "/etc/passwd",
            "",
        ] {
            assert!(safe_join(base, bad).is_err(), "必须拒绝：{bad:?}");
        }
    }
}

// ── 写操作（安装/更新/回滚）拆分到 plugin_mutate.rs（2026-10-12）──
// lib.rs 的 invoke_handler 直接注册 `plugin_registry::plugin_mutate::*`
// （tauri command 宏生成的 __cmd__/__tauri_command_name 符号在定义处模块）。
