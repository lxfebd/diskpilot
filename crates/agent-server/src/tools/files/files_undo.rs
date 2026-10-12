//! 目录树/撤销/回收域（从 files.rs 拆分，2026-10-12）。
//! TreeNode/file_tree + UndoEntry/append_undo + require_confirmed + recycle_file。

use std::path::Path;

use serde::Serialize;

use crate::tools::parallel_walker;

// PathGuard 仅供 cfg(windows) 的 recycle_file 使用（Linux 降级返回不引用它）。
#[cfg(windows)]
use super::files_guard::PathGuard;

#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: Option<u64>,
    pub depth: usize,
}

/// 目录树（深度限制，带大小摘要），供 AI 回答"哪占空间"的结构化补充。
/// 返回浅层目录树而非铺满，避免刷屏。
pub fn file_tree(
    root: &Path,
    max_depth: usize,
    max_nodes: usize,
) -> Result<(Vec<TreeNode>, bool), String> {
    let mut nodes = Vec::new();
    let mut truncated = false;
    for entry in parallel_walker(root) {
        let Ok(entry) = entry else { continue };
        if entry.path_is_symlink() {
            continue; // 防环
        }
        let depth = entry.depth();
        if depth > max_depth.max(1) {
            truncated = true;
            continue;
        }
        if nodes.len() >= max_nodes {
            truncated = true;
            break;
        }
        let ft = entry.file_type();
        let size = if ft.is_file() {
            entry.metadata().map(|m| m.len()).ok()
        } else {
            None
        };
        nodes.push(TreeNode {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path().display().to_string(),
            is_dir: ft.is_dir(),
            size_bytes: size,
            depth,
        });
    }
    Ok((nodes, truncated))
}

/// undo.jsonl 条目（与主项目 diskpilot-executor 的 `UndoEntry` serde 同构，
/// 让 agent-server 的回收也能写进同一份 `~/.diskpilot/undo.jsonl`，兑现
/// 「一切删除可撤销」承诺）。agent-server 保持零横向依赖，故不 use
/// diskpilot-executor，只镜像字段名——主项目 `list_undo` 反序列化的是
/// JSON 形状，不关心类型来源。
#[derive(serde::Serialize)]
// Linux 下仅测试经 files.rs 的 cfg(test) re-export 引用 → 豁免 dead_code。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct UndoEntry {
    pub(crate) timestamp: String,
    #[serde(rename = "action")]
    pub(crate) action: &'static str,
    pub(crate) source: String,
    pub(crate) destination: Option<String>,
    pub(crate) reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bytes_freed: Option<u64>,
}

/// 把 `DISKPILOT_UNDO_LOG` 指向的文件追加一行 undo 条目。
/// 主进程 agent.rs spawn agent-server 时注入该环境变量；未注入（直接跑
/// agent-server / 测试）时静默跳过——写 undo 是增强，不能阻塞回收本身。
/// 纯追加（OpenOptions append），与主项目 write_log 同语义。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn append_undo(entries: &[UndoEntry]) {
    let Some(path) = std::env::var_os("DISKPILOT_UNDO_LOG") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    use std::io::Write;
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    for e in entries {
        if let Ok(line) = serde_json::to_string(e) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// 生成 RFC3339 时间戳（undo 条目的 `timestamp` 字段）。
#[cfg_attr(not(windows), allow(dead_code))]
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// 统计路径占用的真实字节：文件取 len；目录递归求和（jwalk，与 scanner
/// 同款并行遍历）。字节用于 undo 条目的 `bytes_freed`（前端「已清理」展示
/// 真实值，不再一律标预估）。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn path_bytes(p: &std::path::Path) -> Option<u64> {
    if p.is_file() {
        return p.metadata().ok().map(|m| m.len());
    }
    if p.is_dir() {
        let mut sum = 0u64;
        for entry in parallel_walker(p) {
            let Ok(entry) = entry else { continue };
            if entry.file_type().is_file() {
                sum = sum.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
            }
        }
        return Some(sum);
    }
    None
}

/// 写操作确认令牌（纵深防御第二层）：只有经主进程确认门（agent.rs 注入
/// `DISKPILOT_CONFIRMED=1`）的调用才放行。绕过主进程直连 agent-server
/// （任意 MCP 客户端直接发 tools/call）时该环境变量缺失，一律拒绝——
/// 第一层是主进程 `confirmed==Some(true)` 硬门，这层兜住进程级绕过。
pub fn require_confirmed() -> Result<(), String> {
    if std::env::var_os("DISKPILOT_CONFIRMED").is_some() {
        Ok(())
    } else {
        Err("agent-server:confirm: 此写操作必须经 DiskPilot 确认门（confirmed=true）后执行；检测到绕过主进程的直接调用，已拒绝。".into())
    }
}

/// 把单个文件/目录移入系统回收站（写操作，**可逆**）。**由主项目 agent.rs
/// 桥接层做 confirmed 确认门**，本函数只负责「能否安全回收 + 执行」。
/// 守卫（违反即拒绝）：
/// - 路径不存在 → 拒绝；
/// - 盘根 / 系统目录 / 用户主目录根 → 拒绝（复用 PathGuard，回收站也不能碰
///   这些位置）；
/// - 循环调用自身不存在——agent-server 无界面，只提供工具接口。
///
/// 回收成功即写一条 undo 日志（DISKPILOT_UNDO_LOG，与主项目同文件），并
/// 统计实际释放字节（bytes_freed）。
/// 回收失败（被占用/无权限）明确报错，不静默；绝不 delete，只进回收站。
#[cfg(windows)]
pub fn recycle_file(path: &str) -> Result<String, String> {
    require_confirmed()?;
    let p = std::path::Path::new(path.trim());
    if p.as_os_str().is_empty() {
        return Err("路径不能为空".into());
    }
    let guard = PathGuard;
    // 写操作：系统目录下的文件（cmd.exe 等）也拒绝，不只拦系统根自身
    if let Err(e) = guard.check_for_write(p) {
        return Err(format!("拒绝：{e}（安全守卫，回收站也不可操作此位置）"));
    }
    // 回收前统计真实字节（回收成功后路径已消失，必须提前取）。
    let bytes = path_bytes(p).unwrap_or(0);
    // 复用 workspace trash crate（与主项目 executor 同款，含误报兜底语义）
    let (ok, message) = match trash::delete(p) {
        Ok(()) => (true, format!("已将「{}」移入回收站（可还原）", p.display())),
        Err(e) => {
            if !p.exists() {
                // 与主项目 executor 同款：报错但路径已消失 = 实际已回收
                (
                    true,
                    format!(
                        "已将「{}」移入回收站（系统返回警告「{e}」，但路径已消失，判定为已回收，可还原）",
                        p.display()
                    ),
                )
            } else {
                (
                    false,
                    format!(
                        "回收失败：{}（路径仍在，可能被占用或权限不足；文件未被删除）",
                        e
                    ),
                )
            }
        }
    };
    if ok {
        append_undo(std::slice::from_ref(&UndoEntry {
            timestamp: now_rfc3339(),
            action: "recycle",
            source: p.display().to_string(),
            destination: None,
            reason: "AI 回收（已确认）".into(),
            bytes_freed: Some(bytes),
        }));
    }
    Ok(message)
}

#[cfg(not(windows))]
pub fn recycle_file(_path: &str) -> Result<String, String> {
    Ok("当前平台不是 Windows，回收站操作不可用".into())
}
