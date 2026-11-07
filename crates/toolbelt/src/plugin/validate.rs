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

/// 插件入口路径校验（R505）：`tool.plugin.json` 的 `entry` 只允许「插件目录内的
/// 相对子路径」——允许 `bin/demo.exe`，但拒绝 `..` 段、绝对路径、盘符、空段、
/// Windows 保留设备名（按文件名段校验）。否则 `toolbelt_launch` 里
/// `插件目录.join(entry)` 会穿越到 Tools 根外启动任意 exe（本地 zip 直装
/// verify=none 场景可达）。安装/启动时另有 ensure_path_under 做 canonicalize
/// 落点兜底，这里拦的是文本穿越。
pub fn validate_plugin_entry(entry: &str) -> Result<String, ToolbeltError> {
    let t = entry.trim();
    let norm = t.replace('\\', "/");
    let segs: Vec<&str> = norm.split('/').collect();
    let bad = t.is_empty()
        || t.len() > 255
        || norm.starts_with('/')
        || (norm.len() >= 2 && norm.as_bytes()[1] == b':')
        || segs.iter().any(|s| s.is_empty() || *s == "." || *s == "..")
        || t.ends_with('.')
        || t.ends_with(' ');
    // Windows 保留设备名（CON/PRN/AUX/NUL/COM1-9/LPT1-9），按文件名段、大小写不敏感。
    let file_name = segs.last().copied().unwrap_or("");
    let stem = file_name.split('.').next().unwrap_or("");
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    if bad || RESERVED.contains(&stem.to_ascii_lowercase().as_str()) {
        return Err(ToolbeltError::BadArgs(format!(
            "插件 entry「{entry}」非法：只允许插件目录内的相对子路径（拒绝 .. 段、绝对路径、盘符与 Windows 保留名）"
        )));
    }
    Ok(t.to_string())
}

/// 根内落点校验（R505 纵深防御）：`p` 经 canonicalize 后必须仍位于 `root` 之内，
/// 返回解析后的真实路径。用于 `toolbelt_launch`：不管 entry / launchTarget /
/// exe_rel 元数据怎么写，最终要启动的 exe 必须落在 Tools 根内，否则拒绝。
/// canonicalize 失败（文件不存在等）一律拒绝（fail-closed）。
pub fn ensure_path_under(root: &Path, p: &Path) -> Result<std::path::PathBuf, ToolbeltError> {
    let root_c = root
        .canonicalize()
        .map_err(|e| ToolbeltError::BadArgs(format!("无法解析工具根 {}: {e}", root.display())))?;
    let p_c = p
        .canonicalize()
        .map_err(|e| ToolbeltError::BadArgs(format!("无法解析目标路径 {}: {e}", p.display())))?;
    if p_c.starts_with(&root_c) {
        Ok(p_c)
    } else {
        Err(ToolbeltError::BadArgs(format!(
            "拒绝：目标「{}」解析后越出工具根「{}」（防 entry/launchTarget 路径穿越）",
            p.display(),
            root.display()
        )))
    }
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

#[cfg(test)]
mod r505_tests {
    use super::{ensure_path_under, validate_plugin_entry};

    #[test]
    fn entry_允许相对子路径但拒绝穿越() {
        assert_eq!(validate_plugin_entry("tool.exe").unwrap(), "tool.exe");
        assert_eq!(
            validate_plugin_entry("bin/demo.exe").unwrap(),
            "bin/demo.exe"
        );
        assert_eq!(
            validate_plugin_entry(" my-tool.exe ").unwrap(),
            "my-tool.exe"
        );
        for bad in [
            "",
            "..",
            "../evil.exe",
            "..\\evil.exe",
            "a/../evil.exe",
            "C:\\evil.exe",
            "/abs/evil.exe",
            "C:evil.exe",
            "con.exe",
            "bin/CON",
            "AUX",
            "com1.dll",
            "evil.",
            "/lead.exe",
            "a//b.exe",
            &"x".repeat(256),
        ] {
            assert!(
                validate_plugin_entry(bad).is_err(),
                "entry 应被拒绝: {bad:?}"
            );
        }
    }

    #[test]
    fn ensure_path_under_拒绝穿越与根外目标() {
        let dir = std::env::temp_dir().join(format!("dp-r505-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("插件A")).unwrap();
        std::fs::write(dir.join("插件A").join("tool.exe"), b"MZ").unwrap();
        let outside = dir
            .parent()
            .unwrap()
            .join(format!("dp-r505-outside-{}.exe", std::process::id()));
        std::fs::write(&outside, b"MZ").unwrap();

        // 正常：根内文件放行，且返回 canonical 路径
        let p = ensure_path_under(&dir, &dir.join("插件A").join("tool.exe")).unwrap();
        assert!(p.starts_with(dir.canonicalize().unwrap()));
        // 穿越写法（文本 .. 越出根）解析后落在根外 → 拒绝
        assert!(ensure_path_under(&dir, &dir.join("..").join("outside.exe")).is_err());
        // 根内的 .. 折叠后仍在根内 → 放行（语义正确：解析后才是真实落点）
        assert!(ensure_path_under(
            &dir,
            &dir.join("插件A").join("..").join("插件A").join("tool.exe")
        )
        .is_ok());
        // 根外文件 → 拒绝
        assert!(ensure_path_under(&dir, &outside).is_err());
        // 不存在的文件 → fail-closed 拒绝
        assert!(ensure_path_under(&dir, &dir.join("不存在.exe")).is_err());

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&outside);
    }
}
