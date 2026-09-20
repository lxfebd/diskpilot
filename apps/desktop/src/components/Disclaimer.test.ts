import { describe, it, expect, beforeEach } from 'vitest';
import { hasAcceptedDisclaimer, canAcceptDisclaimer } from './Disclaimer';

// Disclaimer 组件本体依赖 React 渲染，这里只测可测的纯逻辑：localStorage
// 记忆位（开屏「同意后不再弹」的判定）+ 同意闸门（10 秒倒计时 + 滚到底双条件）。
// DOM/倒计时由组件内部管理。
describe('免责声明 localStorage 记忆位', () => {
  beforeEach(() => {
    try {
      localStorage.removeItem('diskpilot.disclaimer.v1');
    } catch {
      /* node 环境 localStorage 不可用时跳过 */
    }
  });

  it('未同意过 → false', () => {
    expect(hasAcceptedDisclaimer()).toBe(false);
  });

  it('同意过（写入 1）→ true', () => {
    try {
      localStorage.setItem('diskpilot.disclaimer.v1', '1');
    } catch {
      // 无 localStorage 的 node 环境：函数本身返回 false 也符合降级语义
    }
    // 与组件 accept() 写入的键值一致
    const raw = (() => {
      try {
        return localStorage.getItem('diskpilot.disclaimer.v1');
      } catch {
        return null;
      }
    })();
    expect(raw === '1' ? hasAcceptedDisclaimer() : !hasAcceptedDisclaimer()).toBe(true);
  });

  it('其他值（如 0/垃圾）→ 按未同意处理', () => {
    try {
      localStorage.setItem('diskpilot.disclaimer.v1', '0');
    } catch {
      /* 忽略 */
    }
    expect(hasAcceptedDisclaimer()).toBe(false);
  });
});

// 10 秒阅读机制是安全红线：倒计时没走完或声明没滚到底，任一条件不满足都不可同意。
// 抽成纯函数后在这里锁死，防止以后被误放宽（比如改成只滚到底就能点、或去掉倒计时）。
describe('免责声明 10 秒阅读闸门', () => {
  it('倒计时未走完 + 未滚到底 → 不可同意', () => {
    expect(canAcceptDisclaimer(10, false)).toBe(false);
    expect(canAcceptDisclaimer(5, false)).toBe(false);
    expect(canAcceptDisclaimer(1, false)).toBe(false);
  });

  it('只滚到底但倒计时未走完 → 不可同意（防止秒滑到底直接跳闸）', () => {
    expect(canAcceptDisclaimer(10, true)).toBe(false);
    expect(canAcceptDisclaimer(3, true)).toBe(false);
  });

  it('倒计时走完但没滚到底 → 不可同意（防止挂机等时间）', () => {
    expect(canAcceptDisclaimer(0, false)).toBe(false);
  });

  it('倒计时走完且滚到底 → 可同意（唯一放行组合）', () => {
    expect(canAcceptDisclaimer(0, true)).toBe(true);
  });

  it('组件倒计时用 Math.max(0, c-1) clamp 永不小于 0；闸门对负值输入（异常态）不放行', () => {
    // 负值 countdown 是组件内部 Math.max 保证不会出现的越界输入，闸门对异常态保守处理
    expect(canAcceptDisclaimer(-1, true)).toBe(false);
    expect(canAcceptDisclaimer(-5, false)).toBe(false);
  });
});