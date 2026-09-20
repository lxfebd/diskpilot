// trace 计时工具：给「进行中的工具调用」显示已运行秒数，避免长工具
// （bench_disk / bench_memory / stress_test 等）跑数秒到数十秒期间无反馈
// 让人以为卡死。纯函数，便于单测；TraceBlock 组件引用它驱动 UI。
import type { TraceItem } from '../store';

/** 找 trace 里最后一条 tool_call（= 当前正在运行的工具调用入口）。 */
export function lastToolCall(trace: TraceItem[] | undefined | null): TraceItem | null {
  if (!trace) return null;
  for (let i = trace.length - 1; i >= 0; i--) {
    if (trace[i].kind === 'tool_call') return trace[i];
  }
  return null;
}

/** 工具已运行秒数：>=0；时钟异常（now < ts）按 0 处理。 */
export function runningSecs(ts: number, now: number): number {
  if (!(ts > 0) || !(now > 0)) return 0;
  const d = now - ts;
  return d > 0 ? Math.floor(d / 1000) : 0;
}