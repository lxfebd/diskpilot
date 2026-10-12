//! fancmd 定位与基础工具域（从 selfheal.rs 拆分，2026-10-12）。
//! find_fancmd / run_fancmd / is_admin。

/// 定位 fancmd 可执行文件（打包资源 / 开发目录 / PATH 三级探测）。
#[cfg(windows)]
pub(crate) fn find_fancmd() -> Option<String> {
    // 1) 环境变量显式指路（hw.rs 探测时设置）
    if let Ok(dir) = std::env::var("DISKPILOT_TOOLS_DIR") {
        let p = std::path::Path::new(&dir).join("fancmd.exe");
        if p.is_file() {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    // 2) 开发环境：从当前 exe 上溯找 tools/fancmd/bin（含 Release/net8.0 实际产物目录）
    if let Ok(cur) = std::env::current_exe() {
        let mut probe = cur.clone();
        for _ in 0..6 {
            if !probe.pop() {
                break;
            }
            let cand_direct = probe
                .join("tools")
                .join("fancmd")
                .join("bin")
                .join("fancmd.exe");
            if cand_direct.is_file() {
                return Some(cand_direct.to_string_lossy().into_owned());
            }
            let cand_release = probe
                .join("tools")
                .join("fancmd")
                .join("bin")
                .join("Release")
                .join("net8.0")
                .join("fancmd.exe");
            if cand_release.is_file() {
                return Some(cand_release.to_string_lossy().into_owned());
            }
        }
    }
    // 3) PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let full = dir.join("fancmd.exe");
            if full.is_file() {
                return Some(full.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// 运行 fancmd 并捕获 stdout（超时兜底，非 PowerShell，直接 spawn exe）。
#[cfg(windows)]
pub(crate) fn run_fancmd(args: &[&str]) -> Result<String, String> {
    let exe = find_fancmd()
        .ok_or("未找到 fancmd.exe（开发：tools/fancmd/bin；打包：resources/fancmd）")?;
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    let mut child = std::process::Command::new(&exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .spawn()
        .map_err(|e| format!("启动 fancmd（{exe}）失败：{e}"))?;
    // 后台线程先读 stdout：输出膨胀时若等退出后才读，进程写满 64KB 管道缓冲
    // 会阻塞，与「等退出」互等死锁（与 hw.rs run_fancmd_sensors 同一预读策略）。
    let raw = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    if let Some(mut so) = child.stdout.take() {
        let raw2 = std::sync::Arc::clone(&raw);
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = so.read_to_end(&mut buf);
            if let Ok(mut g) = raw2.lock() {
                *g = buf;
            }
        });
    }
    let started = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(15);
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("等待 fancmd 失败：{e}"));
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("fancmd 执行超时（{}s）", timeout.as_secs()));
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    };
    std::thread::sleep(std::time::Duration::from_millis(50));
    let raw = raw.lock().map(|g| g.clone()).unwrap_or_default();
    if !status.success() && raw.is_empty() {
        return Err(format!("fancmd 退出码 {:?}", status.code()));
    }
    match std::str::from_utf8(&raw) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => {
            let (cow, _, _) = encoding_rs::GBK.decode(&raw);
            Ok(cow.into_owned())
        }
    }
}

/// 是否管理员（S-1-5-32-544 Administrators 组成员）。
#[cfg(windows)]
pub(crate) fn is_admin() -> bool {
    crate::ps::ps_capture(
        "$ErrorActionPreference='SilentlyContinue'; (whoami /groups | Select-String 'S-1-5-32-544' | Measure-Object).Count -gt 0",
    )
    .map(|s| s.trim() == "True")
    .unwrap_or(false)
}
