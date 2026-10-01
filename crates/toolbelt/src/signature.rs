//! 插件包签名（Ed25519）：zip 包完整性的密码学校验。
//!
//! 签名模型（写死在规范里，注释即契约）：
//! - **签名对象** = 插件包内除 `tool.plugin.json` 自身外的**所有文件字节**，
//!   按 zip 内路径（正斜杠）字典序拼接（每个文件 = `[u64 长度 LE][路径 UTF-8][内容]`）。
//!   `tool.plugin.json` 里的 `digest` / `signature` 描述的是**其他文件**——
//!   清单自证自己的签名是循环引用，不存在（此设计下清单可被信任，因为
//!   它是由下载通道（https + 社区注册表）整体送达的）。
//! - `digest`（可选） = sha256_hex(签名对象)（不带签名也允许，用于完整性）。
//! - `signature`（可选，hex） = ed25519 对签名对象的签名。
//! - `signer`（可选） = 签名者公钥的 hex（ed25519 公钥，32 字节）。
//! - 验签规则：`signature` 与 `signer` 同时存在才强制校验；只有 `digest` 时
//!   校验 sha256；两者都缺 = 未签名插件（本地 zip 直装仍允许，URL 安装强制要求）。
//!
//! 私钥永不进仓库：密钥生成/签名只用于打包工具与测试（`sign_plugin_dir`），
//! 仓库内的密钥对仅测试临时生成。

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
/// 计算插件包「签名对象」的 sha256（hex 小写）。传入 zip 内各文件
/// （路径为 zip 内正斜杠相对路径），会按路径字典序自动排序拼接。
/// `tool.plugin.json` 自身必须**排除**（循环引用，见模块注释）。
pub fn sha256_hex(entries: &[(String, Vec<u8>)]) -> String {
    let mut hasher = Sha256::new();
    let mut sorted: Vec<_> = entries.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, content) in &sorted {
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update(content);
    }
    hex::encode(hasher.finalize())
}

/// 计算单个文件的 sha256（hex 小写）——**裸文件字节**哈希（用于下载包完整性
/// 校验、blob 缓存命中判定）。注意与 [`sha256_hex`] 的区别：后者是对「签名对象」
/// （路径帧 + 字典序拼接）做哈希，只用于插件包签名模型，不能用于文件完整性。
pub fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 校验 sha256 digest（hex，大小写不敏感）。`digest` 为空视为通过。
pub fn verify_sha256(entries: &[(String, Vec<u8>)], digest: &str) -> Result<(), String> {
    if digest.trim().is_empty() {
        return Ok(());
    }
    let expect = digest.trim().to_ascii_lowercase();
    if expect.len() != 64 || !expect.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("插件 digest 格式非法（应为 64 位 hex）".into());
    }
    let actual = sha256_hex(entries);
    if actual == expect {
        Ok(())
    } else {
        Err(format!(
            "插件 digest 校验失败：期望 {expect}，实际 {actual}（包内容被改动过）"
        ))
    }
}

/// 校验 Ed25519 签名。`signature`/`signer` 任一为空 = 未签名，直接通过
/// （调用方可用 `require_signed` 强制）。公钥/签名 hex 非法或验签失败返回 Err。
pub fn verify_ed25519(
    entries: &[(String, Vec<u8>)],
    signature_hex: &str,
    signer_hex: &str,
) -> Result<(), String> {
    let sig = signature_hex.trim();
    let signer = signer_hex.trim();
    if sig.is_empty() || signer.is_empty() {
        return Ok(()); // 未签名：交给调用方的 require_signed 策略决定
    }
    let vk_bytes = decode_hex(signer, "签名者公钥").map_err(|e| e)?;
    if vk_bytes.len() != 32 {
        return Err("签名者公钥长度非法（应为 32 字节 hex）".into());
    }
    let sig_bytes = decode_hex(sig, "签名").map_err(|e| e)?;
    if sig_bytes.len() != 64 {
        return Err("签名长度非法（应为 64 字节 hex）".into());
    }

    // 签名对象按与 sha256 相同规则（字典序拼接）计算。
    let mut payload: Vec<u8> = Vec::new();
    let mut sorted: Vec<_> = entries.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, content) in &sorted {
        payload.extend_from_slice(&(path.len() as u64).to_le_bytes());
        payload.extend_from_slice(path.as_bytes());
        payload.extend_from_slice(content);
    }

    let vk = VerifyingKey::from_bytes(
        &vk_bytes
            .try_into()
            .map_err(|_| "公钥字节错误".to_string())?,
    )
    .map_err(|e| format!("公钥非法: {e}"))?;
    let sig = Signature::from_bytes(
        &sig_bytes
            .try_into()
            .map_err(|_| "签名字节错误".to_string())?,
    );
    vk.verify(&payload, &sig)
        .map_err(|e| format!("签名校验失败: {e}（插件可能被篡改）"))
}

/// 生成 Ed25519 密钥对，返回 (公钥 hex, 私钥 hex)。仅测试/打包工具用。
pub fn generate_keypair() -> (String, String) {
    // 线程安全随机源（os_rng 默认 feature 可用）。
    let mut bytes = [0u8; 32];
    getrandom_bytes(&mut bytes);
    let sk = SigningKey::from_bytes(&bytes);
    let vk = sk.verifying_key();
    (hex::encode(vk.to_bytes()), hex::encode(sk.to_bytes()))
}

/// 用私钥（hex）对「签名对象」签名，返回签名 hex。仅打包工具/测试用。
pub fn sign_entries(entries: &[(String, Vec<u8>)], secret_hex: &str) -> Result<String, String> {
    let sk_bytes = decode_hex(secret_hex.trim(), "私钥").map_err(|e| e)?;
    if sk_bytes.len() != 32 {
        return Err("私钥长度非法（应为 32 字节 hex）".into());
    }
    let sk = SigningKey::from_bytes(
        &sk_bytes
            .try_into()
            .map_err(|_| "私钥字节长度错误".to_string())?,
    );
    let mut payload: Vec<u8> = Vec::new();
    let mut sorted: Vec<_> = entries.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, content) in &sorted {
        payload.extend_from_slice(&(path.len() as u64).to_le_bytes());
        payload.extend_from_slice(path.as_bytes());
        payload.extend_from_slice(content);
    }
    let sig: Signature = sk.sign(&payload);
    Ok(hex::encode(sig.to_bytes()))
}

/// 由私钥（hex）派生公钥（hex）。发布工具用：`sign_entries` 只签不回键，
/// 清单里要写 `signer` 必须从这里取（`generate_keypair` 生成的是随机新对，
/// 这里是从既有私钥推出同一密钥对的公钥）。
pub fn public_key_from_secret(secret_hex: &str) -> Result<String, String> {
    let sk_bytes = decode_hex(secret_hex.trim(), "私钥")?;
    if sk_bytes.len() != 32 {
        return Err("私钥长度非法（应为 32 字节 hex）".into());
    }
    let sk = SigningKey::from_bytes(
        &sk_bytes
            .try_into()
            .map_err(|_| "私钥字节长度错误".to_string())?,
    );
    Ok(hex::encode(sk.verifying_key().to_bytes()))
}

/// 给插件目录补签名后写回 `tool.plugin.json`（signature + signer + digest + verify）。
/// 签名对象 = 目录内除 `tool.plugin.json` 自身外的所有普通文件，按 zip 内路径
/// （相对目录、正斜杠）字典序拼接——与 [`crate::export_plugin_zip`] 打出的包、
/// [`crate::install_plugin_zip`] 验签点同一口径。清单用 `serde_json::Value`
/// 原位改动，保留作者手写的其余字段不重排。私钥只在调用方手里，永不落盘。
pub fn sign_plugin_dir(plugin_dir: &std::path::Path, secret_hex: &str) -> Result<(), String> {
    use std::io::Read;

    let manifest_path = plugin_dir.join("tool.plugin.json");
    let mut text = String::new();
    std::fs::File::open(&manifest_path)
        .and_then(|mut f| f.read_to_string(&mut text))
        .map_err(|e| format!("读取插件清单失败: {e}"))?;
    let mut meta: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("插件清单解析失败: {e}"))?;

    // 收集签名对象（与 export_plugin_zip 同口径：普通文件、跳过清单自身）。
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
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
                continue; // 符号链接等：与 export 一致，不纳入签名对象
            }
            let rel = p
                .strip_prefix(plugin_dir)
                .map_err(|_| "路径越界".to_string())?;
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if rel_str.is_empty() || rel_str == "tool.plugin.json" {
                continue;
            }
            let Ok(bytes) = std::fs::read(&p) else {
                continue;
            };
            entries.push((rel_str, bytes));
        }
    }

    let sig = sign_entries(&entries, secret_hex)?;
    let pub_hex = public_key_from_secret(secret_hex)?;
    let digest = sha256_hex(&entries);
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("signature".into(), serde_json::Value::String(sig));
        obj.insert("signer".into(), serde_json::Value::String(pub_hex));
        obj.insert("digest".into(), serde_json::Value::String(digest));
        obj.insert("verify".into(), serde_json::Value::String("ed25519".into()));
    }
    let out = serde_json::to_string_pretty(&meta).map_err(|e| format!("序列化清单失败: {e}"))?;
    std::fs::write(&manifest_path, out).map_err(|e| format!("写回清单失败: {e}"))?;
    Ok(())
}

/// 预检 zip 内 `tool.plugin.json` 是否带完整 Ed25519 签名（signature + signer 齐备，
/// 且 verify 字段不是 "none"）。只读解压清单，不解压内容。URL 安装强制要求
/// 签名（本地 zip 直装才允许无签名）。
pub fn plugin_zip_has_ed25519(zip_path: &std::path::Path) -> Result<bool, String> {
    use std::io::Read;
    let file = std::fs::File::open(zip_path).map_err(|e| format!("无法打开插件包: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("不是合法 zip 包: {e}"))?;
    for i in 0..archive.len() {
        let mut f = archive
            .by_index(i)
            .map_err(|e| format!("读取插件包失败: {e}"))?;
        if f.is_dir() {
            continue;
        }
        if f.name().split('/').next_back() == Some("tool.plugin.json") {
            let mut buf = String::new();
            f.read_to_string(&mut buf)
                .map_err(|e| format!("读取插件清单失败: {e}"))?;
            let meta: crate::ToolPluginMeta =
                serde_json::from_str(&buf).map_err(|e| format!("插件清单解析失败: {e}"))?;
            if meta.verify.trim().eq_ignore_ascii_case("none") {
                return Ok(false);
            }
            return Ok(!meta.signature.trim().is_empty() && !meta.signer.trim().is_empty());
        }
    }
    Ok(false)
}

fn decode_hex(s: &str, what: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{what}格式非法（应为 hex）"));
    }
    hex::decode(s).map_err(|e| format!("{what}解码失败: {e}"))
}

// getrandom 的极简封装（ed25519-dalek 内部已依赖 getrandom，这里复用它生成密钥）。
fn getrandom_bytes(out: &mut [u8]) {
    use getrandom::getrandom;
    let _ = getrandom(out);
}
