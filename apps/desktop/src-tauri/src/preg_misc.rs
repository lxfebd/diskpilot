//! 注册表辅助（从 plugin_registry.rs 拆分，2026-10-12）。
//! resolve_tools_root / sweep_orphaned_tmp_dirs。

use tauri::AppHandle;

// ── 写命令：安装 / 更新 / 回滚 ─────────────────────────────────────────

/// 定位 Tools 根（失败返回 Err）。
pub(crate) fn resolve_tools_root(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let explicit = crate::toolbelt::toolbelt_explicit_root(app);
    diskpilot_toolbelt::find_tools_root(explicit.as_deref()).map_err(|e| e.to_string())
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
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_tmp = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e == "dp-old-tmp")
            .unwrap_or(false);
        if is_tmp && path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

// ── 写操作（安装/更新/回滚）拆分到 plugin_mutate.rs（2026-10-12）──
// lib.rs 的 invoke_handler 直接注册 `plugin_registry::plugin_mutate::*`
// （tauri command 宏生成的 __cmd__/__tauri_command_name 符号在定义处模块）。
