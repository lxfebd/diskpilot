//! R508：写操作一次性确认 nonce + 频率限制（防重放 / 防伪造 / 防滥发）。
//!
//! 威胁背景：`confirmed=true` 只是前端传参，webview 被注入脚本后可以直接
//! invoke 任意写命令。R505 加了后端 confirmed 校验，但参数本身仍可被伪造。
//! 本模块补上「**一次性、有时效、绑定操作、消费即失效**」的 nonce：
//!
//! - 前端确认窗点「确认」时先调 `issue_confirm_nonce(op)` 拿 nonce（60s 有效期），
//!   随写命令一起提交；后端 `consume` 校验并**当场消费**（同一 nonce 第二次
//!   使用直接拒绝 → 截获的 invoke 参数无法重放）。
//! - nonce 绑定 op，跨操作不能挪用。
//! - 频率限制：同一 op 1 秒内最多 1 次、60 秒窗口最多 10 次（防脚本狂刷）。
//! - 签发/消费/拒绝全部写审计日志（mcp_audit），可追溯。
//!
//! 诚实边界：实时 XSS 仍可自己申请 nonce（Tauri 同源 webview 无法区分），
//! nonce 防的是**重放**；真正的防线是 Tauri capability（命令白名单）+ 来源校验
//! （写命令要求窗口 label == "main"）+ 本层的频率限制与审计。

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// nonce 有效期：确认窗弹起到用户点确认一般几秒，60s 足够且不留太久。
pub(crate) const NONCE_TTL: Duration = Duration::from_secs(60);
/// nonce 表上限：超出时清掉最旧（正常不会到）。
const NONCE_TABLE_CAP: usize = 256;
/// 允许申请 nonce 的写操作白名单（与前端 withConfirmNonce 的 op 一一对应）。
pub(crate) const NONCE_OPS: &[&str] = &[
    "execute_scope",
    "recycle_paths",
    "undo",
    "install_scaffold",
    "uninstall_scaffold",
    "toolbelt_recycle",
    "toolbelt_run",
    // R509：插件写命令 + AI 清理计划全部纳入 nonce 闸
    "execute_ai_plan",
    "plugin_uninstall",
    "plugin_install",
    "plugin_install_url",
    "plugin_activate",
    "plugin_deactivate",
    "plugin_export",
    "plugin_registry_install",
    "plugin_registry_update",
    "plugin_registry_rollback",
    "plugin_set_registry_url",
    "plugin_registry_refresh",
    // R515：MCP 写工具统一走 agent_write 闸（11 个写工具共用一个 op，
    // 工具名记在审计日志里区分——见 agent.rs 的 agent_call_tool）。
    "agent_write",
];

pub(crate) struct NonceEntry {
    pub op: String,
    pub exp: Instant,
}

/// 确认闸门状态：nonce 表 + 每 op 的调用频率窗口。
pub(crate) struct ConfirmGuard {
    pub nonces: Mutex<HashMap<String, NonceEntry>>,
    pub rate: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl ConfirmGuard {
    pub fn new() -> Self {
        Self {
            nonces: Mutex::new(HashMap::new()),
            rate: Mutex::new(HashMap::new()),
        }
    }

    fn rand_hex() -> String {
        let mut b = [0u8; 32];
        if getrandom::getrandom(&mut b).is_err() {
            // getrandom 失败是系统级严重问题，fail-closed：返回空串让上层拒绝。
            return String::new();
        }
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// 签发一次性 nonce。op 不在白名单 / 随机源失败都拒绝。
    pub fn issue(&self, op: &str) -> Result<String, String> {
        if !NONCE_OPS.contains(&op) {
            return Err(format!("非法操作类型（不在写操作白名单）: {op}"));
        }
        let nonce = Self::rand_hex();
        if nonce.is_empty() {
            return Err("系统随机源不可用，拒绝签发确认 nonce".into());
        }
        let mut t = self.nonces.lock().unwrap();
        // 顺手清过期项 + 超容清最旧。
        let now = Instant::now();
        t.retain(|_, v| v.exp > now);
        if t.len() >= NONCE_TABLE_CAP {
            let oldest = t.iter().min_by_key(|(_, v)| v.exp).map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                t.remove(&k);
            }
        }
        t.insert(
            nonce.clone(),
            NonceEntry {
                op: op.to_string(),
                exp: now + NONCE_TTL,
            },
        );
        Ok(nonce)
    }

    /// 消费 nonce：存在 + op 匹配 + 未过期 → **移除**（一次性）。任一不符拒绝。
    pub fn consume(&self, op: &str, nonce: &str) -> Result<(), String> {
        let nonce = nonce.trim();
        if nonce.is_empty() {
            return Err("缺少确认 nonce（confirm_nonce）".into());
        }
        let mut t = self.nonces.lock().unwrap();
        let now = Instant::now();
        t.retain(|_, v| v.exp > now);
        // 先查后删：op 不匹配 / 已过期时**不消费** nonce（留给合法调用方）。
        let Some(e) = t.get(nonce) else {
            return Err("确认 nonce 无效或已被使用（防重放：一次性消费）".into());
        };
        if e.op != op {
            return Err(format!(
                "确认 nonce 操作类型不匹配（签发给 {}，实际 {}）",
                e.op, op
            ));
        }
        if e.exp <= now {
            return Err("确认 nonce 已过期（60s 有效期）".into());
        }
        t.remove(nonce);
        Ok(())
    }

    /// 频率限制：同一 op 1 秒内 ≤1 次、60 秒窗口 ≤10 次。通过才记录本次。
    pub fn hit_rate(&self, op: &str) -> Result<(), String> {
        let mut m = self.rate.lock().unwrap();
        let now = Instant::now();
        let q = m.entry(op.to_string()).or_default();
        q.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        if q.len() >= 10 {
            return Err(format!("操作 {op} 过于频繁（60 秒内上限 10 次），请稍后"));
        }
        if q.back()
            .is_some_and(|t| now.duration_since(*t) < Duration::from_secs(1))
        {
            return Err(format!("操作 {op} 过于频繁（1 秒内上限 1 次）"));
        }
        q.push_back(now);
        Ok(())
    }
}

/// 写操作总闸：dry_run 永远放行；真实写必须 confirmed==true **且** 一次性 nonce
/// 消费成功 **且** 频率限制通过。三者任一失败都拒绝落盘。
pub(crate) fn write_gate(
    guard: &ConfirmGuard,
    op: &str,
    dry_run: bool,
    confirmed: Option<bool>,
    nonce: Option<&str>,
) -> Result<(), String> {
    if dry_run {
        return Ok(());
    }
    if confirmed != Some(true) {
        return Err(format!(
            "{op}: 必须由用户明确确认（confirmed=true）后才能真实落盘。"
        ));
    }
    guard.consume(op, nonce.unwrap_or(""))?;
    guard.hit_rate(op)?;
    Ok(())
}

/// 校验调用来自主窗口（label == "main"）：拒绝其他 webview/窗口注入的 invoke。
pub(crate) fn require_main_window(window: &tauri::Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(format!(
            "拒绝来自非主窗口的调用（label={}）",
            window.label()
        ))
    }
}

/// 签发 nonce 的 Tauri 命令：前端确认窗点「确认」时调用，拿到后随写命令提交。
#[tauri::command]
pub(crate) fn issue_confirm_nonce(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    window: tauri::Window,
    op: String,
) -> Result<String, String> {
    require_main_window(&window)?;
    let nonce = state.confirm_guard.issue(&op)?;
    crate::mcp::mcp_audit::append_audit(
        &app,
        serde_json::json!({
            "ts": crate::mcp::mcp_audit::audit_ts(),
            "op": "issue_confirm_nonce",
            "nonce_op": op,
            "ttl_secs": NONCE_TTL.as_secs(),
        }),
    );
    Ok(nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_and_consume_once() {
        let g = ConfirmGuard::new();
        let n = g.issue("undo").unwrap();
        assert!(g.consume("undo", &n).is_ok());
        // 重放拒绝
        assert!(g.consume("undo", &n).is_err());
    }

    #[test]
    fn op_mismatch_rejected() {
        let g = ConfirmGuard::new();
        let n = g.issue("undo").unwrap();
        assert!(g.consume("execute_scope", &n).is_err());
        // 消费失败不吞 nonce：原 op 仍可消费
        assert!(g.consume("undo", &n).is_ok());
    }

    #[test]
    fn all_whitelisted_ops_can_issue() {
        // 白名单里每个 op 都必须能签发并消费（R509 加满插件写命令后回归保护）。
        let g = ConfirmGuard::new();
        for op in NONCE_OPS {
            let n = g.issue(op).unwrap();
            assert!(g.consume(op, &n).is_ok(), "op {op} 应可消费");
        }
    }

    #[test]
    fn unknown_op_and_empty_rejected() {
        let g = ConfirmGuard::new();
        assert!(g.issue("rm_rf").is_err());
        assert!(g.issue("").is_err());
        assert!(g.consume("undo", "").is_err());
        assert!(g.consume("undo", "deadbeef").is_err());
    }

    #[test]
    fn rate_limit_1s_and_60s_window() {
        let g = ConfirmGuard::new();
        assert!(g.hit_rate("undo").is_ok());
        // 1 秒内第二次拒绝
        assert!(g.hit_rate("undo").is_err());
        // 不同 op 独立窗口
        assert!(g.hit_rate("recycle_paths").is_ok());
    }

    #[test]
    fn write_gate_requires_confirmed_and_nonce() {
        let g = ConfirmGuard::new();
        // dry_run 永远放行
        assert!(write_gate(&g, "undo", true, None, None).is_ok());
        // 真实写：缺 confirmed 拒绝（nonce 不被消费）
        let n = g.issue("undo").unwrap();
        assert!(write_gate(&g, "undo", false, None, Some(&n)).is_err());
        assert!(write_gate(&g, "undo", false, Some(true), Some(&n)).is_ok());
        // 重放同一 nonce → 拒绝（consume 在 hit_rate 之前，不会误伤频率窗口）
        assert!(write_gate(&g, "undo", false, Some(true), Some(&n)).is_err());
        // 换一个 op（避开 1 秒频率限制），新 nonce 正常消费
        let n2 = g.issue("recycle_paths").unwrap();
        assert!(write_gate(&g, "recycle_paths", false, Some(true), Some(&n2)).is_ok());
        assert!(write_gate(&g, "recycle_paths", false, Some(true), Some(&n2)).is_err());
    }

    #[test]
    fn write_gate_missing_nonce_rejected() {
        let g = ConfirmGuard::new();
        assert!(write_gate(&g, "execute_scope", false, Some(true), None).is_err());
    }
}
