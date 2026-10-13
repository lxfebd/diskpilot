import { describe, it, expect } from 'vitest';
import { stressHistYMax } from './charts';

describe('R129 叠画 y 轴动态上界 stressHistYMax', () => {
  it('正常 ≤100 的峰值返回 100（视觉口径与固定 0–100 一致）', () => {
    expect(stressHistYMax([[40, 60, 80]])).toBe(100);
    expect(stressHistYMax([[30, 50], [35, 99.9]])).toBe(100);
  });
  it('峰值超过 100 时上探到峰值——曲线不再被顶部截断', () => {
    expect(stressHistYMax([[60, 104.5, 90]])).toBe(104.5);
    expect(stressHistYMax([[50, 70], [80, 112]])).toBe(112);
  });
  it('真空数组 → 100（类型完备分支）', () => {
    expect(stressHistYMax([])).toBe(100);
  });
});
