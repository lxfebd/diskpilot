//! DiskPilot 共享的 PowerShell 原语。
//!
//! 三份实现在重构前分别维护于：
//! - `apps/desktop/src-tauri/src/hw.rs::ps_capture`（原始版，无 UTF-8 首行注入）
//! - `crates/agent-server/src/ps.rs`（移植改良版：首行强制 UTF-8 输出编码）
//! - `crates/agent-server/src/tools/sys.rs::escape_ps_single_quoted` + desktop `startup.rs`
//!
//! 统一到这里后，desktop 与 agent-server 共用同一份执行器/转义/探针脚本，
//! 避免「一边改了 UTF-8 根治、另一边还只靠 GBK 兜底」这类漂移。
//!
//! **跨平台**：`ps_capture` / `ps_capture_timeout` 仅在 Windows 有真实现
//! （PowerShell.exe）；非 Windows 构建返回明确错误，不 panic、不 spawn 失败命令。
//! 消费模块（hw/net/sys/…）自身已有 `#[cfg(not(windows))]` 降级分支，运行时
//! 不会走到这里的非 Windows 实现——它只为让那些模块顶层的 use 能在
//! macOS/Linux 上通过编译。

// Read/Stdio 仅供 cfg(windows) 的 PowerShell 执行器使用（非 Windows 返回 Err 分支无进程 IO）。
#[cfg(windows)]
use std::io::Read;
#[cfg(windows)]
use std::process::Stdio;
use std::time::Duration;

/// 跑一段只读 PowerShell/CIM 脚本并捕获 stdout（默认 30s 超时）。
#[cfg(windows)]
pub fn ps_capture(script: &str) -> Result<String, String> {
    ps_capture_timeout(script, Duration::from_secs(30))
}

/// 带超时的 PowerShell 执行器。
///
/// - CREATE_NO_WINDOW 语义（stdin/stderr null）、try_wait 轮询超时强杀；
/// - 脚本首行强制 `[Console]::OutputEncoding = UTF8`，根治中文系统 GBK 乱码
///   （GBK 兜底降为二阶保险）；
/// - 中文 Windows 上控制台输出是 GBK，UTF-8 失败时按 GBK 兜底解码（encoding_rs）。
#[cfg(windows)]
pub fn ps_capture_timeout(script: &str, timeout: Duration) -> Result<String, String> {
    // 首行注入 UTF-8 输出编码，避免中文系统 GBK 乱码（GBK 兜底仍在下面保留）
    let full = format!(
        "[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false);\n{script}"
    );
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &full,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 PowerShell 失败：{e}"))?;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("等待 PowerShell 失败：{e}"));
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("PowerShell 查询超时（{}s）", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(60));
    };
    let mut raw = Vec::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = so.read_to_end(&mut raw);
    }
    // 把 stderr 也读空，避免管道 buffer 残留导致句柄泄漏
    if let Some(mut se) = child.stderr.take() {
        let mut sink = Vec::new();
        let _ = se.read_to_end(&mut sink);
    }
    if !status.success() && raw.is_empty() {
        return Err(format!("PowerShell 退出码 {:?}", status.code()));
    }
    match std::str::from_utf8(&raw) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => {
            #[cfg(windows)]
            {
                let (cow, _, _) = encoding_rs::GBK.decode(&raw);
                Ok(cow.into_owned())
            }
            #[cfg(not(windows))]
            Ok(String::from_utf8_lossy(&raw).into_owned())
        }
    }
}

/// 非 Windows 平台：PowerShell 不存在，返回明确错误（消费模块的降级分支负责解释）。
#[cfg(not(windows))]
pub fn ps_capture(_script: &str) -> Result<String, String> {
    Err("当前平台不是 Windows，PowerShell 查询不可用".into())
}

/// 非 Windows 平台：PowerShell 不存在，返回明确错误。
#[cfg(not(windows))]
pub fn ps_capture_timeout(_script: &str, _timeout: Duration) -> Result<String, String> {
    Err("当前平台不是 Windows，PowerShell 查询不可用".into())
}

/// PS 单引号字符串字面量转义：单引号翻倍（`''`）是 PS 里唯一需要的转义。
///
/// 所有把外部可控文本（注册表值名、服务名、Wi-Fi 名、用户输入…）放进
/// PowerShell 源码字面量的位置都必须过这里，否则 `'` 可逃出字符串、
/// `;` 后接任意命令（命令注入）。desktop 与 agent-server 共用的安全铁律。
#[cfg(windows)]
pub fn escape_ps_single_quoted(s: &str) -> String {
    s.replace('\'', "''")
}

/// `ConvertTo-Json` 单项数组折叠成对象的坑：PS 里 `@(Get-CimInstance ...)` 只有 1 条时
/// 序列化出来是 `{...}` 而不是 `[{...}]`。本函数把文本包成 JSON 数组（若已是数组则原样）。
/// 用法：脚本末尾 `$out | ConvertTo-Json -Depth 4 -Compress`，结果再经本函数包数组。
pub fn json_array_of(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return "[]".into();
    }
    if t.starts_with('[') {
        return t.to_string();
    }
    format!("[{t}]")
}

/// AMD ADL（atiadlxx.dll）显卡温度/占用探针脚本。
///
/// 原双份维护于 desktop `hw.rs` 与 agent-server `tools/hw.rs`（2927 字符完全
/// 相同），统一迁入此 crate 防漂移。
///
/// 实现：PowerShell Add-Type 动态 P/Invoke（零新增 Rust 依赖，零 windows-sys
/// feature）；签名对齐 AMD ADL SDK 官方头文件（非网传变体）：
/// - `ADL_Main_Control_Create(ADL_Main_Memory_Alloc, int iEnumConnectedAdapters)`
///   必须传 allocator 回调，传 0 会导致后续枚举时内部内存分配失败；
/// - `ADL_Adapter_NumberOfAdapters_Get(int* lpNumAdapters)`
/// - `ADL_GetTemperature(int iAdapterIndex, int* lpTemperature)` —— 2 参数官方签名；
///   同时声明 4 参数变体 EntryPoint 容错旧 DLL，先试 2 参再试 4 参；
/// - `ADL_GLUtilization_Get(int iAdapterIndex, int* gpu, int* mem, int* eng)`
///
/// 温度单位：官方文档是千分之一摄氏度（65 ℃ → 65000），个别旧 DLL 直接摄氏，
/// 脚本按值域启发式换算（>300 视为 milli）。
///
/// DLL 不存在/初始化失败/无 A 卡 → `note != 'OK'`，消费方静默降级（N 卡机
/// 正常情况，不算错误）；PowerShell 子进程崩了 → ps_capture Err → 外层吞 None。
pub const ADL_PROBE_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$dll='C:\Windows\System32\atiadlxx.dll'
$result=@{ init_rc=-1; note=''; adapters=@() }
if(-not (Test-Path $dll)) {
  $result.note='NO_AMD_ADL'
  $result | ConvertTo-Json -Depth 4 -Compress
  exit 0
}
try {
  Add-Type -Namespace DiskPilot -Name ADL -TypeDefinition @'
using System.Runtime.InteropServices;
public static class ADL {
  public delegate System.IntPtr ADL_Main_Memory_Alloc(int size);
  public static System.IntPtr Alloc(int size) { return System.Runtime.InteropServices.Marshal.AllocHGlobal(size); }
  [DllImport("atiadlxx.dll", CallingConvention=CallingConvention.StdCall)]
  public static extern int ADL_Main_Control_Create(ADL_Main_Memory_Alloc cb, int iEnumConnectedAdapters);
  [DllImport("atiadlxx.dll", CallingConvention=CallingConvention.StdCall)]
  public static extern int ADL_Adapter_NumberOfAdapters_Get(out int lpNumAdapters);
  [DllImport("atiadlxx.dll", CallingConvention=CallingConvention.StdCall, EntryPoint="ADL_GetTemperature")]
  public static extern int ADL_GetTemperature2(int iAdapterIndex, out int lpTemperature);
  [DllImport("atiadlxx.dll", CallingConvention=CallingConvention.StdCall, EntryPoint="ADL_GetTemperature")]
  public static extern int ADL_GetTemperature4(int iAdapterIndex, out int lpTemperature, int flag, int unused);
  [DllImport("atiadlxx.dll", CallingConvention=CallingConvention.StdCall)]
  public static extern int ADL_GLUtilization_Get(int iAdapterIndex, out int gpu, out int mem, out int eng);
}
'@
} catch {
  $result.note='ADD_TYPE_FAILED'
  $result | ConvertTo-Json -Depth 4 -Compress
  exit 0
}
$create=[DiskPilot.ADL]::ADL_Main_Control_Create([DiskPilot.ADL+ADL_Main_Memory_Alloc][DiskPilot.ADL]::Alloc, 0)
if($create -ne 0) {
  $result.init_rc=[int]$create
  $result.note='CREATE_FAILED'
  $result | ConvertTo-Json -Depth 4 -Compress
  exit 0
}
$num=0
$rc=[DiskPilot.ADL]::ADL_Adapter_NumberOfAdapters_Get([ref]$num)
if($rc -ne 0 -or $num -le 0) {
  $result.init_rc=[int]$rc
  $result.note='ENUM_FAILED'
  $result | ConvertTo-Json -Depth 4 -Compress
  exit 0
}
$list=@()
for($i=0; $i -lt $num; $i++) {
  $o=[ordered]@{ index=[int]$i }
  $temp=0; $trc=-1
  $trc=[DiskPilot.ADL]::ADL_GetTemperature2($i,[ref]$temp)
  if($trc -ne 0) { $trc=[DiskPilot.ADL]::ADL_GetTemperature4($i,[ref]$temp,0,0) }
  if($trc -eq 0 -and $temp -ne 0) {
    if($temp -gt 300) { $o.temp_c=[int][Math]::Round($temp/1000.0) } else { $o.temp_c=[int]$temp }
  } else { $o.temp_c=$null }
  $gpu=0; $mem=0; $eng=0
  $urc=[DiskPilot.ADL]::ADL_GLUtilization_Get($i,[ref]$gpu,[ref]$mem,[ref]$eng)
  if($urc -eq 0) { $o.gpu_util=[int]$gpu; $o.mem_util=[int]$mem; $o.engine_util=[int]$eng } else { $o.gpu_util=$null; $o.mem_util=$null; $o.engine_util=$null }
  $list += $o
}
$result.init_rc=[int]$create
$result.note='OK'
$result.adapters=$list
$result | ConvertTo-Json -Depth 4 -Compress
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn escape_ps_single_quoted_escapes_quote() {
        assert_eq!(escape_ps_single_quoted("plain"), "plain");
        assert_eq!(escape_ps_single_quoted("o'brien"), "o''brien");
        // 恶意构造：逃出单引号字面量并注入命令；翻倍后整串仍在字面量内，
        // 每个原引号都翻成 `''` 转义对（引号数 = 原数量 ×2），无法闭合。
        let evil = "x'; Start-Process calc; '";
        let esc = escape_ps_single_quoted(evil);
        assert_eq!(esc, "x''; Start-Process calc; ''");
        assert_eq!(esc.matches('\'').count(), evil.matches('\'').count() * 2);
        let disabled = format!("{}.disabled", evil);
        assert_eq!(
            escape_ps_single_quoted(&disabled),
            "x''; Start-Process calc; ''.disabled"
        );
    }

    #[test]
    fn json_array_of_wraps_scalar() {
        assert_eq!(json_array_of(""), "[]");
        assert_eq!(json_array_of("{}"), "[{}]");
        assert_eq!(json_array_of("[1,2]"), "[1,2]");
        assert_eq!(json_array_of("  [3]  "), "[3]");
    }

    // ── 执行器真跑 PowerShell（Windows）──────────────────────────────
    // ps_capture 是全部 hw/net/sys 采集的地基：超时强杀、退出码语义、
    // UTF-8 首行注入（中文系统 GBK 乱码根治）此前零测试。

    #[cfg(windows)]
    #[test]
    fn ps_capture_returns_stdout_and_injects_utf8_first_line() {
        let out = ps_capture("Write-Output 'hello-diskpilot'").expect("应成功");
        assert!(out.contains("hello-diskpilot"), "got: {out}");
    }

    /// 中文 Windows 回归：控制台默认 GBK，首行强制 UTF8 编码后中文必须
    /// 无损返回（此前 desktop/agent-server 各自维护、一边没修好的漂移源头）。
    #[cfg(windows)]
    #[test]
    fn ps_capture_chinese_output_is_not_mojibake() {
        let out = ps_capture("Write-Output '中文测试✓'").expect("应成功");
        assert!(out.contains("中文测试"), "UTF-8 首行注入失效？got: {out}");
    }

    /// 超时强杀语义：挂起脚本必须在超时后**快速返回 Err**（不挂死调用方）。
    #[cfg(windows)]
    #[test]
    fn ps_capture_timeout_kills_hanging_script() {
        let t0 = std::time::Instant::now();
        let r = ps_capture_timeout("Start-Sleep 30", Duration::from_secs(1));
        let elapsed = t0.elapsed();
        let err = r.expect_err("挂起脚本必须超时报错");
        assert!(err.contains("超时"), "错误文案应说明超时，got: {err}");
        assert!(
            elapsed < Duration::from_secs(8),
            "强杀应在超时后快速返回，实际 {elapsed:?}"
        );
    }

    /// 退出码语义：非零退出且无 stdout → Err；非零退出但有 stdout → 保留
    /// 输出（采集脚本常见「部分成功 + 末尾非零」，不能整段丢弃）。
    #[cfg(windows)]
    #[test]
    fn ps_capture_exit_code_semantics() {
        let e = ps_capture("exit 3").expect_err("无输出非零退出应报错");
        assert!(e.contains("退出码"), "got: {e}");
        let ok = ps_capture("Write-Output 'partial-out'; exit 3").expect("有输出应保留");
        assert!(ok.contains("partial-out"), "got: {ok}");
    }

    /// 注入防御端到端：恶意串过 escape 后放进单引号字面量，必须**原样保留**
    /// 在字面量内、绝不作为命令执行。判据用副作用探针（注入若执行会创建临时
    /// 文件）——字面量文本本身含标记词时 contains 无法区分「保留」与「执行」。
    #[cfg(windows)]
    #[test]
    fn escaped_literal_cannot_break_out_at_executor_level() {
        let probe =
            std::env::temp_dir().join(format!("dp-inject-probe-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&probe);
        let evil = format!(
            "x'; New-Item -Path '{}' -Force | Out-Null; '",
            probe.display()
        );
        let script = format!("Write-Output '{}'", escape_ps_single_quoted(&evil));
        let out = ps_capture(&script).expect("应成功");
        assert!(!probe.exists(), "注入命令被执行了：探针文件被创建");
        assert!(out.contains(&evil), "字面量应原样保留，got: {out}");
        let _ = std::fs::remove_file(&probe);
    }

    /// 非 Windows 降级：不 spawn、不 panic，返回明确错误。
    #[cfg(not(windows))]
    #[test]
    fn ps_capture_non_windows_returns_clear_error() {
        let e = ps_capture("whatever").expect_err("非 Windows 必须报错");
        assert!(e.contains("不是 Windows"), "got: {e}");
    }
}
