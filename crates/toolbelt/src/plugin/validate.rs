//! 插件 id / 分类净化域：validate_plugin_id / plugin_id_from_name /
//! validate_plugin_category。从 plugin.rs 拆分（2026-10-12）。
//! kebab-case 身份约束与 Tools 分类目录校验，防目录越界注入。

use std::path::Path;

use crate::catalog::ToolbeltError;
/// 插件 id 必须是 kebab-case（小写字母/数字开头，可含 `-`），与市场/AI 工具
/// 注册共用同一身份约束。除格式外天然排除 `..`、`/`、`\`、空格等目录越界字符。
pub(crate) fn validate_plugin_id(id: &str) -> Result<String, ToolbeltError> {
    let t = id.trim();
    let valid = !t.is_empty()
        && t.len() <= 64
        && t.as_bytes()[0].is_ascii_lowercase()
        && t.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid {
        return Err(ToolbeltError::BadArgs(format!(
            "插件 id「{id}」非法：只允许小写字母/数字/连字符（kebab-case，1-64 位）"
        )));
    }
    Ok(t.to_string())
}

/// 把任意工具/插件「显示名」净化成稳定的 kebab-case 插件 id：
/// 小写化 → 非字母数字字符折叠为 `-` → 去首尾/重复 `-` → 前缀补 `t`（数字开头），
/// 保证结果一定通过 [`validate_plugin_id`]（市场索引/AI 工具注册的持久化身份）。
/// 空输入返回 Err。
pub fn plugin_id_from_name(name: &str) -> Result<String, ToolbeltError> {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = false;
    for b in name.to_ascii_lowercase().bytes() {
        if b.is_ascii_lowercase() || b.is_ascii_digit() {
            out.push(b as char);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        return Err(ToolbeltError::BadArgs(format!(
            "无法从名称「{name}」生成插件 id（没有可用字符）"
        )));
    }
    // 数字开头非法：补一个稳定前缀
    let mut final_id = out;
    if final_id.as_bytes()[0].is_ascii_digit() {
        final_id.insert(0, 't');
    }
    if final_id.len() > 64 {
        final_id.truncate(64);
        while final_id.ends_with('-') {
            final_id.pop();
        }
    }
    Ok(final_id)
}

/// 插件分类必须落到 Tools 根下已存在的分类目录（'' 映射已在上层处理）。
/// 既防 `../`、绝对路径注入，也避免「安装即新建一个误拼的分类目录」。
pub(crate) fn validate_plugin_category(
    cat: &str,
    tools_root: &Path,
) -> Result<String, ToolbeltError> {
    let t = cat.trim();
    let bad_seg = t
        .split(['/', '\\'])
        .any(|seg| seg.is_empty() || seg == "." || seg == ".." || seg.contains(':'));
    if t.is_empty() || t.len() > 64 || bad_seg || t.contains('\0') {
        return Err(ToolbeltError::BadArgs(format!(
            "插件分类「{cat}」非法：请用 Tools 下已有的分类目录名"
        )));
    }
    let cand = tools_root.join(t);
    if !cand.is_dir() {
        return Err(ToolbeltError::BadArgs(format!(
            "插件分类「{cat}」不存在于 Tools 根（可用分类见工具墙左侧导航）"
        )));
    }
    Ok(t.to_string())
}
