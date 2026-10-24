import { describe, expect, it, afterEach } from 'vitest';
import {
  isEscapeClose,
  pickTop,
  pushEscapeLayer,
  removeEscapeLayer,
  escapeStackDepth,
  escapeStackTop,
} from './escapeKey';

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

// R349：叠层时 Esc 只关最上面那一层。
describe('Esc 栈顶（R349 pickTop）', () => {
  it('空栈 → null（没有弹窗就不吃键）', () => {
    expect(pickTop([])).toBeNull();
  });
  it('只有一层 → 就是它', () => {
    const a = () => undefined;
    expect(pickTop([a])).toBe(a);
  });
  it('多层 → 取最后挂上来的（更深的先响应，不会父子一起关）', () => {
    const a = () => undefined;
    const b = () => undefined;
    const c = () => undefined;
    expect(pickTop([a, b, c])).toBe(c);
    expect(pickTop([a, b])).toBe(b);
  });
  it('不改变原栈', () => {
    const a = () => undefined;
    const s = [a];
    pickTop(s);
    expect(s).toEqual([a]);
  });
});

describe('Esc 栈的压入与弹出（R349）', () => {
  afterEach(() => {
    // 模块级栈是共享状态：每条用例自己收干净，避免串味。
    while (escapeStackDepth() > 0) {
      const top = escapeStackTop();
      if (top) removeEscapeLayer(top);
    }
  });

  it('父层先开、子层后开 → 栈顶是子层；关掉子层后父层重新接管', () => {
    const parent = () => undefined;
    const child = () => undefined;
    pushEscapeLayer(parent);
    pushEscapeLayer(child);
    expect(escapeStackDepth()).toBe(2);
    expect(escapeStackTop()).toBe(child);
    removeEscapeLayer(child);
    expect(escapeStackDepth()).toBe(1);
    expect(escapeStackTop()).toBe(parent);
    removeEscapeLayer(parent);
    expect(escapeStackDepth()).toBe(0);
    expect(escapeStackTop()).toBeNull();
  });

  it('移除一个没入栈的函数不会破坏栈（按引用定位，静默忽略）', () => {
    const a = () => undefined;
    pushEscapeLayer(a);
    removeEscapeLayer(() => undefined);
    expect(escapeStackDepth()).toBe(1);
    expect(escapeStackTop()).toBe(a);
  });
});
