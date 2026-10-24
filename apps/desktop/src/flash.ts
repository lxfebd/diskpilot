// R358：把「提示文案一闪而过」的定时器收成一份。
//
// 四处（McpServers / PowerPlanCard / StartupPanel / SystemTools）此前各写
//   setMsg(s); setTimeout(() => setMsg(null), 3500);
// 两个问题：
//   ① 连续两次操作时，第一条的定时器到点会把第二条的文案提前抹掉
//      （第二条本该显示 3.5 秒，实际只剩「3.5 秒 - 两次间隔」）；
//   ② 组件卸载后定时器仍在跑，回调对着已卸载的组件 setState。
// FlashTimer 每次 arm 先清掉上一个 → 只保留最新一条的计时；卸载时 clear。
import { useCallback, useEffect, useRef, useState } from 'react';

/** 提示文案默认停留时长（毫秒）。 */
export const FLASH_MS = 3500;

/** 只保留最新一次的「稍后清空」定时器。 */
export class FlashTimer {
  private handle: ReturnType<typeof setTimeout> | null = null;

  constructor(
    private readonly onClear: () => void,
    private readonly ms: number = FLASH_MS,
  ) {}

  /** 重新计时：先清掉上一轮，再排一个新的。 */
  arm(): void {
    this.clear();
    this.handle = setTimeout(() => {
      this.handle = null;
      this.onClear();
    }, this.ms);
  }

  /** 取消未到点的清空（幂等）。 */
  clear(): void {
    if (this.handle === null) return;
    clearTimeout(this.handle);
    this.handle = null;
  }

  /** 是否还有待触发的清空。 */
  get pending(): boolean {
    return this.handle !== null;
  }
}

/**
 * 一条「闪一下就消失」的提示文案。
 * 返回 [文案 | null, 显示函数]；显示函数传 null 表示立刻清空（并取消计时）。
 */
export function useFlashText<T = string>(ms: number = FLASH_MS): [T | null, (value: T | null) => void] {
  const [value, setValue] = useState<T | null>(null);
  const timer = useRef<FlashTimer | null>(null);
  if (timer.current === null) timer.current = new FlashTimer(() => setValue(null), ms);
  useEffect(() => () => timer.current?.clear(), []);
  const show = useCallback((next: T | null) => {
    setValue(next);
    if (next === null) timer.current?.clear();
    else timer.current?.arm();
  }, []);
  return [value, show];
}