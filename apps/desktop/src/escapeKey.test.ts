import { describe, expect, it } from 'vitest';
import { isEscapeClose } from './escapeKey';

// R141：Esc 关窗判定——4 处手写 keydown 收敛后的单一真值。
describe('isEscapeClose（Esc 关窗判定）', () => {
  it('Escape + 启用 → 关闭', () => {
    expect(isEscapeClose('Escape', true)).toBe(true);
  });
  it('未启用 → 不关（弹窗没开不该吃键）', () => {
    expect(isEscapeClose('Escape', false)).toBe(false);
  });
  it('其他按键 → 不关（含 Esc 的别名差异不做模糊匹配）', () => {
    expect(isEscapeClose('Enter', true)).toBe(false);
    expect(isEscapeClose('escape', true)).toBe(false);
    expect(isEscapeClose('', true)).toBe(false);
  });
});