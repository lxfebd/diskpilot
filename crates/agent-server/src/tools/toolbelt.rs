//! 工具箱（toolbelt）接入 AI：让图吧工具箱 30 个工具被内置 AI 直接操作。
//!
//! - `toolbelt_list`（L0 只读）：列全部 Tool Manifest（名称/分类/CLI|GUI/权限级/用途/风险），
//!   AI 判断「该用哪个工具」。
//! - `toolbelt_run`（L2 写）：按工具名执行 CLI 工具。三层防线与主项目 toolbelt_run 同款：
//!   L3（BIOS 刷写/格式化等）永久拒绝；medium/high 风险必须 confirmed=true；
//!   cli.run 权限未开启直接拒绝；默认 30s 超时。
//!
//! 依赖 `diskpilot-toolbelt` crate 纯逻辑（manifest / 命令构建 / 执行），与主项目共用同一套
//! 安全门（toolbelt.toml 覆盖 + manifest 风险 + L3 永禁），agent-server 独立进程可跑。

use diskpilot_toolbelt::{
    all_manifests, find_manifest, find_tools_root, run, toolbelt_command_for, Risk, ToolMode,
};

/// 列全部工具 manifest（只读，L0）。
pub fn list_tools() -> String {
    let manifests = all_manifests();
    if manifests.is_empty() {
        return "工具箱清单为空（tool_manifests.json 未内嵌？）。".into();
    }
    let mut lines = Vec::with_capacity(manifests.len() + 2);
    lines.push(format!(
        "图吧工具箱集成：{} 个工具（{} CLI 可 AI 操作 / {} GUI 仅启动）",
        manifests.len(),
        manifests
            .iter()
            .filter(|m| m.invocation.mode == diskpilot_toolbelt::ToolMode::Cli)
            .count(),
        manifests
            .iter()
            .filter(|m| m.invocation.mode == diskpilot_toolbelt::ToolMode::Gui)
            .count()
    ));
    lines.push(String::new());
    // 按分类分组
    let mut by_cat: std::collections::BTreeMap<&str, Vec<&diskpilot_toolbelt::ToolManifest>> =
        std::collections::BTreeMap::new();
    for m in manifests.iter() {
        by_cat.entry(&m.category).or_default().push(m);
    }
    for (cat, tools) in by_cat {
        lines.push(format!("【{cat}】"));
        for t in tools {
            let mode = match t.invocation.mode {
                diskpilot_toolbelt::ToolMode::Cli => "CLI",
                diskpilot_toolbelt::ToolMode::Gui => "GUI",
            };
            let risk = if t.risk.is_empty() { "-" } else { &t.risk };
            lines.push(format!(
                "  {name} [{mode} · 权限{perm} · 风险{risk}]：{purpose}",
                name = t.name,
                perm = t.permission_level,
                purpose = t.purpose
            ));
        }
        lines.push(String::new());
    }
    lines.push("提示：CLI 工具可用 toolbelt_run 执行（L3 永久禁止如 FPT64 刷 BIOS 除外；medium/high 风险需用户确认）。".into());
    lines.join("\n")
}

/// 执行一个工具箱 CLI 工具（L2 写，三层防线）。
///
/// - `tool`: 工具名（如 `crystaldiskinfo` / `wiztree`，与 manifest 一致）
/// - `args`: 参数列表（透传给工具，不拼 shell）
/// - `confirmed`: 用户已确认（medium/high 风险必须 true）
/// - `timeout_secs`: 超时（默认 30s）
pub fn run_tool(
    tool: &str,
    args: &[String],
    confirmed: bool,
    timeout_secs: Option<u64>,
) -> Result<String, String> {
    // ① manifest 级 L3 永禁（BIOS 刷写/格式化等），与主项目双保险
    if let Some(m) = find_manifest(tool) {
        if m.permission_level == "L3" {
            return Err(format!(
                "「{tool}」为 L3 永久禁止操作（BIOS 刷写/格式化等），不可执行。"
            ));
        }
    }
    // ② 定位工具根目录；找不到说明环境未装工具箱
    let root = find_tools_root(None).map_err(|e| format!("未找到工具箱目录：{e}"))?;
    // ③ 构建命令（manifest 参数模板 + toolbelt.toml 覆盖）
    let cmd = toolbelt_command_for(tool, args, &root, timeout_secs)
        .map_err(|e| format!("构建命令失败：{e}"))?;
    // ④ medium/high 风险必须用户确认
    if cmd.risk >= Risk::Medium && !confirmed {
        return Err(format!(
            "toolbelt:confirm: 「{}」为 {:?} 风险工具，需用户确认后执行（参数：{}）",
            cmd.tool,
            cmd.risk,
            cmd.args.join(" ")
        ));
    }
    let cmdline = format!("{} {}", cmd.program.display(), cmd.args.join(" "));
    let risk = cmd.risk;
    // GUI 工具（CPU-Z / CoreTemp / ThrottleStop 等）启动后不会自己退出，
    // 同步等待会挂死（用户可见「卡住不动」）。改为启动即返回，进程独立运行。
    if let Some(m) = find_manifest(tool) {
        if m.invocation.mode == ToolMode::Gui {
            let mut launch = std::process::Command::new(&cmd.program);
            launch.args(&cmd.args).current_dir(&cmd.cwd);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                launch.creation_flags(CREATE_NO_WINDOW);
            }
            match launch.spawn() {
                Ok(_) => {
                    return Ok(format!(
                        "命令：{cmdline}\n风险：{risk}\n✅ GUI 工具已启动（PID 独立运行，不等待退出——界面由你/用户在屏幕上操作）。"
                    ));
                }
                Err(e) => {
                    return Err(format!("启动 GUI 工具失败：{e}"));
                }
            }
        }
    }
    let outcome = run(&cmd);
    let mut out = String::new();
    out.push_str(&format!(
        "命令：{cmdline}\n风险：{risk} · 退出码：{}\n",
        outcome
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "超时/中断".into())
    ));
    if outcome.timed_out {
        out.push_str("⚠️ 执行超时（已强制终止）。\n");
    }
    if !outcome.stdout.trim().is_empty() {
        out.push_str(&format!("—— 输出 ——\n{}", outcome.stdout.trim()));
    }
    if !outcome.stderr.trim().is_empty() {
        out.push_str(&format!("\n—— 错误 ——\n{}", outcome.stderr.trim()));
    }
    if outcome.stdout.trim().is_empty() && outcome.stderr.trim().is_empty() {
        out.push_str("（无输出）");
    }
    Ok(out)
}
