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
