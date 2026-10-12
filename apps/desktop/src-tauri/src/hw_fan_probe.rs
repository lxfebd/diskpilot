//! 风扇通道探测与调速（从 hw_fan.rs 拆分，2026-10-12）。
//! WMI/CLI 多通道探测、调速写入、恢复、档位换算与进程级原转速快照/守护标志。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::hw::hw_core::{parse_fancmd_diag, ps_capture, run_cli_capture};

/// 风扇转速（WMI 支持有限，读不到为 null）。
pub(crate) const FAN_PS: &str = r#"$ErrorActionPreference='SilentlyContinue'
$out=[ordered]@{}
$fans=@()
Get-CimInstance -Namespace root/cimv2 -ClassName Win32_Fan -ErrorAction SilentlyContinue | ForEach-Object {
  $fans += [ordered]@{ name=[string]$_.Name; desired_rpm=$_.DesiredSpeed; actual_rpm=$null; status=[string]$_.Status; active=($_.ActiveCooling -eq $true) }
}
try {
  Get-CimInstance -Namespace root/wmi -ClassName mssctree_faninformation -ErrorAction SilentlyContinue | ForEach-Object {
    $fans += [ordered]@{ name=[string]$_.InstanceName; desired_rpm=$_.DesiredValue; actual_rpm=$null; status=''; active=$true }
  }
} catch {}
$out.fans=$fans
$out.thermal=@(Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue | Select-Object InstanceName,CurrentTemperature)
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 温度熔断阈值：应用自定义曲线后，后台守护采样到该温度即自动回退原转速。
/// 对齐 hw_run_test 的 temp_limit（默认 90℃），下限保护到 70℃。
pub(crate) const FAN_TEMP_BREAKER_C: f32 = 90.0;

/// 风扇应用后的温度守护采样周期。
pub(crate) const FAN_GUARD_POLL: Duration = Duration::from_secs(5);

/// 原转速快照 + 守护运行标志（进程级静态，与 THERMAL_CACHE 同款）。
static FAN_SNAPSHOT: OnceLock<Mutex<Option<u32>>> = OnceLock::new();
static FAN_GUARD_RUNNING: OnceLock<AtomicBool> = OnceLock::new();
pub(crate) fn fan_snapshot_lock() -> &'static Mutex<Option<u32>> {
    FAN_SNAPSHOT.get_or_init(|| Mutex::new(None))
}
pub(crate) fn fan_guard_running() -> bool {
    FAN_GUARD_RUNNING
        .get_or_init(|| AtomicBool::new(false))
        .load(Ordering::Relaxed)
}
pub(crate) fn fan_guard_set(v: bool) {
    FAN_GUARD_RUNNING
        .get_or_init(|| AtomicBool::new(false))
        .store(v, Ordering::Relaxed);
}

/// 内置 WMI 通道探测：Win32_Fan / mssctree_faninformation 只读属性。
/// 标准 WMI 类没有通用的「写风扇」接口——如实报告不可写，不伪装成可调。
fn wmi_fan_probe() -> serde_json::Value {
    let mut out = serde_json::json!({
        "channel": "wmi",
        "writable": false,
        "mode": "readonly",
        "reason": "WMI 标准类（Win32_Fan / mssctree_faninformation）只暴露只读属性（DesiredSpeed / DesiredValue），没有通用的写入接口。",
        "fans": []
    });
    if let Ok(raw) = ps_capture(FAN_PS, Duration::from_secs(20)) {
        let raw = raw.trim().trim_start_matches('\u{feff}');
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(fans) = v.get("fans").and_then(|x| x.as_array()) {
                out["fans"] = serde_json::json!(fans);
            }
        }
    }
    out
}

/// 厂商/第三方 CLI 外挂通道：在本机 PATH 与常见安装目录找可写调速 CLI。
/// 探测顺序：
///
/// 1. fancmd（DiskPilot 自带 ITE SuperIO 写控桥，tools/fancmd —— 真实可写，无需第三方软件）
/// 2. nbfc（NoteBook FanControl：`nbfc set -s <0-100>` 写转速）
///
/// 找不到就不编造，如实降级。
fn cli_fan_probe() -> serde_json::Value {
    // (显示名, exe, 子命令模板, 是否按空格拆分参数, 是否有独立 reset 子命令)
    let candidates: &[(&str, &str, &str, bool, bool)] = &[
        (
            "DiskPilot fancmd（ITE SuperIO 写控桥）",
            "fancmd.exe",
            "write 0 {speed}",
            true,
            true,
        ),
        (
            "NoteBook FanControl",
            "nbfc.exe",
            "set -s {speed}",
            false,
            false,
        ),
    ];
    for (name, exe, subcmd_tpl, arg_split, has_reset) in candidates {
        if let Some(exe_path) = find_cli_in_paths(exe) {
            // fancmd 额外跑 `diag json` 拿结构化诊断（缺驱动/芯片不可识别等，
            // 供内置 AI 自修复参考）。只读、超时兜底，失败不致命。
            let mut diagnostics = serde_json::json!({});
            let mut writable = true;
            if *exe == "fancmd.exe" {
                match run_cli_capture(&exe_path, &["diag", "json"], Duration::from_secs(10)) {
                    Ok(raw) => {
                        let d = parse_fancmd_diag(&raw);
                        diagnostics = d.clone();
                        // 驱动没加载 / 芯片没识别 / diag 输出没法解析（旧版 fancmd 不认
                        // `diag json`、或非管理员下驱动不可用输出人类可读文本）→
                        // fancmd 存在但写通道不可确证，如实降级，绝不拿假可写下结论。
                        if d.get("parsed").and_then(|x| x.as_bool()) == Some(false) {
                            writable = false;
                        }
                        if let Some(drv) = d.get("driver") {
                            if drv.get("loaded").and_then(|x| x.as_bool()) == Some(false) {
                                writable = false;
                            }
                        }
                        if let Some(sio) = d.get("sio") {
                            if sio.get("found").and_then(|x| x.as_bool()) == Some(false) {
                                writable = false;
                            }
                        }
                    }
                    // diag 命令本身执行失败（如旧版 exe 直接 usage 退出）→ 不可写。
                    Err(_) => writable = false,
                }
            }
            return serde_json::json!({
                "channel": "cli",
                "writable": writable,
                "mode": if writable { "cli" } else { "degraded" },
                "cli": {
                    "name": name,
                    "exe": exe,
                    "exe_path": exe_path,
                    "subcmd_tpl": subcmd_tpl,
                    "arg_split": arg_split,
                    "has_reset": has_reset,
                },
                "diagnostics": diagnostics,
                "reason": if writable {
                    format!("检测到可写调速 CLI：{name}（{exe_path}）。")
                } else {
                    format!(
                        "检测到 {name}（{exe_path}），但其自诊断显示当前不可写（缺驱动或 SuperIO 芯片不可识别）。驱动装在非标准路径时请让内置 AI 自动安装/加载端口驱动后重试。"
                    )
                },
                "fans": diagnostics.get("fans").cloned().unwrap_or(serde_json::json!([]))
            });
        }
    }
    serde_json::json!({
        "channel": "cli",
        "writable": false,
        "mode": "readonly",
        "reason": "未检测到可写调速 CLI（fancmd / NoteBook FanControl 均未找到）。如需落地调速，请先安装可写通道，或让内置 AI 自动安装 fancmd + inpoutx64 后重试。",
        "fans": []
    })
}

/// 在 PATH 和常见安装目录里找 exe 的完整路径（找到第一个即停）。
/// 额外探测两个本地路径：
///   - 开发环境：仓库内 tools/fancmd/bin/fancmd.exe（从当前 exe 相对上溯）
///   - 打包环境：resources/fancmd/fancmd.exe（与主程序同目录）
fn find_cli_in_paths(exe: &str) -> Option<String> {
    let is_file = |p: &std::path::Path| p.is_file();
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let full = dir.join(exe);
            if is_file(&full) {
                return Some(full.to_string_lossy().into_owned());
            }
        }
    }
    // 常见安装目录兜底（32/64 位 Program Files + LOCALAPPDATA）。
    let bases = [
        std::env::var("ProgramFiles").unwrap_or_default(),
        std::env::var("ProgramFiles(x86)").unwrap_or_default(),
        std::env::var("LOCALAPPDATA").unwrap_or_default(),
    ];
    for base in bases {
        if base.is_empty() {
            continue;
        }
        let candidates = [
            std::path::Path::new(&base)
                .join("NoteBook FanControl")
                .join(exe),
            std::path::Path::new(&base).join("nbfc").join(exe),
        ];
        for c in candidates {
            if is_file(&c) {
                return Some(c.to_string_lossy().into_owned());
            }
        }
    }
    // 开发环境：仓库 tools/fancmd/bin（从当前 exe 上溯到仓库根）。
    if let Ok(cur) = std::env::current_exe() {
        let mut probe = cur.clone();
        for _ in 0..6 {
            if !probe.pop() {
                break;
            }
            let fancmd = probe.join("tools").join("fancmd").join("bin").join(exe);
            if is_file(&fancmd) {
                return Some(fancmd.to_string_lossy().into_owned());
            }
            if probe.join("tools").join("fancmd").is_dir() {
                break;
            }
        }
    }
    // 打包环境：与主程序同目录的 resources/fancmd。
    if let Ok(cur) = std::env::current_exe() {
        if let Some(dir) = cur.parent() {
            let res = dir.join("resources").join("fancmd").join(exe);
            if is_file(&res) {
                return Some(res.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// 多通道探测汇总：内置 WMI（只读）+ 外挂 CLI（可写时写入）。
/// 返回 { wmi, cli, writable, best_channel, note }。
/// 注意：`cli` 字段**直接**是 CLI 通道内容（name/exe/exe_path/subcmd_tpl/…），
/// 不是 cli_fan_probe 的整块——fan_control / fan_curve_apply 把 `probe["cli"]`
/// 原样传给 cli_set_fan_speed / cli_restore_fan_speed，套一层会导致
/// 「调速 CLI 信息不完整（缺 exe_path / subcmd_tpl）」。
pub(crate) fn fan_probe_all() -> serde_json::Value {
    let wmi = wmi_fan_probe();
    let probe = cli_fan_probe();
    let writable = probe["writable"].as_bool().unwrap_or(false);
    let best_channel = if writable { "cli" } else { "wmi" };
    serde_json::json!({
        "wmi": wmi,
        "cli": if writable {
            // 可写：平铺通道内容（probe.cli 里层）。
            probe["cli"].clone()
        } else {
            // 只读/降级：cli 字段给个空对象，保持键存在；writable=false 已足够区分。
            serde_json::json!({})
        },
        "cli_probe": probe,
        "writable": writable,
        "best_channel": best_channel,
        "note": if writable {
            "检测到可写调速通道（CLI 外挂）。应用曲线会写入该通道。"
        } else {
            "当前无可写调速通道（WMI 标准类只读 + 未检测到调速 CLI）。只能给出建议，不能落地调速。"
        }
    })
}

/// 用 CLI 通道写入转速百分比（0-100）。写失败返回 Err。
/// nbfc 把子命令模板当单个参数；fancmd 需要按空格拆成多个参数（arg_split=true）。
pub(crate) fn cli_set_fan_speed(cli: &serde_json::Value, speed_pct: u32) -> Result<(), String> {
    let speed_pct = speed_pct.clamp(0, 100);
    let exe_path = cli["exe_path"].as_str().unwrap_or("").to_string();
    let tpl = cli["subcmd_tpl"].as_str().unwrap_or("").to_string();
    if exe_path.is_empty() || tpl.is_empty() {
        return Err("调速 CLI 信息不完整（缺 exe_path / subcmd_tpl）。".into());
    }
    let arg_split = cli["arg_split"].as_bool().unwrap_or(false);
    let sub = tpl.replace("{speed}", &speed_pct.to_string());
    let mut cmd = std::process::Command::new(&exe_path);
    if arg_split {
        for part in sub.split_whitespace() {
            cmd.arg(part);
        }
    } else {
        cmd.arg(&sub);
    }
    let result = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();
    match result {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format!(
            "调速 CLI 执行失败（退出码 {:?}）：{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => Err(format!("启动调速 CLI 失败：{e}（{exe_path}）")),
    }
}

/// 恢复主板自动控制（fancmd 走 `write reset <idx>`；nbfc 无 reset 走写 100%）。
pub(crate) fn cli_restore_fan_speed(cli: &serde_json::Value) -> Result<(), String> {
    let exe_path = cli["exe_path"].as_str().unwrap_or("").to_string();
    let tpl = cli["subcmd_tpl"].as_str().unwrap_or("").to_string();
    if exe_path.is_empty() || tpl.is_empty() {
        return Err("调速 CLI 信息不完整（缺 exe_path / subcmd_tpl）。".into());
    }
    if cli["has_reset"].as_bool().unwrap_or(false) {
        // fancmd: 提取 fan idx 然后 `write reset <idx>`。tpl 形如 "write 0 {speed}"。
        let fan_idx = tpl.split_whitespace().nth(1).unwrap_or("0").to_string();
        let result = std::process::Command::new(&exe_path)
            .args(["write", "reset", &fan_idx])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output();
        match result {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => Err(format!(
                "恢复调速 CLI 执行失败（退出码 {:?}）：{}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            )),
            Err(e) => Err(format!("启动恢复调速 CLI 失败：{e}（{exe_path}）")),
        }
    } else {
        // nbfc 没有独立 reset：写 100% 近似主板接管。
        cli_set_fan_speed(cli, 100)
    }
}

/// 从档位/曲线算目标转速百分比（纯函数，可单测）。
pub(crate) fn speed_pct_for(
    level: Option<&str>,
    curve: Option<&[serde_json::Value]>,
) -> Result<u32, String> {
    match level.unwrap_or("balanced") {
        "quiet" => Ok(35),
        "balanced" => Ok(55),
        "balanced_plus" => Ok(70),
        "high" => Ok(85),
        "urgent" => Ok(100),
        "custom" => {
            let anchors =
                curve.ok_or_else(|| "custom 档位需要提供 curve 锚点（非空）。".to_string())?;
            if anchors.is_empty() {
                return Err("custom 档位需要提供 curve 锚点（非空）。".into());
            }
            let mut sum = 0u64;
            let mut n = 0u64;
            for a in anchors {
                if let Some(p) = a["fan_pct"].as_u64() {
                    sum += p;
                    n += 1;
                }
            }
            if n == 0 {
                return Err("custom 档位的 curve 锚点缺少 fan_pct 字段。".into());
            }
            Ok(((sum / n) as u32).clamp(20, 100))
        }
        other => Err(format!(
            "未知档位：{other}。可用 quiet / balanced / balanced_plus / high / urgent / custom"
        )),
    }
}
