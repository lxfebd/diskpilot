import { useEffect, useRef } from 'react';

// R344：弹窗关闭后把焦点还给「打开它的那个元素」。
// 此前关掉设置窗/确认框后焦点落到 body：键盘用户要从页面开头重新 Tab 一遍，读屏软件
// 也从头念。这里在弹窗打开那一刻记下 document.activeElement，关闭（或卸载）时 focus() 回来；
// 元素若已被移除就静默跳过——宁可不还，也不要编造一个落点。

// 纯函数：只接受真正能接收焦点的对象（有 focus 方法），其余一律 null。
export function focusReturnTarget(el: unknown): HTMLElement | null {
  if (!el || typeof el !== 'object') return null;
  const candidate = el as { focus?: unknown };
  return typeof candidate.focus === 'function' ? (el as HTMLElement) : null;
}

// 说明：只有「自己是一个组件、且随手写 `{open && <Dialog/>}` 挂载」的弹窗才用得上；
// 内联在父组件里的弹窗（工具箱、跑分、插件市场）没有独立的 mount 时机，留给后续抽出
// 统一弹窗组件时再一并接。
//
// open 由 false 变 true 时记录，变回 false（或组件卸载）时归还。
// 注意：必须在弹窗自己的「自动聚焦」effect 之前调用，否则记住的会是弹窗内部元素。
export function useReturnFocus(open: boolean): void {
  const prev = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (!open) return;
    prev.current = focusReturnTarget(typeof document === 'undefined' ? null : document.activeElement);
    return () => {
      prev.current?.focus?.();
      prev.current = null;
    };
  }, [open]);
}

// ── R347：Tab 不许跑出弹窗 ──
//
// R343 给 15 处遮罩写了 role=dialog / aria-modal="true"，等于向读屏软件承诺「外面不可交互」；
// 但浏览器不会替我们兑现——Tab 照旧会走到背景页的输入框和按钮上，读屏用户被带出对话框
// 还不知道自己在哪。真正兑现这句承诺要自己做 Tab 循环。

const FOCUSABLE_SEL = [
  'a[href]',
  'area[href]',
  'button',
  'input',
  'select',
  'textarea',
  'iframe',
  '[tabindex]',
  '[contenteditable="true"]',
].join(', ');

// 弹窗选择器与 R343 落地的标记保持一致（通用 ConfirmDialog 用的是 alertdialog）。
export const MODAL_SEL = '[role="dialog"][aria-modal="true"], [role="alertdialog"][aria-modal="true"]';

// 纯函数：容器里「当前真的能按 Tab 到达」的元素，按 DOM 顺序。
// 只认元素自身的 disabled / aria-hidden / 负 tabindex：更深的隐藏（display:none 后代、
// 祖先 aria-hidden）这里不判——宁可在少数情况下少列一个，也不假装能判断可见性。
export function focusableWithin(root: ParentNode | null | undefined): HTMLElement[] {
  if (!root) return [];
  const all = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SEL));
  return all.filter((el) => {
    if (el.hasAttribute('disabled')) return false;
    if (el.getAttribute('aria-hidden') === 'true') return false;
    if (el.tabIndex < 0) return false;
    return true;
  });
}

// 纯函数：Tab / Shift+Tab 在 [0, count) 里绕圈。current 不在范围内（焦点在弹窗外或
// 根本没进过弹窗）时，正向给第一个、反向给最后一个——先把焦点拉回弹窗里。
export function trapTarget(current: number, count: number, shift: boolean): number {
  if (count <= 0) return -1;
  if (current < 0 || current >= count) return shift ? count - 1 : 0;
  return (current + (shift ? -1 : 1) + count) % count;
}

// 纯函数：处理一次 Tab 按键，返回是否已经接管（接管时自己 preventDefault）。
// 容器里一个可聚焦元素都没有时放手——那时强行拦下 Tab 会让人彻底按不动。
export function trapTabKey(
  root: ParentNode | null | undefined,
  e: { key: string; shiftKey?: boolean; preventDefault?: () => void },
  active: unknown = null,
): boolean {
  if (e.key !== 'Tab' || !root) return false;
  const list = focusableWithin(root);
  if (list.length === 0) return false;
  const cur = active ? list.indexOf(active as HTMLElement) : -1;
  const idx = trapTarget(cur, list.length, !!e.shiftKey);
  e.preventDefault?.();
  list[idx]?.focus?.();
  return true;
}

// 全局挂一次：任何 Tab 都交给「最上层那个 aria-modal 弹窗」处理。
// 最上层取 DOM 顺序里的最后一个（嵌套弹窗总在其父对话框之后渲染），不按 z-index 猜。
export function useModalFocusTrap(): void {
  useEffect(() => {
    if (typeof document === 'undefined') return;
    const onKey = (ev: KeyboardEvent): void => {
      if (ev.key !== 'Tab') return;
      const dialogs = document.querySelectorAll<HTMLElement>(MODAL_SEL);
      const top = dialogs[dialogs.length - 1];
      if (!top) return;
      trapTabKey(top, ev, document.activeElement);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}
