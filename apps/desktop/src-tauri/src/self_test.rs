//! --self-test 无头自检（R513）：CI 在 ubuntu/macos/windows 上跑
//! `diskpilot-desktop --self-test`，不建窗口，验证关键安全/扫描路径后退出。
//! 全部通过 exit 0，任一失败 exit 1 并打印 FAIL 行——清掉 security-model 里
//! 「桌面运行时未在 CI 自动化验证」的已知边界。

/// 跑全部自检项，返回失败数（main.rs 的 --self-test 入口调用）。
pub fn run() -> i32 {
    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: &str| {
        if ok {
            println!("PASS {name}: {detail}");
        } else {
            println!("FAIL {name}: {detail}");
            failures += 1;
        }
    };

    // 1. 扫描核心：临时目录造 3 个文件，scan_with_stats 应看到 3 个文件。
    let tmp = std::env::temp_dir().join(format!("diskpilot-self-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).ok();
    for i in 0..3 {
        std::fs::write(tmp.join(format!("f{i}.txt")), b"hello").ok();
    }
    let scan_res = diskpilot_scanner::scan_with_stats(&tmp, Default::default(), |_| {});
    match scan_res {
        Ok((_, stats)) => check(
            "scanner.walk",
            stats.files_seen == 3,
            &format!("files_seen={} mode={}", stats.files_seen, stats.mode),
        ),
        Err(e) => check("scanner.walk", false, &format!("扫描失败: {e:#}")),
    }
    std::fs::remove_dir_all(&tmp).ok();

    // 2. Ed25519 签名：生成密钥对 → 签名 → 验签通过；篡改 payload → 验签失败。
    // generate_keypair 返回 (公钥 hex, 私钥 hex)。
    let (pubkey, secret) = diskpilot_toolbelt::signature::generate_keypair();
    let entries = vec![(String::new(), b"envelope-payload".to_vec())];
    let sig = diskpilot_toolbelt::signature::sign_entries(&entries, &secret);
    match sig {
        Ok(sig) => {
            let ok = diskpilot_toolbelt::signature::verify_ed25519(&entries, &sig, &pubkey).is_ok();
            let tampered = vec![(String::new(), b"tampered".to_vec())];
            let bad =
                diskpilot_toolbelt::signature::verify_ed25519(&tampered, &sig, &pubkey).is_err();
            check(
                "signature.ed25519",
                ok && bad,
                if ok && bad {
                    "验签通过且篡改被拒"
                } else {
                    "验签/篡改拒绝异常"
                },
            );
        }
        Err(e) => check("signature.ed25519", false, &format!("签名失败: {e}")),
    }

    // 3. nonce 闸：签发→消费一次成功；重复消费失败；op 不匹配不消费原 nonce。
    let guard = crate::confirm_nonce::ConfirmGuard::new();
    let n = guard.issue("execute_scope");
    match n {
        Ok(n) => {
            let once = guard.consume("execute_scope", &n).is_ok();
            let twice = guard.consume("execute_scope", &n).is_err();
            let n2 = guard.issue("undo").ok();
            let mismatch_keeps = match &n2 {
                Some(n2) => {
                    guard.consume("clean_scope", n2).is_err() && guard.consume("undo", n2).is_ok()
                }
                None => false,
            };
            check(
                "nonce.once",
                once && twice && mismatch_keeps,
                if once && twice && mismatch_keeps {
                    "一次性消费 + 重复拒绝 + op 不匹配不吞 nonce"
                } else {
                    "nonce 语义异常"
                },
            );
        }
        Err(e) => check("nonce.once", false, &format!("签发失败: {e}")),
    }

    // 4. nonce 频率限制：同 op 连发 12 次应触发 60s 窗口上限（10 次）。
    let guard2 = crate::confirm_nonce::ConfirmGuard::new();
    let mut allowed = 0;
    for _ in 0..12 {
        if let Ok(n) = guard2.issue("plugin_install") {
            if guard2.consume("plugin_install", &n).is_ok()
                && guard2.hit_rate("plugin_install").is_ok()
            {
                allowed += 1;
            }
        }
    }
    check(
        "nonce.rate_limit",
        (1..=2).contains(&allowed),
        &format!(
            "1s 窗口内 12 次连发只放行 {allowed} 次（per-op 1s 上限 1 次，跨秒边界最多 2 次）"
        ),
    );

    // 5. write_gate：dry_run 放行；真写无 confirmed 拒绝。
    let guard3 = crate::confirm_nonce::ConfirmGuard::new();
    let dry_ok =
        crate::confirm_nonce::write_gate(&guard3, "execute_scope", true, None, None).is_ok();
    let real_blocked =
        crate::confirm_nonce::write_gate(&guard3, "execute_scope", false, None, None).is_err();
    check(
        "nonce.write_gate",
        dry_ok && real_blocked,
        if dry_ok && real_blocked {
            "dry_run 放行 + 无确认的真写被拒"
        } else {
            "write_gate 语义异常"
        },
    );

    if failures == 0 {
        println!("SELF-TEST OK: 全部通过");
        0
    } else {
        println!("SELF-TEST FAILED: {failures} 项失败");
        1
    }
}
