import { describe, it, expect } from 'vitest';
import { extractLongSeries, formatSpan, stressHistYMax, trendSpan, type TrendPoint } from './charts';

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

// R136：长期历史三路 series 一次提取（此前 5 处内联 .map().filter()）。
describe('R136 extractLongSeries（cpu/gpu/mem 三路提取）', () => {
  const pts: TrendPoint[] = [
    { ts: 1, cpu_temp_c: 55, gpu_temp_c: 60, mem_percent: 40 },
    { ts: 2, cpu_temp_c: null, gpu_temp_c: 72, mem_percent: null },
    { ts: 3 }, // 字段缺失
    { ts: 4, cpu_temp_c: 88, gpu_temp_c: null, mem_percent: 91.5 },
  ];
  it('读不到的字段如实滤掉，不造 0', () => {
    expect(extractLongSeries(pts)).toEqual({
      cpuTemps: [55, 88],
      gpuTemps: [60, 72],
      memPercents: [40, 91.5],
    });
  });
  it('空点列 → 三路全空数组', () => {
    expect(extractLongSeries([])).toEqual({ cpuTemps: [], gpuTemps: [], memPercents: [] });
  });
  it('不修改入参（纯函数）', () => {
    const input: TrendPoint[] = [{ ts: 1, cpu_temp_c: 50 }];
    extractLongSeries(input);
    expect(input).toEqual([{ ts: 1, cpu_temp_c: 50 }]);
  });
});

// R138：长期历史时间跨度（回答「这批数据跨了多久」）。
describe('R138 trendSpan（采样时间跨度）', () => {
  it('首末差 = 跨度，除以间隔数 = 平均间隔', () => {
    expect(
      trendSpan([{ ts: 100 }, { ts: 400 }, { ts: 1000 }]),
    ).toEqual({ spanSec: 900, avgGapSec: 450 });
  });
  it('少于 2 点 / 全同 ts → null（无法给出跨度）', () => {
    expect(trendSpan([])).toBeNull();
    expect(trendSpan([{ ts: 5 }])).toBeNull();
    expect(trendSpan([{ ts: 7 }, { ts: 7 }])).toBeNull();
  });
  it('ts 非有限值被滤掉', () => {
    expect(trendSpan([{ ts: 0 }, { ts: Number.NaN }, { ts: 60 }])).toEqual({
      spanSec: 60,
      avgGapSec: 60,
    });
  });
});

describe('R138 formatSpan（秒 → 人话跨度）', () => {
  it('天/小时/分/秒四档', () => {
    expect(formatSpan(3 * 86400 + 5 * 3600)).toBe('3d 5h');
    expect(formatSpan(2 * 3600 + 10 * 60)).toBe('2h 10m');
    expect(formatSpan(45 * 60)).toBe('45m');
    expect(formatSpan(30)).toBe('30s');
  });
  it('非正/非法值 → 0s（不抛不编造）', () => {
    expect(formatSpan(0)).toBe('0s');
    expect(formatSpan(-5)).toBe('0s');
    expect(formatSpan(Number.NaN)).toBe('0s');
  });
});
