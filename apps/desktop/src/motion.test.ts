import { describe, it, expect, afterEach, vi } from 'vitest';
import { prefersReducedMotion, scrollBehavior } from './motion';

describe('减少动态效果（R357）', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('无 window（node 测试环境）→ 一律按不减少处理', () => {
    expect(prefersReducedMotion()).toBe(false);
    expect(scrollBehavior()).toBe('smooth');
  });

  it('系统开了减少动态效果 → scrollBehavior 退回 auto，且显式 auto 不变', () => {
    vi.stubGlobal('window', { matchMedia: (q: string) => ({ matches: true, media: q }) });
    expect(prefersReducedMotion()).toBe(true);
    expect(scrollBehavior()).toBe('auto');
    expect(scrollBehavior('auto')).toBe('auto');
  });

  it('系统没开 → 保留调用方偏好', () => {
    vi.stubGlobal('window', { matchMedia: () => ({ matches: false }) });
    expect(prefersReducedMotion()).toBe(false);
    expect(scrollBehavior()).toBe('smooth');
    expect(scrollBehavior('auto')).toBe('auto');
  });

  it('matchMedia 缺失 → 不减少', () => {
    vi.stubGlobal('window', {});
    expect(prefersReducedMotion()).toBe(false);
    expect(scrollBehavior()).toBe('smooth');
  });
});