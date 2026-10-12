import { describe, expect, it, beforeAll } from 'vitest';
import { fmtHwTime, gpuLiveSource, vramStr } from './HwPanels';
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

// AdapterRAM 溢出判定回归：原条件 `n >= 4GiB || n <= 4GiB-1MiB` 第二支写反，
// 2GB 卡被标成「≥4GB 溢出」、u32 饱和值反落进 formatBytes（2026-10-11 修复）。
describe('vramStr（AdapterRAM 溢出区判定）', () => {
  beforeAll(() => setLang('zh'));

  it('正常显存走格式化，不标溢出', () => {
    // WMI AdapterRAM 是 UINT32，真实读数 <4GiB-1MiB 才可能来自真实显存；
    // ≥阈值一律是饱和值（真实 6GiB 也读成 u32 饱和，见下一条）。
    expect(vramStr(2 * 1024 ** 3)).toBe('2.00 GB'); // 2GB 卡——原 bug 会标「≥4GB 溢出」
    expect(vramStr(1024 ** 3)).toBe('1.00 GB');
    expect(vramStr(3 * 1024 ** 3 + 512 * 1024 ** 2)).toBe('3.50 GB');
  });

  it('u32 饱和区标「≥4GB（WMI 溢出）」', () => {
    expect(vramStr(4294967295)).toContain('≥4 GB'); // u32::MAX —— 原 bug 反而 formatBytes
    expect(vramStr(4294967294)).toContain('≥4 GB');
    expect(vramStr(4293918720)).toContain('≥4 GB'); // 4GiB-1MiB 阈值
  });

  it('0/负/非数字 → 未知', () => {
    expect(vramStr(0)).toBe('未知');
    expect(vramStr(-1)).toBe('未知');
    expect(vramStr('n/a')).toBe('未知');
  });
});
