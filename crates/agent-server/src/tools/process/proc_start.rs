//! 启动进程域（从 process.rs 拆分，2026-10-12）。
//! start_process + is_whitelisted_exe（M2 短名/.. 白名单防线）。

use crate::tools::files::realpath_or_lexical;

#[cfg(windows)]
pub(crate) fn is_whitelisted_exe(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty() {
        return false;
    }
    // 扩展名必须是 .exe（大小写不敏感）
    let lower = cmd.to_ascii_lowercase();
    if !lower.ends_with(".exe") {
        return false;
    }
    // 去掉可能带的可执行文件引号（Start-Process 传参习惯）——这里命令应当
    // 是裸路径，不接受带引号或带参命令（参数走 args 数组）。
    if cmd.starts_with('"') || cmd.contains(' ') {
        return false;
    }
    let p = std::path::Path::new(cmd);
    let absolute = if p.is_absolute() {
        p.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(p)
    } else {
        return false;
    };
    // M2 修复：先做真实路径展开（GetFullPathNameW + GetLongPathNameW），
    // 把 8.3 短名（C:\PROGRA~1）与 `..` 变体（C:\WINDOWS\..\PROGRA~1）
    // 归一化后再比对白名单前缀——否则短名/变体可绕过白名单，把任意
    // 目录下的 exe 当成「已安装程序」启动。展开失败回退词法展开（路径
    // 不存在时短名本不可解析，词法兜底够用）。
    let abs_lower = realpath_or_lexical(&absolute)
        .to_string_lossy()
        .to_ascii_lowercase();
    let mut whitelist: Vec<String> = Vec::new();
    if let Ok(sys) = std::env::var("WINDIR") {
        whitelist.push(format!("{}\\system32", sys.trim_end_matches('\\')).to_lowercase());
        whitelist.push(format!("{}\\syswow64", sys.trim_end_matches('\\')).to_lowercase());
        whitelist.push(
            format!(
                "{}\\system32\\windowspowershell\\v1.0",
                sys.trim_end_matches('\\')
            )
            .to_lowercase(),
        );
    }
    if let Ok(pf) = std::env::var("PROGRAMFILES") {
        whitelist.push(pf.trim_end_matches('\\').to_lowercase());
    }
    if let Ok(pf86) = std::env::var("PROGRAMFILES(X86)") {
        whitelist.push(pf86.trim_end_matches('\\').to_lowercase());
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        whitelist.push(local.trim_end_matches('\\').to_lowercase());
    }
    // ProgramData 下常见应用子目录也放行（如 WinGet 链接），但只放行一级子目录
    if let Ok(pd) = std::env::var("PROGRAMDATA") {
        let pd = pd.trim_end_matches('\\').to_lowercase();
        whitelist.push(format!("{pd}\\microsoft\\windows\\start menu\\programs"));
    }
    // 白名单前缀判定：`abs_lower` 必须**正好等于**白名单目录，或**以
    // `白名单目录 + 路径分隔符` 开头**。只 `starts_with(w)` 会放过
    // `C:\Program Files Evil\x.exe`（Program Files 后接空格）这类逃逸——
    // 白名单目录名被当作任意子目录的前缀。这里强制边界：`\` 之后才算命中。
    whitelist
        .iter()
        .any(|w| abs_lower == w.as_str() || abs_lower.starts_with(&format!("{w}\\")))
}

/// 启动一个已安装程序（白名单执行，写操作）。**由主项目 agent.rs 桥接层做
/// confirmed 确认门**，本函数只负责「能否安全启动 + 执行」。守卫（违反即拒绝）：
/// - 命令不在白名单目录（系统目录/已安装程序目录）或不是 .exe → 拒绝；
/// - 命令不存在 → 拒绝；
/// - 不等待进程退出（启动即返回，分离运行）。
///
/// 返回启动的 PID 与进程名（PID 拿不到时只报进程名）。
#[cfg(windows)]
pub fn start_process(command: &str, args: &[String], cwd: &str) -> Result<String, String> {
    let cmd = command.trim();
    if cmd.is_empty() {
        return Err("命令不能为空".into());
    }
    if !is_whitelisted_exe(cmd) {
        return Err(format!(
            "拒绝：命令「{cmd}」不在白名单内（只允许系统目录或已安装程序目录下的 .exe，且不带参数——参数请走 args 数组）"
        ));
    }
    let path = std::path::PathBuf::from(cmd);
    if !path.is_file() {
        return Err(format!("拒绝：命令「{cmd}」不存在"));
    }
    // cwd 必须存在（不存在就让进程用默认工作目录，不报错——非安全边界）
    let mut child = std::process::Command::new(&path);
    for a in args {
        // 防参数注入：每个参数原样传入，绝不拼接 shell 字符串
        child.arg(a);
    }
    if !cwd.trim().is_empty() {
        let cd = std::path::PathBuf::from(cwd.trim());
        if cd.is_dir() {
            child.current_dir(&cd);
        }
    }
    // 分离运行：spawn 后不 wait（启动器语义，进程自己跑）
    let spawned = child.spawn().map_err(|e| format!("启动进程失败：{e}"))?;
    let pid = spawned.id();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| cmd.to_string());
    Ok(format!("已启动 {name}（PID {pid}）"))
}

#[cfg(not(windows))]
pub fn start_process(_command: &str, _args: &[String], _cwd: &str) -> Result<String, String> {
    Ok("当前平台不是 Windows，进程启动不可用".into())
}
