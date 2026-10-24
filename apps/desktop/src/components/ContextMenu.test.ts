// R353：右键菜单的键盘导航——菜单项之间怎么走（纯函数，便于单测）。
import { describe, it, expect } from 'vitest';
import { nextMenuIndex } from './ContextMenu';

describe('右键菜单的方向键落点（R353 nextMenuIndex）', () => {
  it('向下走一项，到底环绕回第一项', () => {
    expect(nextMenuIndex(0, 3, 1)).toBe(1);
    expect(nextMenuIndex(1, 3, 1)).toBe(2);
    expect(nextMenuIndex(2, 3, 1)).toBe(0);
  });

  it('向上走一项，到顶环绕回最后一项', () => {
    expect(nextMenuIndex(2, 3, -1)).toBe(1);
    expect(nextMenuIndex(0, 3, -1)).toBe(2);
  });

  it('还没落在任何一项上时，向下从第一项开始、向上从最后一项开始', () => {
    expect(nextMenuIndex(-1, 3, 1)).toBe(0);
    expect(nextMenuIndex(-1, 3, -1)).toBe(2);
  });

  it('只有一项时怎么走都是它自己', () => {
    expect(nextMenuIndex(0, 1, 1)).toBe(0);
    expect(nextMenuIndex(0, 1, -1)).toBe(0);
  });

  it('空菜单没有落点', () => {
    expect(nextMenuIndex(-1, 0, 1)).toBe(-1);
    expect(nextMenuIndex(0, 0, -1)).toBe(-1);
  });

  it('越界的当前下标按「不在任何项上」处理', () => {
    expect(nextMenuIndex(5, 3, 1)).toBe(0);
    expect(nextMenuIndex(5, 3, -1)).toBe(2);
  });
});