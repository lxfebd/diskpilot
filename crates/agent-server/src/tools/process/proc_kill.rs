//! 结束进程域（从 process.rs 拆分，2026-10-12）。
//! KILL_GUARD_PS + kill_process（白名单守卫 + PowerShell 只读 $pid 防护）。

#[cfg(windows)]
pub(crate) const KILL_GUARD_PS: &str = r#"
$ErrorActionPreference='Stop'
function is-system-dir([string]$p) {
  if(-not $p){ return $true }
  $sys=[System.Environment]::SystemDirectory
  $windir=[System.Environment]::GetFolderPath('Windows')
  $root=[System.IO.Path]::GetPathRoot($p)
  $pl=$p.ToLowerInvariant()
  return ($pl.StartsWith($sys.ToLowerInvariant())) -or ($pl.StartsWith($windir.ToLowerInvariant())) -or ($pl.StartsWith($root.ToLowerInvariant()+'windows'))
}
function blocked-pid([int]$targetPid) {
  # 系统关键进程 PID + 系统目录里的进程一律拒绝。
  # 注意参数名必须避开 $pid：它是 PowerShell 内置只读自动变量
  # （当前 PS 进程 PID），用 $pid 当参数会让绑定直接报
  # "Cannot overwrite variable pid because it is read-only or constant"
  # ——整个守卫脚本执行失败，任何进程都杀不掉（2026-10-11 实测修复）。
  if($targetPid -lt 5){ return $true }
  $p=Get-Process -Id $targetPid -ErrorAction SilentlyContinue
  if(-not $p){ return $true }  # 不存在=已被结束，也不用杀
  $pp=$null
  try { $pp=$p.Path } catch {}
  return is-system-dir $pp
}
if(blocked-pid $args[0]) {
  Write-Output 'GUARD_BLOCKED'
  exit 0
}
Stop-Process -Id $args[0] -Force -ErrorAction Stop
Write-Output 'KILLED'
"#;

#[cfg(windows)]
pub fn kill_process(pid: u32) -> Result<String, String> {
    if pid < 5 {
        return Err(format!(
            "进程 PID {pid} 是系统关键进程，拒绝结束（最小允许 PID 为 5）"
        ));
    }
    // 拒绝结束调用者自身（agent-server 子进程持有句柄，自杀无意义且可能卡住）
    if pid == std::process::id() {
        return Err("拒绝结束 agent-server 自身进程".into());
    }
    // 预检：PID 不存在直接给清晰中文提示（避免抛 PowerShell 裸退出码）
    let probe = crate::ps::ps_capture(&format!(
        "if (Get-Process -Id {} -ErrorAction SilentlyContinue) {{ 'EXISTS' }} else {{ 'MISSING' }}",
        pid
    ))
    .map_err(|e| format!("进程查询失败：{e}"))?;
    if probe.trim() == "MISSING" {
        return Err(format!("进程 PID {pid} 不存在（可能已退出）"));
    }
    let out = crate::ps::ps_capture(&format!("{KILL_GUARD_PS} {pid}"))
        .map_err(|e| format!("结束进程失败：{e}"))?;
    let trimmed = out.trim();
    if trimmed == "GUARD_BLOCKED" {
        Err("拒绝：目标进程是系统进程或位于系统目录，不可结束（安全守卫）".into())
    } else if trimmed == "KILLED" {
        Ok(format!("已结束进程 PID {pid}"))
    } else {
        Err(format!("结束进程未确认成功，输出：{trimmed}"))
    }
}

#[cfg(not(windows))]
pub fn kill_process(_pid: u32) -> Result<String, String> {
    Ok("当前平台不是 Windows，进程控制不可用".into())
}
