import { describe, expect, it } from 'vitest';
import { perfScaleFor, pollIntervalMs } from './perfHooks';

// 低配降载参数（纯函数）：弱机降载、中机微降、强机不动。
describe('perfScaleFor（低配降载档位参数）', () => {
  it('weak：overscan 裁到 6、轮询翻倍', () => {
    const s = perfScaleFor('weak');
    expect(s.scale).toBe(1);
    expect(s.overscan).toBe(6);
    expect(s.pollFactor).toBe(2);
  });

  it('medium：overscan 10、轮询 1.5 倍', () => {
    const s = perfScaleFor('medium');
    expect(s.scale).toBe(0.5);
    expect(s.overscan).toBe(10);
    expect(s.pollFactor).toBe(1.5);
  });

  it('strong：不降载（overscan 14 原值、轮询 1 倍）', () => {
    const s = perfScaleFor('strong');
    expect(s.scale).toBe(0);
    expect(s.overscan).toBe(14);
    expect(s.pollFactor).toBe(1);
  });
});

describe('pollIntervalMs（轮询间隔按档位缩放）', () => {
  it('强机 5s 轮询保持 5000ms', () => {
    expect(pollIntervalMs(perfScaleFor('strong'), 5000)).toBe(5000);
  });

  it('弱机 5s 轮询翻倍到 10000ms', () => {
    expect(pollIntervalMs(perfScaleFor('weak'), 5000)).toBe(10000);
  });

  it('弱机 1s 温度轮询翻倍到 2000ms', () => {
    expect(pollIntervalMs(perfScaleFor('weak'), 1000)).toBe(2000);
  });

  it('中机 5s 轮询到 7500ms（1.5 倍取整）', () => {
    expect(pollIntervalMs(perfScaleFor('medium'), 5000)).toBe(7500);
  });
});
