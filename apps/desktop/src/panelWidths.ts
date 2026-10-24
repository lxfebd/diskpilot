// R408：AI 侧栏宽度夹取。
// 跨会话记住的 aiWidth 在窗口变窄后不能挤掉中栏 MIN_CENTER 保底：
// 旧代码初始化只判 Number.isFinite 且 >=260，没有上界——大屏记住的宽值
// 在窄窗上会把工作台挤爆（fit 兜底只收缩左/右栏，从不收 AI 侧栏）。
export const AI_WIDTH_MIN = 260;
export const AI_WIDTH_DEFAULT = 300;
// 主内容区至少留这么多像素（含最小左栏 320 + 中栏 360 的预算内），
// 与 dragAi 的 480 同口径——上界 = 窗口宽 - 侧栏占位 - 480。
export const AI_MAX_BUDGET = 480;

export function clampAiWidth(v: number, winW: number, sidebarFootprint: number): number {
  if (!Number.isFinite(v)) return AI_WIDTH_DEFAULT;
  const maxAi = Math.max(AI_WIDTH_MIN, winW - sidebarFootprint - AI_MAX_BUDGET);
  return Math.max(AI_WIDTH_MIN, Math.min(maxAi, v));
}
