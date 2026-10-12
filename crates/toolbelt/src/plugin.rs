//! 插件装卸（zip 安装 / 导出 / id 净化）+ 图吧绿色版兜底扫描。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：`install_plugin_zip`（解压 + 路径穿越
//! 防护 + B1 签名校验）、`export_plugin_zip`、插件 id/分类净化，以及定位
//! 「图吧工具箱」绿色版 tools 目录的兜底扫描（find_tuba_green_tools）。

use std::io::{Read, Write};
use std::path::Path;

use crate::catalog::{ToolPluginMeta, ToolbeltError};

/// 从本地 `.zip` 插件包安装：解析包内 `tool.plugin.json` 的 id/category，
/// 解压到 Tools 根对应分类下（插件目录 = `<分类>/<id>`），已存在则报错（避免
/// 覆盖用户数据，升级走「先卸载再安装」）。路径穿越防护：拒绝 `../` 与绝对路径
/// 条目；文件名解码失败跳过。返回安装后的目录相对 Tools 根路径。
pub fn install_plugin_zip(zip_path: &Path, tools_root: &Path) -> Result<String, ToolbeltError> {
    let file = std::fs::File::open(zip_path).map_err(|e| {
        ToolbeltError::BadArgs(format!("无法打开插件包 {}: {e}", zip_path.display()))
    })?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| ToolbeltError::BadArgs(format!("不是合法的 zip 包: {e}")))?;

    // 1. 解析包内 tool.plugin.json（可能在根，也可能在单层子目录里）
    let (meta, base_dir) = {
        let mut found = None;
        for i in 0..archive.len() {
            let mut f = archive
                .by_index(i)
                .map_err(|e| ToolbeltError::BadArgs(e.to_string()))?;
            if f.is_dir() {
                continue;
            }
            let Some(name) = f.name().split('/').next_back() else {
                continue;
            };
            if name == "tool.plugin.json" {
                let mut buf = String::new();
                use std::io::Read;
                f.read_to_string(&mut buf)
                    .map_err(|e| ToolbeltError::BadArgs(e.to_string()))?;
                let meta: ToolPluginMeta = serde_json::from_str(&buf).map_err(|e| {
                    ToolbeltError::BadArgs(format!("tool.plugin.json 解析失败: {e}"))
                })?;
                // 取该文件所在目录（去尾 '/'），作为解压基目录
                let dir = f
                    .name()
                    .rsplit_once('/')
                    .map(|(d, _)| d.to_string())
                    .unwrap_or_default();
                found = Some((meta, dir));
                break;
            }
        }
        found.ok_or_else(|| ToolbeltError::BadArgs("插件包内缺少 tool.plugin.json".into()))?
    };

    // 1.5 签名校验（B1 插件签名，2026-09-09）：
    // 收集 zip 内除 tool.plugin.json 自身外全部文件（路径字典序 → 签名对象），
    // 按 meta 里的 digest/signature/signer 做 sha256 完整性 + Ed25519 验签。
    // verify 字段 = "none" 显式跳过（本地 zip 直装允许无签名）；"sha256" 只查完整性；
    // "ed25519" 或 signature+signer 齐备则强制验签。校验失败直接拒绝安装。
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut f = match archive.by_index(i) {
            Ok(f) => f,
            Err(e) => {
                return Err(ToolbeltError::BadArgs(format!("读取插件包条目失败: {e}")));
            }
        };
        if f.is_dir() {
            continue;
        }
        let name = f.name().replace('\\', "/");
        let base = name.split('/').next().unwrap_or("");
        if base == "tool.plugin.json"
            || name == "tool.plugin.json"
            || name.ends_with("/tool.plugin.json")
        {
            continue; // 清单自证是循环引用，排除
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            continue;
        }
        entries.push((name, buf));
    }
    let verify_mode = meta.verify.trim().to_ascii_lowercase();
    let require_signed = match verify_mode.as_str() {
        "none" => false,
        "sha256" => {
            if let Err(e) = crate::signature::verify_sha256(&entries, &meta.digest) {
                return Err(ToolbeltError::BadArgs(e));
            }
            false
        }
        "ed25519" => true,
        _ => {
            // 未声明 verify 但带签名域 → 走完整校验；只有 signature 没有 signer
            // 之类半截签名也按「未完整签名」处理，只查 digest（如有）。
            let has_sig = !meta.signature.trim().is_empty() && !meta.signer.trim().is_empty();
            if has_sig {
                true
            } else {
                if let Err(e) = crate::signature::verify_sha256(&entries, &meta.digest) {
                    return Err(ToolbeltError::BadArgs(e));
                }
                false
            }
        }
    };
    if require_signed {
        if meta.signer.trim().is_empty() {
            return Err(ToolbeltError::BadArgs(
                "插件声明了 Ed25519 验签但缺少 signer（签名者公钥）".into(),
            ));
        }
        crate::signature::verify_ed25519(&entries, &meta.signature, &meta.signer)
            .map_err(ToolbeltError::BadArgs)?;
    }

    // id/category 净化（到期前是路径穿越的一半：它们直接拼进目录路径）。
    // id 限定 kebab-case（与市场/AI 工具注册共用同一套身份约束），category
    // 只允许「已存在分类或回退到其他工具」——两者都拒绝 `../`、绝对路径段、
    // 分隔符注入，用户不可能通过安装一个 zip 把文件写到 Tools 根之外。
    let plugin_id = validate_plugin_id(&meta.id)?;
    let cat = if meta.category.trim().is_empty() {
        "其他工具".to_string()
    } else {
        validate_plugin_category(&meta.category, tools_root)?
    };
    let cat_dir = tools_root.join(&cat);
    let plugin_dir = cat_dir.join(&plugin_id);
    if plugin_dir.exists() {
        return Err(ToolbeltError::BadArgs(format!(
            "插件「{}」已存在于 {}，先卸载再重装（卸载会移入回收站，可恢复）",
            plugin_id,
            plugin_dir.display()
        )));
    }
    std::fs::create_dir_all(&plugin_dir)
        .map_err(|e| ToolbeltError::BadArgs(format!("创建插件目录失败: {e}")))?;

    // 2. 解压（跳过 tool.plugin.json 本身——已单独解析）。任一文件写入失败
    // 立即整体回滚：删掉刚建的插件目录，不给用户留半个无法启用的插件。
    let extract_err: Option<ToolbeltError> = {
        let mut err = None;
        for i in 0..archive.len() {
            let mut f = match archive.by_index(i) {
                Ok(f) => f,
                Err(e) => {
                    err = Some(ToolbeltError::BadArgs(e.to_string()));
                    break;
                }
            };
            let Some(rel) = f
                .name()
                .strip_prefix(&base_dir)
                .map(|s| s.trim_start_matches('/'))
            else {
                continue;
            };
            if rel.is_empty() || rel == "tool.plugin.json" {
                continue;
            }
            // 路径穿越防护
            let norm = rel.replace('\\', "/");
            if norm.split('/').any(|seg| seg == "..")
                || norm.starts_with('/')
                || (norm.len() >= 2 && norm.as_bytes()[1] == b':')
            {
                continue;
            }
            let dest = plugin_dir.join(norm);
            if f.is_dir() {
                std::fs::create_dir_all(&dest).ok();
            } else {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                let mut out = match std::fs::File::create(&dest) {
                    Ok(o) => o,
                    Err(e) => {
                        err = Some(ToolbeltError::BadArgs(format!(
                            "写入 {} 失败: {e}",
                            dest.display()
                        )));
                        break;
                    }
                };
                if let Err(e) = std::io::copy(&mut f, &mut out) {
                    err = Some(ToolbeltError::BadArgs(format!(
                        "解压 {} 失败: {e}",
                        dest.display()
                    )));
                    break;
                }
            }
        }
        err
    };
    if let Some(e) = extract_err {
        let _ = std::fs::remove_dir_all(&plugin_dir);
        return Err(e);
    }

    Ok(plugin_dir
        .strip_prefix(tools_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default())
}

mod green;
mod validate;
pub(crate) use green::find_tuba_green_tools;
pub use validate::plugin_id_from_name;
pub(crate) use validate::{validate_plugin_category, validate_plugin_id};

/// 把已安装插件目录（根含 `tool.plugin.json`）打包成可分发的 zip，返回
/// 生成的 zip 路径。zip 内结构与 [`install_plugin_zip`] 一致：
/// `tool.plugin.json` 在 zip 根，其余文件原样平铺——这样导出→导入是
/// 完美的 round-trip，别人拿到 zip 直接装就是一样的插件。
///
/// 目标文件已存在时覆盖（重新导出同名包是常见操作）。安全约束：
/// - 只允许打包带 `tool.plugin.json` 的插件目录（复用 catalog 的 id 定位）
/// - 跟随符号链接写入的是目标文件内容，防止把别的目录链进来
pub fn export_plugin_zip(plugin_dir: &Path, out_zip: &Path) -> Result<(), ToolbeltError> {
    let manifest_path = plugin_dir.join("tool.plugin.json");
    if !manifest_path.is_file() {
        return Err(ToolbeltError::BadArgs(format!(
            "「{}」不是插件目录（缺 tool.plugin.json）",
            plugin_dir.display()
        )));
    }
    let meta = std::fs::read_to_string(&manifest_path)
        .map_err(|e| ToolbeltError::BadArgs(format!("读取插件清单失败: {e}")))?
        .trim()
        .to_string();
    if let Some(parent) = out_zip.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let file = std::fs::File::create(out_zip)
        .map_err(|e| ToolbeltError::BadArgs(format!("创建导出包失败: {e}")))?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    // tool.plugin.json 放 zip 根：install 端「单层子目录/根」两处扫描都能命中。
    zw.start_file("tool.plugin.json", opts)
        .map_err(|e| ToolbeltError::BadArgs(format!("写入插件清单失败: {e}")))?;
    zw.write_all(meta.as_bytes())
        .map_err(|e| ToolbeltError::BadArgs(format!("写入插件清单失败: {e}")))?;

    let mut stack = vec![plugin_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            let Ok(meta) = ent.metadata() else { continue };
            if meta.is_dir() {
                stack.push(p);
                continue;
            }
            if !meta.is_file() {
                continue; // 符号链接等：只打普通文件，避免包进意外目标
            }
            // zip 内相对路径 = 相对插件目录（正斜杠），tool.plugin.json 已单独写。
            let rel = p
                .strip_prefix(plugin_dir)
                .map_err(|_| ToolbeltError::BadArgs("路径越界".into()))?;
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if rel_str.is_empty() || rel_str == "tool.plugin.json" {
                continue;
            }
            zw.start_file(&rel_str, opts)
                .map_err(|e| ToolbeltError::BadArgs(format!("写入 {} 失败: {e}", rel_str)))?;
            let Ok(mut f) = std::fs::File::open(&p) else {
                continue;
            };
            let _ = std::io::copy(&mut f, &mut zw);
        }
    }
    zw.finish()
        .map_err(|e| ToolbeltError::BadArgs(format!("封包失败: {e}")))?;
    Ok(())
}
