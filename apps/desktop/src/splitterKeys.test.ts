import { describe, it, expect } from 'vitest';
import { splitterDelta, SPLITTER_STEP, SPLITTER_STEP_LARGE } from './splitterKeys';

describe('分隔条方向键（R371）', () => {
  it('SPLITTER_STEP / SPLITTER_STEP_LARGE 是正数且大步大于小步', () => {
    expect(SPLITTER_STEP).toBeGreaterThan(0);
    expect(SPLITTER_STEP_LARGE).toBeGreaterThan(SPLITTER_STEP);
  });

  it('← 向左 -step，→ 向右 +step', () => {
    expect(splitterDelta({ key: 'ArrowLeft' })).toBe(-SPLITTER_STEP);
    expect(splitterDelta({ key: 'ArrowRight' })).toBe(SPLITTER_STEP);
  });

  it('Shift 一起按时走大步（一次性多调一点）', () => {
    expect(splitterDelta({ key: 'ArrowLeft', shiftKey: true })).toBe(-SPLITTER_STEP_LARGE);
    expect(splitterDelta({ key: 'ArrowRight', shiftKey: true })).toBe(SPLITTER_STEP_LARGE);
  });

  it('不是左右方向键的键返回 null（不能吃掉 Tab / 上下键 / Enter）', () => {
    for (const key of ['ArrowUp', 'ArrowDown', 'Tab', 'Enter', 'a', '', 'ArrowLeftx']) {
      expect(splitterDelta({ key })).toBeNull();
    }
    expect(splitterDelta({ key: 'Enter', shiftKey: true })).toBeNull();
  });

  it('事件对象没有 shiftKey 也不炸（合成事件最小形状）', () => {
    expect(splitterDelta({ key: 'ArrowRight' })).toBe(SPLITTER_STEP);
  });
});
