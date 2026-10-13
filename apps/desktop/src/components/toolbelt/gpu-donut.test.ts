import { describe, expect, it } from 'vitest';
import { parseGpuTemp, fpsSparkPath, fpsRange } from './gpu-donut';

describe('parseGpuTemp（甜甜圈 90℃ 熔断守门的数据源契约）', () => {
  it('命中英伟达行', () => {
    expect(parseGpuTemp('温度：\nCPU：52 ℃\nGPU（NVIDIA）：71.4 ℃\n')).toBe(71.4);
  });
  it('命中 AMD 行', () => {
    expect(parseGpuTemp('GPU（AMD）：63 ℃')).toBe(63);
  });
  it('无温度行返回 null（诚实：读不到 ≠ 0）', () => {
    expect(parseGpuTemp('GPU 温度不可用')).toBeNull();
    expect(parseGpuTemp('')).toBeNull();
  });
});

describe('fpsSparkPath（FPS 迷你趋势折线）', () => {
  it('不足 2 点返回空串', () => {
    expect(fpsSparkPath([])).toBe('');
    expect(fpsSparkPath([60])).toBe('');
  });
  it('归一化到自身 min/max：两端点贴边', () => {
    const d = fpsSparkPath([60, 120], 120, 34);
    // join 用空格分隔（与 spaceSparkPath 一致）；首点 (0, H-3)，末点 (W, 3)
    expect(d).toBe('M0.0,31.0 L120.0,3.0');
  });
  it('值平 = 水平中线（span=0 时居中，不贴底）', () => {
    const d = fpsSparkPath([60, 60], 120, 34);
    expect(d).toBe('M0.0,17.0 L120.0,17.0');
  });
  it('中间值归一化到中点', () => {
    const d = fpsSparkPath([60, 90, 120], 120, 34);
    // 90 是 (60,120) 中点 → y 中点 (31+3)/2 = 17
    expect(d).toBe('M0.0,31.0 L60.0,17.0 L120.0,3.0');
  });
});

describe('fpsRange（HUD min/max 标注）', () => {
  it('空数组 → null', () => {
    expect(fpsRange([])).toBeNull();
  });
  it('返回 min/max', () => {
    expect(fpsRange([60, 120, 80])).toEqual({ min: 60, max: 120 });
  });
  it('单点 min=max', () => {
    expect(fpsRange([75])).toEqual({ min: 75, max: 75 });
  });
});