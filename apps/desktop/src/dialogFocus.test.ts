import { describe, it, expect, vi } from 'vitest';
import { focusReturnTarget } from './dialogFocus';

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
