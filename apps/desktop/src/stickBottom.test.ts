import { describe, it, expect } from 'vitest';
import { stickNearBottom } from './stickBottom';

describe('聊天区贴底跟随（R406 stickNearBottom）', () => {
  it('贴底（距离 0）要跟随', () => {
    expect(stickNearBottom({ scrollTop: 500, scrollHeight: 600, clientHeight: 100 })).toBe(true);
  });

  it('橡皮筋式过冲（过滚一点点）仍算贴底', () => {
    expect(stickNearBottom({ scrollTop: 450, scrollHeight: 600, clientHeight: 100 })).toBe(true);
  });

  it('往上翻过历史就不跟随', () => {
    expect(stickNearBottom({ scrollTop: 0, scrollHeight: 900, clientHeight: 200 })).toBe(false);
  });

  it('阈值：刚好 80 内跟随，超过 80 不跟', () => {
    const base = { scrollHeight: 1000, clientHeight: 100 };
    expect(stickNearBottom({ ...base, scrollTop: 820 })).toBe(true);
    expect(stickNearBottom({ ...base, scrollTop: 819 })).toBe(false);
  });

  it('空元素按跟随处理（首屏挂载前）', () => {
    expect(stickNearBottom(null)).toBe(true);
    expect(stickNearBottom(undefined)).toBe(true);
  });

  it('非有限数值不当作贴底（防 NaN 拽人）', () => {
    expect(stickNearBottom({ scrollTop: NaN, scrollHeight: 100, clientHeight: 100 })).toBe(false);
  });
});
