// R406：聊天区要不要「跟着新消息滚到底」。
// 此前 ChatPanel 在 turns/chatBusy 变化时无条件 scrollTo(scrollHeight)——用户往上翻
// 历史时，一条新消息（或流式增量）会把他拽回底部，正在看的内容直接飞走。
// stickNearBottom 是纯函数：贴底（含橡皮筋式过冲）才继续跟随，往上翻过就别动。

export interface ScrollMetrics {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}

export function stickNearBottom(
  el: ScrollMetrics | null | undefined,
  thresholdPx = 80,
): boolean {
  if (!el) return true;
  const dist = el.scrollHeight - el.scrollTop - el.clientHeight;
  if (!Number.isFinite(dist)) return false;
  return dist <= thresholdPx;
}
