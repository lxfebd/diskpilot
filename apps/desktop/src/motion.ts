/**
 * R357：跟随系统「减少动态效果」。
 * CSS 层有全局兜底（tokens.css 的 prefers-reduced-motion 块，动画/过渡/滚动收敛），
 * JS 里显式请求的平滑滚动也要自觉——否则开着 reduce 的用户仍会被 smooth 拖走。
 * 测试环境（无 window）与 matchMedia 缺失时一律按「不减少」处理（保守、不破坏现有行为）。
 */
export function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false;
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/** R357：调用方想要 smooth 时，系统开了减少动态效果就退回 auto（瞬间到位）。 */
export function scrollBehavior(preferred: 'smooth' | 'auto' = 'smooth'): ScrollBehavior {
  return prefersReducedMotion() ? 'auto' : preferred;
}