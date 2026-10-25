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

// ── R424：弹窗打开时把焦点送进去、关闭时归还 ──
// useReturnFocus 只负责「关闭归还」；内联在父组件里的弹窗（工具箱、跑分、插件市场、
// Steam 详情）打开后焦点还留在触发按钮上，键盘用户按 Tab 第一步就走出了弹窗，读屏
// 用户也不知道弹窗已经打开。useDialogFocus 在打开那一刻把焦点送到最上层 aria-modal
// 弹窗的可聚焦元素（与 useModalFocusTrap 同为「DOM 顺序最后一个」口径），
// 关闭时归还给 opener——进场与归还一体，内联/挂载式弹窗都适用。
// open 由 false 变 true 时进场，变回 false（或组件卸载）时归还。
// 注意：必须在弹窗自己的「自动聚焦」effect 之前调用，否则记住的会是弹窗内部元素。
// R426：进场焦点跳过头部行——弹窗 DOM 顺序第一个可聚焦元素通常是右上角的关闭按钮，
// 焦点落在它上面意味着用户按一下 Enter/空格就把刚打开的弹窗关掉（破坏性默认）。
// 这里优先选头部（.modal-head / .steam-modal-head）之外第一个可聚焦元素，即取消/
// 主操作或主要交互（如 Steam 搜索框）；整个弹窗只剩头部有关键元素时才回退头一个。
// R430：进场 / 归还的行为开关。lockScroll 默认开（模态弹窗该锁背景）；瞬态浮层
// （右键菜单这种小浮层）传 false——它不是模态，锁住背景会让滚轮彻底没反应。
export type DialogFocusOptions = { lockScroll?: boolean };

export function useReturnFocus(open: boolean, options: DialogFocusOptions = {}): void {
  const prev = useRef<HTMLElement | null>(null);
  const lockScroll = options.lockScroll !== false;
  useEffect(() => {
    if (!open) return;
    // R429：打开期间锁住背景滚动（见下方 lockAncestorScroll）；R430 允许逐点关掉。
    const unlockScroll = lockScroll ? lockAncestorScroll(topDialog(), viewport()) : () => {};
    prev.current = focusReturnTarget(typeof document === 'undefined' ? null : document.activeElement);
    return () => {
      prev.current?.focus?.();
      prev.current = null;
      unlockScroll();
    };
  }, [open, lockScroll]);
}

export function useDialogFocus(open: boolean, options: DialogFocusOptions = {}): void {
  const prev = useRef<HTMLElement | null>(null);
  const lockScroll = options.lockScroll !== false;
  useEffect(() => {
    if (!open) return;
    // 进场前先抓住 opener（此时焦点还在触发按钮上），关闭时还给它。
    prev.current = focusReturnTarget(typeof document === 'undefined' ? null : document.activeElement);
    // 最上层弹窗 = DOM 顺序最后一个（嵌套弹窗总在其父对话框之后渲染）。
    const dialogs = document.querySelectorAll<HTMLElement>(MODAL_SEL);
    const top = dialogs[dialogs.length - 1];
    // R429：打开期间锁住背景滚动（见下方 lockAncestorScroll）；R430 可逐点关掉。
    const unlockScroll = lockScroll ? lockAncestorScroll(top, viewport()) : () => {};
    if (top) {
      // 弹窗里第一个真能聚焦的元素；若容器自身可聚焦（tabIndex>=0）则回退容器。
      const list = focusableWithin(top);
      // R426：头部（.modal-head / .steam-modal-head）里的关闭按钮不做初始焦点——
      // 按一下 Enter 就把刚打开的弹窗关掉太容易误触；优先头部之外第一个可聚焦元素。
      const head = top.querySelector<HTMLElement>('.modal-head, .steam-modal-head');
      const target = (head ? list.filter((el) => !head.contains(el)) : list)[0] ?? list[0] ?? (top.tabIndex >= 0 ? top : null);
      target?.focus?.();
    }
    return () => {
      prev.current?.focus?.();
      prev.current = null;
      unlockScroll();
    };
  }, [open, lockScroll]);
}

// ── R429：弹窗打开时锁住背景滚动 ──
// 弹窗遮罩是 position:fixed 全屏（祖先里没有 transform/filter，固定定位不会被改写），
// 但它挂在各 view 的 main 里；滚轮落在遮罩上时浏览器去找「最近的可滚动祖先」——弹窗内容
// 短时背景页跟着滚，正在看的内容被滚走（清理页/优化器/历史页的滚动容器还不一定是 main）。
// 这里锁住弹窗自身的可滚动祖先（不动弹窗内部，所以 modal-body 照常滚），按元素引用计数：
// 嵌套弹窗得全部关掉才恢复，避免内层先解锁把外层的锁一并解开。
const SCROLL_LOCK_ATTR = 'data-scroll-locked';
type LockEntry = { prev: string; count: number };
const scrollLocks = new Map<HTMLElement, LockEntry>();

type ScrollStyleSource = { getComputedStyle: (el: Element) => { overflowY: string; overflow: string } };

function isScrollableY(node: Element, win: ScrollStyleSource): boolean {
  const cs = win.getComputedStyle(node);
  return /^(auto|scroll|overlay)$/.test(cs.overflowY) || /^(auto|scroll|overlay)$/.test(cs.overflow);
}

function viewport(): (Window & typeof globalThis) | null {
  return typeof window === 'undefined' ? null : window;
}

function topDialog(): HTMLElement | null {
  if (typeof document === 'undefined') return null;
  const dialogs = document.querySelectorAll<HTMLElement>(MODAL_SEL);
  return dialogs[dialogs.length - 1] ?? null;
}

// 纯 DOM 操作：从 from 的父级往上找可滚动祖先并锁住，返回解锁函数。
// 只改祖先链，不碰 from 自身与它的子树——弹窗内部该滚的地方照滚。
export function lockAncestorScroll(
  from: Element | null | undefined,
  win: ScrollStyleSource | null | undefined,
): () => void {
  if (!from || !win) return () => {};
  const locked: HTMLElement[] = [];
  let node: Element | null = from.parentElement ?? null;
  while (node) {
    if (isScrollableY(node, win)) {
      const el = node as HTMLElement;
      const entry = scrollLocks.get(el);
      if (entry) {
        entry.count += 1;
      } else {
        const prev = el.style.overflow;
        el.style.overflow = 'hidden';
        el.setAttribute(SCROLL_LOCK_ATTR, '');
        scrollLocks.set(el, { prev, count: 1 });
      }
      locked.push(el);
    }
    node = node.parentElement;
  }
  return () => {
    for (const el of locked) {
      const entry = scrollLocks.get(el);
      if (!entry) continue;
      entry.count -= 1;
      if (entry.count > 0) continue;
      el.style.overflow = entry.prev;
      el.removeAttribute(SCROLL_LOCK_ATTR);
      scrollLocks.delete(el);
    }
  };
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
