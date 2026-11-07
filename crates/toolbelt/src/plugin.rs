//! 插件装卸（zip 安装 / 导出 / id 净化）+ 图吧绿色版兜底扫描。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：`install_plugin_zip`（解压 + 路径穿越
//! 防护 + B1 签名校验）、`export_plugin_zip`、插件 id/分类净化，以及定位
//! 「图吧工具箱」绿色版 tools 目录的兜底扫描（find_tuba_green_tools）。

use std::io::{Read, Write};
use std::path::Path;

use crate::catalog::{ToolPluginMeta, ToolbeltError};

/// R505 zip 炸弹上限：条目数 / 单条解压后大小 / 总解压大小。
/// 远程下载侧另有 256MB 压缩包上限（net_http），这里兜住解压膨胀。
pub(crate) const MAX_ZIP_ENTRIES: usize = 4096;
pub(crate) const MAX_ZIP_ENTRY_BYTES: u64 = 512 * 1024 * 1024; // 单文件 512MB
pub(crate) const MAX_ZIP_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 总解压 2GB

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
            let Ok(raw_name) = f.name().map(|c| c.into_owned()) else {
                continue;
            };
            let Some(name) = raw_name.split('/').next_back() else {
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
                let dir = raw_name
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
    // R505 zip 炸弹上限：条目数 / 单条 / 总量（签名阶段全量入内存，解压阶段落盘）。
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(ToolbeltError::BadArgs(format!(
            "插件包条目数过多（{} > {MAX_ZIP_ENTRIES}），拒绝安装（疑似 zip 炸弹）",
            archive.len()
        )));
    }
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let f = match archive.by_index(i) {
            Ok(f) => f,
            Err(e) => {
                return Err(ToolbeltError::BadArgs(format!("读取插件包条目失败: {e}")));
            }
        };
        if f.is_dir() {
            continue;
        }
        let Ok(nm) = f.name() else {
            continue;
        };
        let name = nm.replace('\\', "/");
        let base = name.split('/').next().unwrap_or("");
        if base == "tool.plugin.json"
            || name == "tool.plugin.json"
            || name.ends_with("/tool.plugin.json")
        {
            continue; // 清单自证是循环引用，排除
        }
        // R505 单条上限：用 Take 读，超出即拒绝（不信 size() 声明值）。
        let mut buf = Vec::new();
        let take = f.size().min(MAX_ZIP_ENTRY_BYTES + 1);
        if f.take(take).read_to_end(&mut buf).is_err() {
            continue;
        }
        if buf.len() as u64 > MAX_ZIP_ENTRY_BYTES {
            return Err(ToolbeltError::BadArgs(format!(
                "插件包内文件「{name}」超过单条上限 {MAX_ZIP_ENTRY_BYTES} 字节，拒绝安装"
            )));
        }
        total += buf.len() as u64;
        if total > MAX_ZIP_TOTAL_BYTES {
            return Err(ToolbeltError::BadArgs(format!(
                "插件包解压总量超过上限 {MAX_ZIP_TOTAL_BYTES} 字节，拒绝安装（疑似 zip 炸弹）"
            )));
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
        // R505：require 场景 signature 也必须非空——verify_ed25519 对空签名
        // 会静默 Ok，这里断死，防止 verify="ed25519"+空 signature 零校验安装。
        if meta.signer.trim().is_empty() || meta.signature.trim().is_empty() {
            return Err(ToolbeltError::BadArgs(
                "插件声明了 Ed25519 验签但 signature/signer 不完整（缺签名或缺签名者公钥）".into(),
            ));
        }
        crate::signature::verify_ed25519(&entries, &meta.signature, &meta.signer)
            .map_err(ToolbeltError::BadArgs)?;
    }

    // R505：entry 只允许单层文件名——否则 launch 时 插件目录.join(entry) 可穿越到
    // Tools 根外启动任意 exe（本地 zip 直装 verify=none 场景可达）。
    let entry = validate_plugin_entry(&meta.entry)?;

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
        let mut total_written: u64 = 0; // R505 解压总量上限累计
        for i in 0..archive.len() {
            let f = match archive.by_index(i) {
                Ok(f) => f,
                Err(e) => {
                    err = Some(ToolbeltError::BadArgs(e.to_string()));
                    break;
                }
            };
            let Ok(nm) = f.name() else {
                continue;
            };
            let Some(rel) = nm
                .strip_prefix(&base_dir)
                .map(|s| s.trim_start_matches('/'))
            else {
                continue;
            };
            if rel.is_empty() || rel == "tool.plugin.json" {
                continue;
            }
            // R505 路径穿越防护（加强版）：Windows 会把段尾的空格/点规范化掉
            // （".. " → ".."），所以逐段 trim_end_matches(['.', ' ']) 后再判 ..；
            // 同时拒绝含 ':' 的段（ADS/备用流 `x.exe:ads` 与盘符）、空段、
            // 绝对路径与 Windows 保留设备名。
            let norm = rel.replace('\\', "/");
            let bad_seg = norm.split('/').any(|seg| {
                let t = seg.trim_end_matches(['.', ' ']);
                // 单个 "." 合法；".."/".. " 与空段/含 ':'/保留设备名拒绝。
                (t.is_empty() && seg != ".")
                    || t == ".."
                    || t.contains(':')
                    || (t != "." && is_reserved_device_name(t))
            });
            if bad_seg || norm.starts_with('/') {
                continue;
            }
            let dest = plugin_dir.join(&norm);
            // R505 解压上限：单条不信 size()，copy 时用 Take 卡死；总量累计卡死。
            if f.is_dir() {
                std::fs::create_dir_all(&dest).ok();
            } else {
                if f.size() > MAX_ZIP_ENTRY_BYTES {
                    err = Some(ToolbeltError::BadArgs(format!(
                        "插件包内文件「{norm}」超过单条上限 {MAX_ZIP_ENTRY_BYTES} 字节，拒绝安装"
                    )));
                    break;
                }
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
                total_written = total_written.saturating_add(f.size());
                if total_written > MAX_ZIP_TOTAL_BYTES {
                    err = Some(ToolbeltError::BadArgs(format!(
                        "插件包解压总量超过上限 {MAX_ZIP_TOTAL_BYTES} 字节，拒绝安装（疑似 zip 炸弹）"
                    )));
                    break;
                }
                let mut limited = f.take(MAX_ZIP_ENTRY_BYTES + 1);
                if let Err(e) = std::io::copy(&mut limited, &mut out) {
                    err = Some(ToolbeltError::BadArgs(format!(
                        "解压 {} 失败: {e}",
                        dest.display()
                    )));
                    break;
                }
                if limited.limit() == 0 {
                    err = Some(ToolbeltError::BadArgs(format!(
                        "插件包内文件「{norm}」超过单条上限 {MAX_ZIP_ENTRY_BYTES} 字节，拒绝安装"
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

    // R505 纵深：解压完成后，entry 对应的文件必须真实存在，且 canonicalize
    // 后仍位于插件目录内（防御历史遗留的 symlink / 竞态替换）。
    let entry_path = plugin_dir.join(&entry);
    if let Err(e) = ensure_path_under(&plugin_dir, &entry_path) {
        let _ = std::fs::remove_dir_all(&plugin_dir);
        return Err(e);
    }
    if !entry_path.is_file() {
        let _ = std::fs::remove_dir_all(&plugin_dir);
        return Err(ToolbeltError::BadArgs(format!(
            "插件 entry「{entry}」在包内不存在（解压后未找到该文件）"
        )));
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
pub use validate::{ensure_path_under, validate_plugin_entry};
pub(crate) use validate::{validate_plugin_category, validate_plugin_id};

/// Windows 保留设备名（CON/PRN/AUX/NUL/COM1-9/LPT1-9），按「主名段」大小写不敏感比较。
/// R505：zip 条目段命中即拒绝（防止写出 `run.exe:ads` 之外的设备名路径）。
pub(crate) fn is_reserved_device_name(seg: &str) -> bool {
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let stem = seg.split('.').next().unwrap_or("");
    RESERVED.contains(&stem.to_ascii_lowercase().as_str())
}

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
