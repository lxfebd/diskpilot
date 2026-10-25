// R314：可点行/格的键盘出口。此前全仓只有侧栏写了 :focus-visible，列表行
// （树视图行、文件列表行、空间大头行、页脚重扫）都是纯鼠标——键盘用户 Tab 跳过、
// Enter 没反应，等于这些地方对他们不存在。这里把「按什么键等于点一下」收敛成
// 一处口径（与原生 button 一致：Enter / 空格），组件只管接线，别再各写各的。

/** 触发一次点击语义的键：Enter 与空格（对齐原生 button 行为）。 */
export function isActivationKey(key: string): boolean {
  return key === 'Enter' || key === ' ';
}

/** 键盘事件的最小形状：不引 React 类型，纯函数才好单测。 */
export interface KeyLike {
  key: string;
  preventDefault?: () => void;
}

/**
 * 包一层 onKeyDown：只有激活键才执行 handler，并阻止空格滚页面/Enter 冒泡的默认行为。
 * 用法：`onKeyDown={activateOnKey(() => onClick())}`，配合 role="button" + tabIndex={0}。
 */
export function activateOnKey(handler: () => void) {
  return (e: KeyLike): void => {
    if (!isActivationKey(e.key)) return;
    e.preventDefault?.();
    handler();
  };
}
/**
 * R365：在一组等长控件里按 delta 移动并环绕（tab / 菜单 / 分段控件通用）。
 * current 越界或为负时从最近的端点算起：delta>=0 → 0，否则 len-1。
 * len<=0 返回 -1（没有可落的位置）。纯函数，便于单测钉住环绕语义。
 */
export function cycleIndex(current: number, len: number, delta: number): number {
  if (len <= 0) return -1;
  if (current < 0 || current >= len) return delta >= 0 ? 0 : len - 1;
  return (current + delta + len) % len;
}
// R368：右键菜单的键盘等价入口（Windows 通行口径：菜单键，或 Shift+F10）。
// 文件树 / 文件列表的操作（打开位置、复制路径、移入回收站）此前只能右键唤出，
// 键盘用户完全够不到同一组动作。
export function isContextMenuKey(e: { key: string; shiftKey?: boolean }): boolean {
  return e.key === 'ContextMenu' || (e.shiftKey === true && e.key === 'F10');
}

// R368：键盘唤出菜单时的锚点——元素左下角内侧一点，与鼠标点行内任意位置
// 的观感一致。拿不到元素或尺寸为 0（虚拟列表尚未布局）时返回 null。
export function contextMenuAnchor(el: Element | null): { x: number; y: number } | null {
  if (!el) return null;
  const r = el.getBoundingClientRect();
  if (!(r.width > 0 || r.height > 0)) return null;
  return { x: Math.round(r.left + 12), y: Math.round(r.top + Math.min(r.height, 22)) };
}

// R439：搜索框内按 Esc 一键清空——有文字时清空并吞掉事件（不冒泡去关弹窗），
// 空输入时放行（Esc 照常走 R349 的弹窗栈关掉当前层）。
// R458：输入法组合期（拼音串 / 候选词）按 Esc 是「取消组合」，不是清空搜索框；
// 一律放行交给输入法，否则中文用户想撤销一次组字会把已输入的整段搜索词清掉。
// 判定三路与 R404（聊天框 Enter）同口径：合成事件自带 isComposing，另外部分
// 浏览器在组合结束的那一下 keydown 只留 nativeEvent.isComposing 或 keyCode 229。
export function clearSearchOnEscape(clear: () => void) {
  return (e: {
    key: string;
    isComposing?: boolean;
    nativeEvent?: { isComposing?: boolean; keyCode?: number };
    preventDefault?: () => void;
    stopPropagation?: () => void;
    currentTarget?: { value: string };
  }) => {
    if (e.isComposing || e.nativeEvent?.isComposing || e.nativeEvent?.keyCode === 229) return;
    if (e.key === 'Escape' && (e.currentTarget?.value ?? '') !== '') {
      e.preventDefault?.();
      e.stopPropagation?.();
      clear();
    }
  };
}
