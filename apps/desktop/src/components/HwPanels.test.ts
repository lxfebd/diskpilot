import { describe, expect, it, beforeAll } from 'vitest';
import { fmtHwTime, gpuLiveSource } from './HwPanels';
import { setLang } from '../i18n';

// fmtHwTime：把归档 Unix 秒转成本地可读时间；非法/空输入返回空串（展示层兜底）。
describe('fmtHwTime', () => {
  it('把 Unix 秒转成本地时间字符串', () => {
    const s = fmtHwTime(1_700_086_400);
    expect(typeof s).toBe('string');
    expect(s.length).toBeGreaterThan(0);
  });

  it('接受字符串形式的数字', () => {
    const s = fmtHwTime('1700086400');
    expect(typeof s).toBe('string');
    expect(s.length).toBeGreaterThan(0);
  });

  it('非法输入返回空串（不抛异常）', () => {
    expect(fmtHwTime('not-a-time')).toBe('');
    expect(fmtHwTime(-1)).toBe('');
    expect(fmtHwTime(Number.NaN)).toBe('');
  });
});

// GPU 实时行来源标签：desktop 端 nvidia-smi 缺席时退 AMD atiadlxx ADL，
// gpu_live.name 决定标签（AMD→ADL 实时，否则 nvidia-smi 实时；无 live→空）。
describe('gpuLiveSource（硬件报告 GPU 来源标签）', () => {
  // 断言的是**界面译文**：语言由 navigator.language 兜底解析，runner 环境不同
  // 就会红。钉死中文，让这条用例与本机/CI 的系统语言无关。
  beforeAll(() => setLang('zh'));

  it('AMD 卡（ADL 兜底 name）→ atiadlxx ADL 实时', () => {
    expect(gpuLiveSource({ name: 'AMD 显卡（ADL 适配器 1）', temperature_c: 58 }))
      .toBe('atiadlxx ADL 实时');
  });

  it('NVIDIA 卡 → nvidia-smi 实时', () => {
    expect(gpuLiveSource({ name: 'NVIDIA GeForce RTX 5090 D v2', temperature_c: 38 }))
      .toBe('nvidia-smi 实时');
  });

  it('无 gpu_live → 空串（回退「N 个传感器」）', () => {
    expect(gpuLiveSource()).toBe('');
    expect(gpuLiveSource({ name: undefined })).toBe('');
  });
});
