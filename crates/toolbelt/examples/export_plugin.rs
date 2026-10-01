//! 发布方式 B 落地（scaffold 社区分享桥的低风险子集）：
//! 把「工具墙插件目录」签上 Ed25519 签名、打成可分发 zip——`cargo run --example export_plugin -- <plugin_dir> <out_zip> [secret_hex]`。
//!
//! 签名是可选项：传了 `secret_hex` 就现场对插件目录补全 `tool.plugin.json`
//! 的 signature/signer/digest/verify（写入目录内清单），再打包；不传则只打包
//! 不解清单（用于本地 zip 直装的无签名分发）。
//!
//! 私钥约定：**绝不落入仓库**。使用者把私钥放在环境变量或自己的钥匙串里，
//! 命令参数会留在 shell 历史里，仅供本机打包用。密钥生成：
//! `generate_keypair()` 或 openssl —— 32 字节原始私钥的 hex。
//!
//! 产物 zip 内的签名对象与安装端验签点同口径（目录内除 tool.plugin.json 外
//! 所有普通文件按 zip 内路径字典序拼帧），所以：导出 → 安装是完美 round-trip。
//! 既有的 `sign_entries`/`verify_ed25519`/`install_plugin_zip` 全部复用，本文件
//! 不做任何重复逻辑。
//!
//! ```text
//! # 无签名打包（本地直装）
//! cargo run --example export_plugin -- "C:/my-tools/MyApp" "dist/myapp.zip"
//! # 签名打包（社区分发 / URL 安装强制）
//! cargo run --example export_plugin -- "C:/my-tools/MyApp" "dist/myapp.zip" "<私钥hex>"
//! ```

use std::path::Path;

use diskpilot_toolbelt::{export_plugin_zip, signature};

fn main() {
    let mut args = std::env::args().skip(1);
    let plugin_dir = args.next().expect("用法: export_plugin <plugin_dir> <out_zip> [secret_hex]");
    let out_zip = args.next().expect("用法: export_plugin <plugin_dir> <out_zip> [secret_hex]");
    let secret = args.next();

    let dir = Path::new(&plugin_dir);
    if !dir.join("tool.plugin.json").is_file() {
        eprintln!("「{plugin_dir}」不是插件目录（缺 tool.plugin.json）");
        std::process::exit(2);
    }

    // 签名：可选。传了私钥就补清单字段，再打包。
    if let Some(sec) = secret.as_deref() {
        match signature::sign_plugin_dir(dir, sec) {
            Ok(()) => {
                let pub_hex = signature::public_key_from_secret(sec).unwrap_or_default();
                println!("已签名插件目录「{plugin_dir}」signer 公钥: {pub_hex}");
            }
            Err(e) => {
                eprintln!("签名失败: {e}");
                std::process::exit(3);
            }
        }
    }

    match export_plugin_zip(dir, Path::new(&out_zip)) {
        Ok(()) => println!("已导出插件包: {out_zip}"),
        Err(e) => {
            eprintln!("导出失败: {e}");
            std::process::exit(1);
        }
    }
}