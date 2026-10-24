import { describe, it, expect, vi } from 'vitest';
import { ListenerGuard } from './tauriEvent';

describe('监听器的取消竞态安全（R387 ListenerGuard）', () => {
  it('attach 正常完成 → cancel 时解绑一次', async () => {
    const un = vi.fn();
    const g = new ListenerGuard<string>();
    await g.attach(async () => un, () => {});
    g.cancel();
    expect(un).toHaveBeenCalledTimes(1);
  });

  it('cancel 先于 attach 完成 → 迟到的解绑立即调用（不留失去所有者的监听器）', async () => {
    const un = vi.fn();
    const g = new ListenerGuard<string>();
    const pending = g.attach(async () => un, () => {});
    g.cancel();
    await pending;
    expect(un).toHaveBeenCalledTimes(1);
  });

  it('cancel 之后回调不再透传', async () => {
    const h = vi.fn();
    let wrapped: ((p: string) => void) | undefined;
    const g = new ListenerGuard<string>();
    await g.attach(async (cb) => { wrapped = cb; return () => {}; }, h);
    wrapped!('before');
    g.cancel();
    wrapped!('after');
    expect(h).toHaveBeenCalledTimes(1);
    expect(h).toHaveBeenCalledWith('before');
  });

  it('重复 cancel 幂等——解绑只发生一次', async () => {
    const un = vi.fn();
    const g = new ListenerGuard<string>();
    await g.attach(async () => un, () => {});
    g.cancel();
    g.cancel();
    expect(un).toHaveBeenCalledTimes(1);
  });

  it('没 attach 就 cancel 不出错，之后 attach 也不会残留', async () => {
    const un = vi.fn();
    const g = new ListenerGuard<string>();
    g.cancel();
    await g.attach(async () => un, () => {});
    expect(un).toHaveBeenCalledTimes(1);
  });
});
