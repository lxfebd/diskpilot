//! 安全信息只读采集：Windows 事件日志、防火墙规则、登录/PowerShell 审计。
//!
//! 纯工具文件，不接路由（由 `tools/mod.rs` 统一接 MCP tool 薄壳）。
//! **全部只读**：不改事件日志、不改防火墙规则、不修改注册表/策略、不删用户文件；
//! 只输出 Windows 事件日志消息文本与防火墙规则的元数据（DisplayName/Port/Protocol/Address）。
//! 全部走 PowerShell（`Get-WinEvent` / `Get-NetFirewallRule` / `Get-NetFirewallPortFilter`
//! / `Get-NetFirewallAddressFilter`）；`ps_capture` 已注入 UTF-8 编码（根治中文乱码，
//! GBK 兜底仍在），本文件负责解析与中文格式化。
//!
//! ## 本文件导出函数清单（供 `tools/mod.rs` 对接）
//! - `pub fn collect_event_logs(logs: Option<String>, hours: u32, max_events: usize) -> Result<String, String>`
//!   —— 事件日志采集（默认 System,Application；Level 2/3 = 错误+警告）。
//! - `pub fn collect_firewall_rules(direction: Option<String>, action: Option<String>, top_n: usize) -> Result<String, String>`
//!   —— 防火墙规则采集（默认 Inbound+Allow 高危组合；联查 PortFilter + AddressFilter）。
//! - `pub fn collect_login_events(max_events: usize) -> Result<String, String>`
//!   —— 登录审计：Security 日志 4624/4625 降级 + PowerShell 脚本块日志 4104 尝试读取。
//!
//! ## 已真机验证的坑（照抄不重新发明）
//! - **Security 日志普通权限不可读**（真机验证）：`Get-WinEvent -LogName Security`
//!   抛 `UnauthorizedAccessException`，`Get-EventLog -List` 也拒。**必须降级**为
//!   `- [Security] 需要管理员权限，无法读取`，绝不整体失败。System / Application 可读。
//! - **`Get-WinEvent.Message` 极长**（单条可占满 AI 上下文）：一律截断 280 字符（登录
//!   脚本块 200 字符），并在截断后附加 `（共 N 字）`；用 `str::chars().take(n)` 走
//!   UTF-8 字符边界（中文/emoji 不会切成半个字）。
//! - **防火墙规则枚举只有 PowerShell**：`Get-NetFirewallRule` 有 889 条（真机量级），
//!   `netsh advfirewall` 输出中文乱码**禁用**。规则只有元数据，端口/地址要联查
//!   `Get-NetFirewallPortFilter`（本地端口/协议）与 `Get-NetFirewallAddressFilter`
//!   （远程地址）。默认只列 Inbound+Allow（高危入站放行），`top_n` 限流。
//! - **无 windows-sys 原生事件日志 API**：`EvtQuery` 未在 windows-sys 0.59 导出，
//!   一律走 PS。
//! - **`ConvertTo-Json` 单元素数组折叠成对象**：本文件所有 PS 模板输出都是 `[ordered]@{ ... }`
//!   对象（非数组），Rust 端直接 `serde_json::from_str::<Value>` 解析，无需 `json_array_of`
//!   兜底。子数组 `Events` / `Rules` / `SecEvents` / `PsEvents` 在 PS 侧用 `@(...)` 强制为数组
//!   后 JSON 序列化不会折叠。
//! - **`Get-WinEvent -FilterHashtable` 权限陷阱**（真机验证）：`-FilterHashtable @{LogName='Security'}`
//!   不抛 `UnauthorizedAccessException`，只返回空（server-side 过滤绕过权限检查）；
//!   必须先用 `-LogName <name> -MaxEvents 1` 做权限探测（会抛 UnauthorizedAccess），
//!   探测通过再跑 FilterHashtable。
//! - **参数注入防护**：日志名走白名单正则（`[A-Za-z._-]`），方向/动作走白名单枚举
//!   （Inbound/Outbound/Any、Allow/Block/Any），其它一律拒绝。`Get-WinEvent -LogName`
//!   本身不接受文件路径（只会命中系统注册的日志），但输入仍做白名单。
//! - **rustc 1.98 拒绝 `{:.1f}`**：本文件全部浮点走 `{:.*}`（当前无浮点格式，保留约束）。
//!
//! ## 隐私红线
//! - 只输出事件日志消息文本（系统日志）与防火墙规则元数据；**绝不读用户个人文件/Temp**。
//! - 不改事件日志配置（`Clear-EventLog` / `Wevtutil` / `Set-LogProperties` 一律禁用）。
//! - 不改防火墙规则（`Set-NetFirewallRule` / `New-NetFirewallRule` / `Enable-NetFirewall`
//!   一律禁用）。

#[cfg(not(windows))]
#[allow(dead_code)] // 跨平台降级文案：Windows 构建不引用
const NOT_WINDOWS: &str = "当前平台不是 Windows，安全信息不可用";

mod sec_common;
mod sec_defender;
mod sec_eventlog;
mod sec_firewall;
mod sec_login;
mod sec_users;

pub use sec_defender::collect_defender_status;
pub use sec_eventlog::collect_event_logs;
pub use sec_firewall::collect_firewall_rules;
pub use sec_login::collect_login_events;
pub use sec_users::collect_user_accounts;

// 回归测试引用的私有符号：sid_tail 仅 Windows 实现，对应测试 fn 带 cfg(windows)。
#[cfg(all(test, windows))]
pub(crate) use sec_users::sid_tail;

// 2026-10-12 拆分：通用小工具 → `sec_common`，事件日志 → `sec_eventlog`，
// 防火墙 → `sec_firewall`，登录/PowerShell 审计 → `sec_login`，
// Defender 状态 → `sec_defender`，用户账户 → `sec_users`。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)] // sid_tail 是 Windows-only 辅助函数
    fn sid_tail_extracts_rid() {
        // S-1-5-21-1234567890-1234567890-1234567890-1001 → RID 1001
        assert_eq!(
            sid_tail("S-1-5-21-1234567890-1234567890-1234567890-1001"),
            "RID 1001"
        );
        // 短 SID → 尾段即最后的段
        assert_eq!(sid_tail("S-1-5"), "RID 5");
        // 空字符串 → 原样返回
        assert_eq!(sid_tail(""), "SID ");
    }

    #[test]
    fn defender_status_non_windows_degrade() {
        let s = collect_defender_status();
        assert!(s.is_ok());
    }

    #[test]
    fn user_accounts_non_windows_degrade() {
        let s = collect_user_accounts();
        assert!(s.is_ok());
    }
}
