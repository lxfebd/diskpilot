//! 插件签名验证测试（B1，2026-09-09）：
//! - sha256 完整性校验
//! - Ed25519 验签 round-trip / 篡改拒绝
//! - install_plugin_zip 的验签点：声明 ed25519 的包必须验签通过才解压；
//!   无签名本地 zip 直装放行；verify=none 跳过
//! 私钥在测试内临时生成，绝不进仓库。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use diskpilot_toolbelt::signature::{
    generate_keypair, sign_entries, verify_ed25519, verify_sha256,
};

fn tempdir(tag: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("dp_sig_{}_{}_{}", tag, std::process::id(), n));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

/// 手工打包一个插件 zip：tool.plugin.json 在根，其余文件平铺。
/// `entries` = (zip 内路径, 内容)。返回 zip 路径。
fn make_plugin_zip(dir: &Path, meta_json: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    let zip_path = dir.join("plugin.zip");
    let f = fs::File::create(&zip_path).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default();
    zw.start_file("tool.plugin.json", opts).unwrap();
    zw.write_all(meta_json.as_bytes()).unwrap();
    for (rel, content) in entries {
        zw.start_file(*rel, opts).unwrap();
        zw.write_all(content).unwrap();
    }
    zw.finish().unwrap();
    zip_path
}

/// 对「除清单外全部文件」构造签名对象条目（与规范一致：正斜杠路径）。
fn payload_entries(entries: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
    entries
        .iter()
        .map(|(p, c)| (p.to_string(), c.to_vec()))
        .collect()
}

#[test]
fn sha256_digest_verifies_and_detects_tamper() {
    let entries = payload_entries(&[("run.bat", b"@echo hi"), ("data/cfg.toml", b"x=1")]);
    let digest = diskpilot_toolbelt::signature::sha256_hex(&entries);
    assert_eq!(digest.len(), 64);

    assert!(verify_sha256(&entries, &digest).is_ok());
    assert!(
        verify_sha256(&entries, &digest.to_uppercase()).is_ok(),
        "hex 大小写不敏感"
    );

    let tampered = payload_entries(&[("run.bat", b"@echo EVIL"), ("data/cfg.toml", b"x=1")]);
    assert!(
        verify_sha256(&tampered, &digest).is_err(),
        "篡改后 digest 必须不匹配"
    );
}

#[test]
fn ed25519_roundtrip_and_tamper_rejection() {
    let (pub_hex, sec_hex) = generate_keypair();
    assert_eq!(pub_hex.len(), 64);
    assert_eq!(sec_hex.len(), 64);

    let entries = payload_entries(&[("run.bat", b"@echo hi"), ("data/cfg.toml", b"x=1")]);
    let sig = sign_entries(&entries, &sec_hex).unwrap();
    assert_eq!(sig.len(), 128); // 64 字节 hex

    assert!(verify_ed25519(&entries, &sig, &pub_hex).is_ok());

    // 篡改一个字节 → 验签失败
    let tampered = payload_entries(&[("run.bat", b"@echo hi!"), ("data/cfg.toml", b"x=1")]);
    assert!(verify_ed25519(&tampered, &sig, &pub_hex).is_err());

    // 错公钥 → 失败
    let (other_pub, _) = generate_keypair();
    assert!(verify_ed25519(&entries, &sig, &other_pub).is_err());
}

#[test]
fn unsigned_entries_pass_when_no_signature_fields() {
    // 无 signature/signer → 通过（交给 require_signed 策略）
    let entries = payload_entries(&[("a.txt", b"hello")]);
    assert!(verify_ed25519(&entries, "", "").is_ok());
}

#[test]
fn install_rejects_tampered_signed_plugin() {
    let dir = tempdir("reject_tamper");
    let tools_root = dir.join("Tools");
    fs::create_dir_all(tools_root.join("其他工具")).unwrap();

    let (pub_hex, sec_hex) = generate_keypair();
    let entries = payload_entries(&[("run.bat", b"@echo hi"), ("data/cfg.toml", b"x=1")]);
    let sig = sign_entries(&entries, &sec_hex).unwrap();
    let digest = diskpilot_toolbelt::signature::sha256_hex(&entries);

    // 声明 ed25519 强制验签的清单
    let meta = format!(
        r#"{{"id":"sig-test","name":"签名测试","version":"1.0.0","entry":"run.bat",
             "category":"其他工具","signature":"{sig}","signer":"{pub_hex}","digest":"{digest}","verify":"ed25519"}}"#
    );

    // 篡改版：run.bat 内容改了但签名没变 → 安装必须被拒绝，且不产生插件目录
    let zip = make_plugin_zip(
        &dir,
        &meta,
        &[("run.bat", b"@echo EVIL"), ("data/cfg.toml", b"x=1")],
    );
    let err = diskpilot_toolbelt::install_plugin_zip(&zip, &tools_root).unwrap_err();
    assert!(
        err.to_string().contains("签名校验失败") || err.to_string().contains("digest"),
        "篡改包应被拒绝，实际错误: {err}"
    );
    assert!(
        !tools_root.join("其他工具").join("sig-test").exists(),
        "拒绝后不得留下插件目录"
    );
}

#[test]
fn install_accepts_correctly_signed_plugin() {
    let dir = tempdir("accept_signed");
    let tools_root = dir.join("Tools");
    fs::create_dir_all(tools_root.join("其他工具")).unwrap();

    let (pub_hex, sec_hex) = generate_keypair();
    let entries = payload_entries(&[("run.bat", b"@echo hi"), ("data/cfg.toml", b"x=1")]);
    let sig = sign_entries(&entries, &sec_hex).unwrap();
    let digest = diskpilot_toolbelt::signature::sha256_hex(&entries);
    let meta = format!(
        r#"{{"id":"sig-ok","name":"签名测试","version":"1.0.0","entry":"run.bat",
             "category":"其他工具","signature":"{sig}","signer":"{pub_hex}","digest":"{digest}","verify":"ed25519"}}"#
    );
    let zip = make_plugin_zip(
        &dir,
        &meta,
        &[("run.bat", b"@echo hi"), ("data/cfg.toml", b"x=1")],
    );

    let installed = diskpilot_toolbelt::install_plugin_zip(&zip, &tools_root).unwrap();
    assert_eq!(installed, "其他工具/sig-ok");
    assert!(tools_root.join("其他工具/sig-ok/run.bat").is_file());
}

#[test]
fn install_accepts_unsigned_plugin_when_verify_none() {
    let dir = tempdir("accept_unsigned");
    let tools_root = dir.join("Tools");
    fs::create_dir_all(tools_root.join("其他工具")).unwrap();

    let meta = r#"{"id":"no-sig","name":"无签名","version":"1.0.0","entry":"run.bat",
                   "category":"其他工具","verify":"none"}"#;
    let zip = make_plugin_zip(&dir, meta, &[("run.bat", b"@echo hi")]);

    let installed = diskpilot_toolbelt::install_plugin_zip(&zip, &tools_root).unwrap();
    assert_eq!(installed, "其他工具/no-sig");
}
