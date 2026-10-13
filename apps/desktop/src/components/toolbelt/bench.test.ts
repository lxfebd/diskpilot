import { describe, expect, it } from 'vitest';
import { parseStressCurve } from './bench';
import { parseTrendJson } from './charts';

// R99：压测温度曲线解析锁定格式契约——后端输出行「温度曲线：0.0s=52.1°C; …」
// 是中文格式（非文案），解析正则改动必须有测试保护（同 gpu-donut parseGpuTemp）。

describe('parseStressCurve（压测温度曲线解析）', () => {
  it('解析整行 → 有序点序列', () => {
    const text = '压测完成，最大温度 65.0°C\n温度曲线：0.0s=52.1°C; 1.0s=58.3°C; 2.0s=65.0°C';
    const pts = parseStressCurve(text);
    expect(pts).toEqual([
      { sec: 0.0, temp: 52.1 },
      { sec: 1.0, temp: 58.3 },
      { sec: 2.0, temp: 65.0 },
    ]);
  });

  it('支持小数秒与无小数数值', () => {
    const pts = parseStressCurve('温度曲线：0s=50°C; 0.5s=55.5°C; 1s=60°C');
    expect(pts).toEqual([
      { sec: 0, temp: 50 },
      { sec: 0.5, temp: 55.5 },
      { sec: 1, temp: 60 },
    ]);
  });

  it('无温度曲线行 → null；只有 1 个点 → null（不足画线）', () => {
    expect(parseStressCurve('压测完成，最大温度 65.0°C')).toBeNull();
    expect(parseStressCurve('温度曲线：0.0s=52.1°C')).toBeNull();
    expect(parseStressCurve('')).toBeNull();
  });

  it('与其它行混排也能找到曲线行', () => {
    const text = ['第一行：开始压测', '温度曲线：0.0s=40°C; 1.0s=41°C', '最后一行：结束'].join('\n');
    expect(parseStressCurve(text)).toHaveLength(2);
  });
});

// R111：sensor_trend format=json 实时通道解析——锁定与后端 rep_report.rs 的
// JSON 契约（{ total, points: [{ ts, cpu_temp_c?, mem_percent? }] }）。
describe('parseTrendJson（sensor_trend JSON 解析）', () => {
  it('解析标准输出 → 温度/内存点序列', () => {
    const text = JSON.stringify({
      total: 3,
      points: [
        { ts: 1000, cpu_temp_c: 52.1, mem_percent: 41.2 },
        { ts: 1001, cpu_temp_c: 58.3, mem_percent: 43.0 },
        { ts: 1002, cpu_temp_c: 65.0, mem_percent: 44.5 },
      ],
    });
    expect(parseTrendJson(text)).toEqual([
      { ts: 1000, cpu_temp_c: 52.1, mem_percent: 41.2 },
      { ts: 1001, cpu_temp_c: 58.3, mem_percent: 43.0 },
      { ts: 1002, cpu_temp_c: 65.0, mem_percent: 44.5 },
    ]);
  });

  it('cpu_temp_c 为 null 的点跳过该字段（后端无温度源时如实记 null）', () => {
    const text = JSON.stringify({
      total: 2,
      points: [
        { ts: 1000, cpu_temp_c: null, mem_percent: 40.0 },
        { ts: 1001, cpu_temp_c: null, mem_percent: 41.0 },
      ],
    });
    const pts = parseTrendJson(text);
    expect(pts).toEqual([
      { ts: 1000, mem_percent: 40.0 },
      { ts: 1001, mem_percent: 41.0 },
    ]);
  });

  it('非 JSON / 缺 points / 不足 2 点 → null（不足画线）', () => {
    expect(parseTrendJson('不是 JSON')).toBeNull();
    expect(parseTrendJson('{"total":5}')).toBeNull();
    expect(parseTrendJson('{"points":[{"ts":1,"cpu_temp_c":50}]}')).toBeNull();
    expect(parseTrendJson('{"points":[]}')).toBeNull();
  });

  it('非法点（缺 ts / 字段类型错误）跳过，其余保留', () => {
    const text = JSON.stringify({
      points: [
        { ts: 1000, cpu_temp_c: 50 },
        { ts: 'bad', cpu_temp_c: 88 },
        { cpu_temp_c: 90 },
        { ts: 1002, cpu_temp_c: 'na' },
        { ts: 1003, cpu_temp_c: 51 },
      ],
    });
    expect(parseTrendJson(text)).toEqual([
      { ts: 1000, cpu_temp_c: 50 },
      { ts: 1002 },
      { ts: 1003, cpu_temp_c: 51 },
    ]);
  });

  it('非法点过滤后只剩 1 个 → null（不足画线）', () => {
    const text = JSON.stringify({
      points: [
        { ts: 'bad', cpu_temp_c: 88 },
        { ts: 1000, cpu_temp_c: 50 },
      ],
    });
    expect(parseTrendJson(text)).toBeNull();
  });
});