import { describe, expect, it } from 'vitest';
import { parseGpuTemp } from './toolbelt/gpu-donut';

// GPU 甜甜圈熔断守门（90℃ 自动退出）：温度解析唯一数据源是
// hw_temperature 文本里 `GPU（厂商）：数字 ℃` 行。N/A 卡任一命中即护栏生效；
// 无温度行返回 null（前端显示 N/A，不误熔断）。契约与 agent-server hw.rs
// 的 collect_temperature 输出一致（parse_gpu_temp_line 测试双端锁定）。
describe('parseGpuTemp（甜甜圈温度熔断解析）', () => {
  it('命中 NVIDIA 行（nvidia-smi 通道）', () => {
    const text = '温度传感器：\n- GPU（NVIDIA）：63 ℃（nvidia-smi 实时）\n说明：…';
    expect(parseGpuTemp(text)).toBe(63);
  });

  it('命中 AMD 行（atiadlxx ADL 通道）——本次跨厂商泛化的核心', () => {
    const text = '温度传感器：\n- GPU（AMD）：58 ℃（atiadlxx ADL 实时）\n说明：…';
    expect(parseGpuTemp(text)).toBe(58);
  });

  it('超 90℃ 的高温能解析（触发熔断的信号）', () => {
    expect(parseGpuTemp('- GPU（AMD）：91 ℃（atiadlxx ADL 实时）')).toBe(91);
    expect(parseGpuTemp('- GPU（NVIDIA）：95 ℃（nvidia-smi 实时）')).toBe(95);
  });

  it('无温度行 / 降级占位 → null，不误触发熔断', () => {
    expect(parseGpuTemp('温度传感器：\n- 热区：35 ℃\n说明：…')).toBeNull();
    expect(parseGpuTemp('- GPU：非 NVIDIA/AMD 或驱动未装，WMI 不提供独立显卡温度')).toBeNull();
    expect(parseGpuTemp('')).toBeNull();
  });

  it('非法数字不解析（防 NaN 误熔断）', () => {
    expect(parseGpuTemp('- GPU（AMD）：abc ℃（atiadlxx ADL 实时）')).toBeNull();
  });
});