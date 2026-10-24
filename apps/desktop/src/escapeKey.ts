// R141：Esc 关窗的统一实现——此前 ContextMenu / SteamWorkshopModal /
// DryRunPreviewDialog / gpu-donut 各自手写一份 keydown 判断（4 处重复，
// 且行为细节不一致：有的判 enabled、有的不判）。收敛到这里：
// 纯函数 isEscapeClose 可单测，hook 只管挂/摘监听。
//
// R349：叠层时 Esc 只关最上面那一层。此前每个弹窗各挂一个 window keydown，
// 「工具箱详情上再弹导出确认」「跑分确认框叠在详情上」这类叠层按一次 Esc 会把
// 父子两层一起关掉（R320 因此没敢给设置窗补 Esc——它内部还有子弹窗）。现在改成
// 一个模块级栈：后挂上来的（更深的）先响应，只有栈顶被调用；栈空才摘 window 监听。
import { useEffect, useRef } from 'react';

/** Esc 是否应该触发关闭：启用中 + 按键就是 Escape。纯函数，便于单测。 */
export function isEscapeClose(key: string, enabled: boolean): boolean {
  return enabled && key === 'Escape';
}

/** 纯函数：Esc 只交给栈顶（最后挂上来的那一层）。空栈 → null。 */
export function pickTop<T>(stack: readonly T[]): T | null {
  return stack.length > 0 ? stack[stack.length - 1] : null;
}

// 每一层一个独立闭包入栈；退栈按引用定位，同一组件两次挂载互不干扰。
const stack: Array<() => void> = [];
let listening = false;

function onWindowKey(e: KeyboardEvent): void {
  if (!isEscapeClose(e.key, true)) return;
  const top = escapeStackTop();
  if (!top) return;
  e.preventDefault();
  top();
}

// 三个栈操作导出出来（不碰 window），便于在不依赖 DOM 的单测里验证「谁先响应」。
export function pushEscapeLayer(fn: () => void): void {
  stack.push(fn);
}

export function removeEscapeLayer(fn: () => void): void {
  const i = stack.lastIndexOf(fn);
  if (i >= 0) stack.splice(i, 1);
}

export function escapeStackDepth(): number {
  return stack.length;
}

/** 当前该响应 Esc 的那一层（栈顶）；没有弹窗时为 null。 */
export function escapeStackTop(): (() => void) | null {
  return pickTop(stack);
}

function pushEscape(fn: () => void): () => void {
  if (!listening) {
    window.addEventListener('keydown', onWindowKey);
    listening = true;
  }
  pushEscapeLayer(fn);
  return () => {
    removeEscapeLayer(fn);
    if (stack.length === 0 && listening) {
      window.removeEventListener('keydown', onWindowKey);
      listening = false;
    }
  };
}

/**
 * 挂一个 window keydown：按 Esc 时调用 onClose（且 preventDefault）。
 * - enabled=false 时不监听（弹窗未开就不该吃键）；
 * - onClose 用 ref 存最新值，effect 依赖只有 enabled——避免父组件每次
 *   渲染传新函数导致监听反复重挂；
 * - 同一时刻多个弹窗开着时，只有「最后挂上来」的那一层收到 Esc（见文件头 R349）。
 */
export function useEscapeKey(onClose: () => void, enabled: boolean = true): void {
  const ref = useRef(onClose);
  ref.current = onClose;
  useEffect(() => {
    if (!enabled) return;
    return pushEscape(() => ref.current());
  }, [enabled]);
}
