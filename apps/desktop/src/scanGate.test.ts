import { describe, expect, it } from 'vitest';
import { ScanGate } from './scanGate';

describe('扫描单飞闸门（R383 ScanGate）', () => {
  it('第一份取到闸，第二份被拒', () => {
    const g = new ScanGate();
    expect(g.busy).toBe(false);
    expect(g.tryEnter()).toBe(true);
    expect(g.busy).toBe(true);
    expect(g.tryEnter()).toBe(false);
    expect(g.busy).toBe(true);
  });

  it('放闸后可以再取——成功/失败/取消都必须 leave', () => {
    const g = new ScanGate();
    expect(g.tryEnter()).toBe(true);
    g.leave();
    expect(g.busy).toBe(false);
    expect(g.tryEnter()).toBe(true);
    expect(g.tryEnter()).toBe(false);
  });

  it('没取就放、多放一次都不出错，也不会误放别人的闸', () => {
    const g = new ScanGate();
    g.leave();
    expect(g.busy).toBe(false);
    expect(g.tryEnter()).toBe(true);
    g.leave();
    g.leave();
    expect(g.busy).toBe(false);
    expect(g.tryEnter()).toBe(true);
  });
});
