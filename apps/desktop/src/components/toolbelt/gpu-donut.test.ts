import { describe, expect, it } from 'vitest';
import { parseGpuTemp, fpsSparkPath, fpsRange, tempHistCurveLine } from './gpu-donut';
import { parseStressCurve } from './bench';

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

describe('tempHistCurveLine（甜甜圈温度曲线序列化，R124）', () => {
  it('不足 2 点返回空串（无曲线不虚构）', () => {
    expect(tempHistCurveLine([])).toBe('');
    expect(tempHistCurveLine([61])).toBe('');
  });
  it('序列化为 stress_test 同契约的「温度曲线」行，parseStressCurve 可解析回', () => {
    const hist = [61, 63.5, 68, 72.2];
    const line = tempHistCurveLine(hist);
    expect(line.startsWith('\n温度曲线：0.0s=61.0°C; 1.0s=63.5°C; 2.0s=68.0°C; 3.0s=72.2°C')).toBe(true);
    const pts = parseStressCurve(line);
    expect(pts).toEqual([
      { sec: 0, temp: 61 },
      { sec: 1, temp: 63.5 },
      { sec: 2, temp: 68 },
      { sec: 3, temp: 72.2 },
    ]);
  });
  it('窗口上限 60 个采样（slice(-60) 后仍可序列化）', () => {
    const hist = Array.from({ length: 60 }, (_, i) => 50 + i * 0.1);
    const line = tempHistCurveLine(hist);
    expect(parseStressCurve(line)).toHaveLength(60);
  });
  // R133：tempHistRef 不再截窗（退出摘要画全长升温段）；>60 样本的序列化
  // 必须完整往返——长压测的曲线不再丢前段。
  it('超过 60 个采样仍完整序列化（R133 全长历史）', () => {
    const hist = Array.from({ length: 90 }, (_, i) => 45 + i * 0.5);
    const line = tempHistCurveLine(hist);
    const pts = parseStressCurve(line);
    expect(pts).not.toBeNull();
    expect(pts).toHaveLength(90);
    expect(pts![0]).toEqual({ sec: 0, temp: 45 });
    expect(pts![pts!.length - 1]).toEqual({ sec: 89, temp: 45 + 89 * 0.5 });
  });
});