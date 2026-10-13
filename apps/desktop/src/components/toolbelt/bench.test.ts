import { describe, expect, it } from 'vitest';
import { parseStressCurve } from './bench';

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