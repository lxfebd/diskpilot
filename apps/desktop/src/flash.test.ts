import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { FlashTimer, FLASH_MS } from './flash';

describe('提示文案定时器（R358）', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('到点回调一次并自己复位', () => {
    const onClear = vi.fn();
    const t = new FlashTimer(onClear, 1000);
    t.arm();
    expect(t.pending).toBe(true);
    vi.advanceTimersByTime(999);
    expect(onClear).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onClear).toHaveBeenCalledTimes(1);
    expect(t.pending).toBe(false);
  });

  it('连续 arm 只保留最新一次——前一条不会把新文案提前抹掉', () => {
    const onClear = vi.fn();
    const t = new FlashTimer(onClear, 1000);
    t.arm();
    vi.advanceTimersByTime(900);
    t.arm();
    vi.advanceTimersByTime(900); // 距第一条 1800ms：若没清掉旧定时器，这里就清空了
    expect(onClear).not.toHaveBeenCalled();
    vi.advanceTimersByTime(100);
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it('clear() 取消未到点的清空，可重复调用', () => {
    const onClear = vi.fn();
    const t = new FlashTimer(onClear, 1000);
    t.arm();
    t.clear();
    t.clear();
    vi.advanceTimersByTime(5000);
    expect(onClear).not.toHaveBeenCalled();
    expect(t.pending).toBe(false);
  });

  it('默认停留 3.5 秒', () => {
    expect(FLASH_MS).toBe(3500);
    const onClear = vi.fn();
    const t = new FlashTimer(onClear);
    t.arm();
    vi.advanceTimersByTime(FLASH_MS - 1);
    expect(onClear).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onClear).toHaveBeenCalledTimes(1);
  });
});