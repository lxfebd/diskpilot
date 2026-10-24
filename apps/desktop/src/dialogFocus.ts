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
