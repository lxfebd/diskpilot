//! agent.rs 单元/集成测试（从 agent.rs 拆分，2026-10-12）。
//! 二进制定位 / 工具清单 77 工具 / 写工具确认门 / 只读工具放行 / scaffolds 注入。

use super::*;

/// 端到端验证（需要 agent-server 已构建）：
/// 真实 spawn 子进程 + MCP 握手 + list_all_tools + call_tool。
/// 二进制定位与环境变量 DISKPILOT_AGENT_SERVER / 当前目录同 target 策略一致。
fn has_binary() -> bool {
    agent_server_binary().is_some()
        || std::path::Path::new("target/debug/agent-server").is_file()
        || std::path::Path::new("target/debug/agent-server.exe").is_file()
}

/// 纯函数：平台相关的二进制名 —— Windows 带 `.exe`，mac/Linux 裸名。
/// 钉死这个约定，防止未来某个平台回归成硬编码 `.exe`。
#[test]
fn agent_server_bin_name_matches_platform() {
    let name = if cfg!(windows) {
        "agent-server.exe"
    } else {
        "agent-server"
    };
    // 用与 agent_server_binary 相同的判定路径验证候选名能拼出正确形态
    let candidates = [
        format!("target/debug/{name}"),
        format!("../../target/debug/{name}"),
        format!("../../../target/debug/{name}"),
    ];
    for c in &candidates {
        assert!(c.ends_with(name), "候选路径应以 {name} 结尾: {c}");
    }
    #[cfg(windows)]
    assert!(name.ends_with(".exe"));
    #[cfg(not(windows))]
    assert!(!name.ends_with(".exe"));
}

#[tokio::test]
async fn agent_list_tools_returns_77_tools() {
    if !has_binary() {
        eprintln!("skip: agent-server 未构建（cargo build -p agent-server）");
        return;
    }
    let tools = agent_list_tools().await.expect("list tools 应成功");
    assert_eq!(
        tools.len(),
        77,
        "应返回 77 个 MCP 工具（66 只读 + 11 写操作，含 AIDA64 复刻 13 个 + 内存/GPU 压测/蓝屏 3 个 + 信息补全 4 个 + 自研 SuperIO 直读 1 个 + 纯只读 sensor_history/observations_overview 2 个）"
    );
    // 抽查几个关键工具存在
    let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
    for expect in [
        "disk_health",
        "list_dir",
        "system_info",
        "list_processes",
        "hw_disk_smart",
        "net_status",
        "security_event_logs",
        "fan_control",
        "bench_cpu",
        "bench_memory",
        "bench_disk",
        "stress_test",
        "system_report",
        "observations_overview",
        "sensor_trend",
        "sensor_history",
        "sensor_alert",
        "hw_cpu_features",
        "hw_dram_timings",
        "hw_displays",
        "bench_gpu",
        "hw_ipmi",
        "hw_acpi",
        "mem_test",
        "stress_test_gpu",
        "bsod_analyze",
        "app_licenses",
        "net_adapter_detail",
        "sys_defender_status",
        "sys_user_accounts",
    ] {
        assert!(names.contains(&expect.to_string()), "缺少工具 {expect}");
    }
    // 程序化校验「只读 / 写」分布，而不是依赖硬编码的数字：
    // 工具墙徽标与确认门都吃同一份 writable，这里保证两侧一致。
    for t in &tools {
        assert_eq!(
            t.writable,
            WRITE_TOOLS.contains(&t.name.as_str()),
            "工具 {} 的 writable 与 WRITE_TOOLS 不一致",
            t.name
        );
    }
    let write_n = tools.iter().filter(|t| t.writable).count();
    let read_n = tools.len() - write_n;
    assert!(
        write_n == WRITE_TOOLS.len() && write_n > 0,
        "写工具数与 WRITE_TOOLS 应一致，got read={read_n} write={write_n}"
    );
    assert!(
        read_n > write_n,
        "只读工具应占多数，got read={read_n} write={write_n}"
    );
    // 每个工具的 schema 是 JSON 对象
    for t in &tools {
        assert!(
            t.input_schema.is_object(),
            "工具 {} 的 schema 应为对象",
            t.name
        );
    }
}

#[tokio::test]
async fn agent_call_tool_disk_health_returns_data() {
    if !has_binary() {
        eprintln!("skip: agent-server.exe 未构建");
        return;
    }
    let r = agent_call_tool_core(
        "disk_health".into(),
        serde_json::json!({}),
        None,
        None,
        None,
    )
    .await
    .expect("call_tool 应成功");
    // Windows 上 agent-server 用 GetLogicalDrives 枚举真实磁盘，必有数据；
    // 非 Windows collect_disks 返回空 → 工具如实报错（功能降级），测试接受：
    // 链路通（call_tool 不 panic）即视为通过，只有 Windows 才钉死内容断言。
    #[cfg(windows)]
    {
        assert!(!r.is_error, "disk_health 不应报错：{}", r.text);
        assert!(
            r.text.contains("磁盘概览"),
            "应返回磁盘概览，got: {}",
            r.text
        );
    }
    #[cfg(not(windows))]
    eprintln!("skip 内容断言: disk_health 在非 Windows 返回 {:?}", r.text);
}

#[tokio::test]
async fn agent_call_tool_list_dir_drive_root_readonly_allowed() {
    if !has_binary() {
        eprintln!("skip: agent-server.exe 未构建");
        return;
    }
    // list_dir C:\ 属只读枚举，PathGuard 只读口径（check_read）放行盘根：
    // AI 分析空间分布的必要入口。写工具（file_recycle 等）仍拒绝盘根。
    // 用 C:\ 盘根实测（读不到完整内容也不应报「安全守卫」错，最多报
    // 读取失败/权限），断言不出现守卫拦截文案即可。
    let r = agent_call_tool_core(
        "list_dir".into(),
        serde_json::json!({ "path": "C:\\" }),
        None,
        None,
        None,
    )
    .await
    .expect("call_tool 应成功");
    assert!(
        !r.text.contains("安全守卫"),
        "只读 list_dir 盘根不应被守卫拦，got: {}",
        r.text
    );
}

#[tokio::test]
async fn agent_call_tool_list_dir_real_subdir_readonly_allowed() {
    if !has_binary() {
        eprintln!("skip: agent-server.exe 未构建");
        return;
    }
    // 用户真实场景：AI 列某个非系统子目录（项目目录）。只读枚举真实存在的
    // 子目录本就该放行；曾因 check 拒绝非白名单路径而误杀（AI 编造路径与
    // 守卫拒绝叠加成「进不去」）。断言不出现守卫文案。用环境变量探测一个
    // 真实非系统目录（用户目录子项）；目录不存在时报「路径不存在」是正确
    // 行为，不在本用例断言范围。
    let home = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from);
    let probe = home.as_deref().and_then(|h| {
        std::fs::read_dir(h)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.is_dir() && !p.to_string_lossy().contains("AppData"))
    });
    let Some(real_dir) = probe else {
        eprintln!("skip: 找不到可用的真实非系统目录，跳过");
        return;
    };
    let r = agent_call_tool_core(
        "list_dir".into(),
        serde_json::json!({ "path": real_dir.to_string_lossy().into_owned() }),
        None,
        None,
        None,
    )
    .await
    .expect("call_tool 应成功");
    assert!(
        !r.text.contains("安全守卫"),
        "只读 list_dir 真实子目录不应被守卫拦，got: {}",
        r.text
    );
}

#[tokio::test]
async fn agent_call_tool_disk_top_directories_drive_root_readonly_allowed() {
    if !has_binary() {
        eprintln!("skip: agent-server.exe 未构建");
        return;
    }
    // disk_top_directories 有双层守卫（mod.rs check_read + diskx.rs 内部第二层
    // PathGuard）。两侧都必须放行盘根，否则 AI 分析全盘空间分布会被拦在门外。
    // 断言不出现守卫拦截文案即可（C:\ 全盘遍历可能较慢，45s blocking_call
    // 上限内正常返回或报「读取失败」都算放行）。
    let r = agent_call_tool_core(
        "disk_top_directories".into(),
        serde_json::json!({ "path": "C:\\", "top_n": 5 }),
        None,
        None,
        None,
    )
    .await
    .expect("call_tool 应成功");
    assert!(
        !r.text.contains("安全守卫"),
        "只读 disk_top_directories 盘根不应被守卫拦，got: {}",
        r.text
    );
}

#[tokio::test]
async fn agent_server_ready_matches_binary() {
    // 纯函数探活：逻辑上应与 agent_server_binary() 一致
    assert_eq!(agent_server_ready(), agent_server_binary().is_some());
}

/// A1 回归：connect() 必须给 agent-server 注入 DISKPILOT_SCAFFOLDS，
/// 否则 MCP cleanup_suggestions 在打包运行时找不到 scaffold 清单
/// （编译期 CARGO_MANIFEST_DIR 兜底只在 dev 生效）。
#[test]
fn connect_injects_scaffolds_env() {
    let candidates = scaffolds_candidates();
    assert!(
        !candidates.is_empty(),
        "scaffolds 候选列表不应为空（至少含 env + exe 同目录 + dev 布局）"
    );
    // 3 号候选 = CARGO_MANIFEST_DIR/../../../scaffolds = 仓库根 scaffolds/
    // （apps/desktop/src-tauri 向上 3 级）。当前仓库该目录真实存在，
    // 说明 dev 布局解析正确；不强制 exists（打包场景只有前两个候选），
    // 但仓库内必然命中。
    let repo_scaffolds =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../scaffolds");
    assert_eq!(
        candidates.last().unwrap(),
        &repo_scaffolds,
        "dev 布局候选应为 CARGO_MANIFEST_DIR/../../../scaffolds"
    );
    let resolved = resolve_scaffolds_dir();
    assert!(
        resolved.is_some(),
        "仓库根 scaffolds/ 应能被解析到（dev 布局兜底）"
    );
    if let Some(dir) = resolved {
        // 至少含一份 scaffold TOML（磁盘上能真实读出）
        let n_toml = std::fs::read_dir(&dir)
            .expect("scaffolds 目录应可读")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().map(|t| t.is_file()).unwrap_or(false)
                    && e.file_name().to_string_lossy().ends_with(".toml")
            })
            .count();
        assert!(n_toml > 0, "scaffolds 目录 {} 下应有 TOML", dir.display());
    }
}

/// B2/B3 回归：工具进度通道端到端可用。
/// 真实 spawn + MCP 握手 + tools/call，钉死「服务端 ProgressSlot → 转发器 →
/// 客户端 ProgressClientHandler::on_progress → core 闭包」整条链路。
/// 完成帧（progress=1000，progress.rs::send_completion）保证任何成功工具
/// 至少产生一条通知——毫秒级快工具也不会因转发器首帧竞态而 0 通知。
#[tokio::test]
async fn agent_call_tool_progress_channel_delivers_notifications() {
    if !has_binary() {
        eprintln!("skip: agent-server.exe 未构建");
        return;
    }
    let dir = std::env::temp_dir().join(format!("dp-progress-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("临时目录应可创建");
    for i in 0..3u8 {
        std::fs::write(dir.join(format!("f{i}.bin")), vec![i; 1024]).expect("写测试文件");
    }

    let seen: Arc<std::sync::Mutex<Vec<(u64, String)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    let progress: Arc<dyn Fn(u64, String) + Send + Sync + 'static> = Arc::new(move |pct, msg| {
        sink.lock()
            .unwrap_or_else(|p| p.into_inner())
            .push((pct, msg));
    });

    let r = agent_call_tool_core(
        "disk_find_biggest_files".into(),
        serde_json::json!({ "path": dir.display().to_string(), "top_n": 5 }),
        None,
        None,
        Some(progress),
    )
    .await
    .expect("call_tool 应成功");
    let _ = std::fs::remove_dir_all(&dir);

    assert!(!r.is_error, "工具应成功返回：{}", r.text);
    let got = seen.lock().unwrap().clone();
    assert!(
        !got.is_empty(),
        "progress 闭包应至少收到完成帧（send_completion 保证）"
    );
    for (pct, _) in &got {
        assert!(*pct <= 1000, "千分比口径越界: {pct}");
    }
    assert!(
        got.iter().any(|(_, m)| m.as_str() == "大文件扫描"),
        "通知 message 应为工具 label，got: {got:?}"
    );
    assert!(
        got.iter().any(|(pct, _)| *pct == 1000),
        "应收到 progress=1000 完成帧，got: {got:?}"
    );
}
