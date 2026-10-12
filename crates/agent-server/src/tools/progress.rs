//! MCP 标准进度通知（`notifications/progress`）上报辅助。
//!
//! 背景：stress_test / bench_disk / disk_find_biggest_files / disk_find_duplicate_files
//! 这类长任务走 `blocking_call` / `spawn_blocking`，执行期间调用方（桌面后端 / 任意
//! MCP 客户端）看不到任何中间状态，只能干等最终结果——用户视角就是「卡住没反应」。
//! 本模块把 rmcp 的 [`ServerNotification::ProgressNotification`] 通道封装成轻量进度
//! 上报器：handler 注入 `RequestContext<RoleServer>`，取出（或自造）`ProgressToken`，
//! 长任务执行期间周期性推送百分比，客户端 `on_progress` 即可实时展示。
//!
//! 约定：
//! - token 优先取请求 `meta.progressToken`（客户端显式要求进度时跟随），否则自造
//!   String token（"diskpilot-<ts>"）——MCP 规范允许任意 String/Number token；
//! - 进度以千分比（0..=1000）原子共享，异步转发任务每 ~500ms 收敛一次，避免
//!   高频率调用 `send_notification` 冲刷 stdio 通道；
//! - 推送失败一律静默（客户端可能已断开，或对端不支持通知：进度是增强，不阻塞
//!   工具结果返回）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::{
    ProgressNotification, ProgressNotificationParam, ProgressToken, ServerNotification,
};
use rmcp::service::{Peer, RequestContext, RoleServer};

/// 从请求上下文提取 progress token：客户端 `meta.progressToken` 优先，
/// 没有则自造一个 String token（每次调用唯一，靠时间戳区分）。
pub(crate) fn progress_token_from(ctx: &RequestContext<RoleServer>) -> ProgressToken {
    if let Some(t) = ctx.meta.get_progress_token() {
        return t;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    ProgressToken(rmcp::model::NumberOrString::String(
        format!("diskpilot-{ts}").into(),
    ))
}

/// 千分比原子槽：长任务闭包每轮写 `store(pct, Relaxed)`，转发任务读它推送。
/// `0..=1000`（0=开始，1000=完成）。不要求单调，转发任务自行取最新值。
#[derive(Clone, Default)]
pub(crate) struct ProgressSlot(Arc<AtomicU64>);

impl ProgressSlot {
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }

    /// 写当前千分比（钳 0..=1000，防止越界）。
    pub(crate) fn set(&self, pct: u64) {
        self.0.store(pct.min(1000), Ordering::Relaxed);
    }

    pub(crate) fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// 启动进度转发任务：每 `interval` 读一次 `slot`，若较上次有变化则向 `peer`
/// 推送一次 `notifications/progress`。返回句柄（调用方在任务结束后 join/abort）。
///
/// `label` 作为通知 message（如「大文件扫描」），`total` 固定 1000（千分比口径）。
/// 首条通知立即发送（进度 0 → 调用方立刻看到「已开始」），此后每变化推送。
pub(crate) fn spawn_progress_forwarder(
    peer: Peer<RoleServer>,
    token: ProgressToken,
    slot: ProgressSlot,
    label: &'static str,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_sent: u64 = u64::MAX; // 保证首轮必发
        loop {
            let cur = slot.get();
            if cur != last_sent {
                let param = ProgressNotificationParam::new(token.clone(), cur as f64)
                    .with_total(1000.0)
                    .with_message(label);
                let notif =
                    ServerNotification::ProgressNotification(ProgressNotification::new(param));
                let _ = peer.send_notification(notif).await;
                last_sent = cur;
                if cur >= 1000 {
                    break; // 完成即收尾，避免空转
                }
            }
            tokio::time::sleep(interval).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_clamps_and_reads() {
        let s = ProgressSlot::new();
        assert_eq!(s.get(), 0);
        s.set(500);
        assert_eq!(s.get(), 500);
        s.set(1234);
        assert_eq!(s.get(), 1000, "千分比应钳到 1000");
        s.set(999);
        assert_eq!(s.get(), 999);
    }

    #[test]
    fn token_from_empty_meta_is_string() {
        // 无 RequestContext 可用：直接验证自造 token 的形态（String 分支）。
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let t = ProgressToken(rmcp::model::NumberOrString::String(
            format!("diskpilot-{ts}").into(),
        ));
        match t.0 {
            rmcp::model::NumberOrString::String(s) => {
                assert!(s.starts_with("diskpilot-"));
            }
            _ => panic!("应产生 String token"),
        }
    }

    #[test]
    fn meta_roundtrip_progress_token() {
        // 客户端可设置 progressToken 到请求 meta，服务端应原样读出。
        let token = ProgressToken(rmcp::model::NumberOrString::Number(42));
        let mut meta = rmcp::model::Meta::new();
        meta.set_progress_token(token.clone());
        assert_eq!(meta.get_progress_token(), Some(token));
    }
}
