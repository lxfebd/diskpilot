import { describe, it, expect, vi } from 'vitest';
import { activateOnKey, isActivationKey } from './a11y';

describe('键盘激活口径（R314）', () => {
  it('只有 Enter 与空格算激活键（对齐原生 button）', () => {
    expect(isActivationKey('Enter')).toBe(true);
    expect(isActivationKey(' ')).toBe(true);
    expect(isActivationKey('a')).toBe(false);
    expect(isActivationKey('Escape')).toBe(false);
    expect(isActivationKey('Tab')).toBe(false);
    expect(isActivationKey('')).toBe(false);
  });

  it('Enter / 空格触发 handler 并阻止默认行为（空格不滚页面）', () => {
    for (const key of ['Enter', ' ']) {
      const handler = vi.fn();
      const preventDefault = vi.fn();
      activateOnKey(handler)({ key, preventDefault });
      expect(handler).toHaveBeenCalledTimes(1);
      expect(preventDefault).toHaveBeenCalledTimes(1);
    }
  });

  it('其他键既不触发也不拦（不能把 Tab / 方向键吃掉）', () => {
    const handler = vi.fn();
    const preventDefault = vi.fn();
    for (const key of ['Tab', 'a', 'ArrowDown', 'Escape']) {
      activateOnKey(handler)({ key, preventDefault });
    }
    expect(handler).not.toHaveBeenCalled();
    expect(preventDefault).not.toHaveBeenCalled();
  });

  it('事件对象没有 preventDefault 也不炸（合成事件最小形状）', () => {
    const handler = vi.fn();
    expect(() => activateOnKey(handler)({ key: 'Enter' })).not.toThrow();
    expect(handler).toHaveBeenCalledTimes(1);
  });
});
