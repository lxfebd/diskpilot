// R141：Esc 关窗的统一实现——此前 ContextMenu / SteamWorkshopModal /
// DryRunPreviewDialog / gpu-donut 各自手写一份 keydown 判断（4 处重复，
// 且行为细节不一致：有的判 enabled、有的不判）。收敛到这里：
// 纯函数 isEscapeClose 可单测，hook 只管挂/摘监听。
import { useEffect, useRef } from 'react';

/** Esc 是否应该触发关闭：启用中 + 按键就是 Escape。纯函数，便于单测。 */
export function isEscapeClose(key: string, enabled: boolean): boolean {
  return enabled && key === 'Escape';
}

/**
 * 挂一个 window keydown 监听：按 Esc 时调用 onClose（且 preventDefault）。
 * - enabled=false 时不监听（弹窗未开就不该吃键）；
 * - onClose 用 ref 存最新值，effect 依赖只有 enabled——避免父组件每次
 *   渲染传新函数导致监听反复重挂。
 */
export function useEscapeKey(onClose: () => void, enabled: boolean = true): void {
  const ref = useRef(onClose);
  ref.current = onClose;
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (!isEscapeClose(e.key, enabled)) return;
      e.preventDefault();
      ref.current();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [enabled]);
}