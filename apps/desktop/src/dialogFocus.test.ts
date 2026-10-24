import { describe, it, expect, vi } from 'vitest';
import { focusReturnTarget, focusableWithin, trapTarget, trapTabKey, MODAL_SEL } from './dialogFocus';

describe('焦点归还目标（R344 focusReturnTarget）', () => {
  it('能接收焦点的元素原样返回', () => {
    const el = { focus: vi.fn() };
    expect(focusReturnTarget(el)).toBe(el);
  });

  it('null / undefined / 空对象 / focus 不是函数 / 字符串都不算可归还目标', () => {
    expect(focusReturnTarget(null)).toBeNull();
    expect(focusReturnTarget(undefined)).toBeNull();
    expect(focusReturnTarget({})).toBeNull();
    expect(focusReturnTarget({ focus: 'nope' })).toBeNull();
    expect(focusReturnTarget('body')).toBeNull();
  });

  it('归还动作只要求 focus 可调用（不认具体类名 / 标签名）', () => {
    let called = 0;
    const el = { focus: () => { called += 1; } };
    focusReturnTarget(el)?.focus();
    expect(called).toBe(1);
  });
});

// ── R347：Tab 不跑出弹窗 ──

function fakeEl(attrs: Record<string, string> = {}, tabIndex = 0) {
  return {
    focus: vi.fn(),
    tabIndex,
    hasAttribute: (n: string) => n in attrs,
    getAttribute: (n: string) => (n in attrs ? attrs[n] : null),
  };
}

function fakeRoot(els: ReturnType<typeof fakeEl>[]) {
  return { querySelectorAll: () => els } as unknown as ParentNode;
}

describe('Tab 循环序号（R347 trapTarget）', () => {
  it('没有可聚焦元素时返回 -1', () => {
    expect(trapTarget(0, 0, false)).toBe(-1);
    expect(trapTarget(0, -3, true)).toBe(-1);
  });

  it('正向绕圈：最后一个再 Tab 回第一个', () => {
    expect(trapTarget(0, 3, false)).toBe(1);
    expect(trapTarget(2, 3, false)).toBe(0);
  });

  it('反向绕圈：第一个 Shift+Tab 到最后一个', () => {
    expect(trapTarget(2, 3, true)).toBe(1);
    expect(trapTarget(0, 3, true)).toBe(2);
  });

  it('焦点不在弹窗内（-1 或越界）时先拉回弹窗：正向给第一个、反向给最后一个', () => {
    expect(trapTarget(-1, 3, false)).toBe(0);
    expect(trapTarget(-1, 3, true)).toBe(2);
    expect(trapTarget(9, 3, false)).toBe(0);
    expect(trapTarget(9, 3, true)).toBe(2);
  });
});

describe('弹窗内可聚焦元素（R347 focusableWithin）', () => {
  it('没有容器或容器为空时返回空数组', () => {
    expect(focusableWithin(null)).toEqual([]);
    expect(focusableWithin(undefined)).toEqual([]);
    expect(focusableWithin(fakeRoot([]))).toEqual([]);
  });

  it('跳过 disabled / aria-hidden="true" / 负 tabindex，其余按原顺序保留', () => {
    const a = fakeEl();
    const disabled = fakeEl({ disabled: '' });
    const hidden = fakeEl({ 'aria-hidden': 'true' });
    const negative = fakeEl({}, -1);
    const b = fakeEl({}, 0);
    expect(focusableWithin(fakeRoot([a, disabled, hidden, negative, b]))).toEqual([a, b]);
  });
});

describe('Tab 按键接管（R347 trapTabKey）', () => {
  it('不是 Tab、或没有容器时不管', () => {
    const el = fakeEl();
    const preventDefault = vi.fn();
    expect(trapTabKey(fakeRoot([el]), { key: 'Enter', preventDefault }, el)).toBe(false);
    expect(trapTabKey(null, { key: 'Tab', preventDefault }, el)).toBe(false);
    expect(preventDefault).not.toHaveBeenCalled();
  });

  it('容器里一个可聚焦元素都没有时放手（不拦 Tab、不硬挪焦点）', () => {
    const preventDefault = vi.fn();
    expect(trapTabKey(fakeRoot([]), { key: 'Tab', preventDefault }, null)).toBe(false);
    expect(preventDefault).not.toHaveBeenCalled();
  });

  it('在最后一个元素上 Tab 会把焦点送回第一个，并 preventDefault', () => {
    const a = fakeEl();
    const b = fakeEl();
    const preventDefault = vi.fn();
    expect(trapTabKey(fakeRoot([a, b]), { key: 'Tab', preventDefault }, b)).toBe(true);
    expect(preventDefault).toHaveBeenCalledTimes(1);
    expect(a.focus).toHaveBeenCalledTimes(1);
    expect(b.focus).not.toHaveBeenCalled();
  });

  it('焦点在弹窗外（背景页元素）时，Tab 直接落到弹窗第一个元素', () => {
    const a = fakeEl();
    const b = fakeEl();
    const outside = fakeEl();
    expect(trapTabKey(fakeRoot([a, b]), { key: 'Tab' }, outside)).toBe(true);
    expect(a.focus).toHaveBeenCalledTimes(1);
  });

  it('Shift+Tab 在第一个元素上回绕到最后一个', () => {
    const a = fakeEl();
    const b = fakeEl();
    expect(trapTabKey(fakeRoot([a, b]), { key: 'Tab', shiftKey: true }, a)).toBe(true);
    expect(b.focus).toHaveBeenCalledTimes(1);
  });
});

describe('弹窗选择器（R347 MODAL_SEL）', () => {
  it('同时认 dialog 与通用确认框用的 alertdialog，并要求 aria-modal', () => {
    expect(MODAL_SEL).toContain('[role="dialog"][aria-modal="true"]');
    expect(MODAL_SEL).toContain('[role="alertdialog"][aria-modal="true"]');
  });
});
