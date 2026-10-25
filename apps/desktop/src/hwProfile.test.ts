import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// 可变的模块级开关：isTauri 用 getter，探针用 vi.fn，便于逐用例改写。
const hoisted = vi.hoisted(() => ({ isTauri: true, probe: vi.fn() }));

vi.mock('./env', () => ({
  get isTauri() {
    return hoisted.isTauri;
  },
}));
vi.mock('./hwCache', () => ({ ensureSysProbe: hoisted.probe }));

import { getHwProfile, tierLabel, recommendLocal, perfContextLine } from './hwProfile';
import { setLang } from './i18n';

const GB = 1024 ** 3;

beforeEach(() => {
  hoisted.isTauri = true;
  hoisted.probe.mockReset();
  setLang('en'); // node 环境无 navigator，默认即 en；显式固定保证用例互不影响
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('tierLabel', () => {
  it('三档各自对应文案（英文）', () => {
    expect(tierLabel('weak')).toBe('low-end');
    expect(tierLabel('medium')).toBe('mid-range');
    expect(tierLabel('strong')).toBe('high-end');
  });

  it('切中文后跟随中文文案（R350 语言即时生效）', () => {
    setLang('zh');
    try {
      expect(tierLabel('weak')).toBe('低配');
      expect(tierLabel('medium')).toBe('中配');
      expect(tierLabel('strong')).toBe('高配');
    } finally {
      setLang('en');
    }
  });
});

describe('recommendLocal', () => {
  it('弱机建议云端、中机允许本地、强机推荐本地', () => {
    expect(recommendLocal('weak')).toBe('suggest-cloud');
    expect(recommendLocal('medium')).toBe('allow-local');
    expect(recommendLocal('strong')).toBe('recommend-local');
  });
});

describe('getHwProfile 档位判定（后端真值优先）', () => {
  it('16 核 / 32 GB → 强档，fromBackend=true', async () => {
    hoisted.probe.mockResolvedValue({ cpu_cores: 16, mem_total_bytes: 32 * GB });
    const p = await getHwProfile(true);
    expect(p.tier).toBe('strong');
    expect(p.cores).toBe(16);
    expect(p.memGb).toBe(32);
    expect(p.fromBackend).toBe(true);
    expect(p.label).toContain('CPU 16 cores');
  });

  it('4 核即便内存 64 GB 也降为弱档（核数命中即降档）', async () => {
    hoisted.probe.mockResolvedValue({ cpu_cores: 4, mem_total_bytes: 64 * GB });
    expect((await getHwProfile(true)).tier).toBe('weak');
  });

  it('16 核但只有 6 GB 内存 → 弱档（内存命中即降档）', async () => {
    hoisted.probe.mockResolvedValue({ cpu_cores: 16, mem_total_bytes: 6 * GB });
    expect((await getHwProfile(true)).tier).toBe('weak');
  });

  it('6 核 / 14 GB → 中档', async () => {
    hoisted.probe.mockResolvedValue({ cpu_cores: 6, mem_total_bytes: 14 * GB });
    expect((await getHwProfile(true)).tier).toBe('medium');
  });

  it('后端探针失败时回退浏览器近似值，fromBackend=false', async () => {
    hoisted.probe.mockRejectedValue(new Error('probe down'));
    vi.stubGlobal('navigator', { hardwareConcurrency: 10, deviceMemory: 12 });
    const p = await getHwProfile(true);
    expect(p.fromBackend).toBe(false);
    expect(p.cores).toBe(10);
    expect(p.memGb).toBe(12);
    expect(p.tier).toBe('medium');
  });
});

describe('getHwProfile 浏览器近似值', () => {
  it('非 Tauri 环境用 hardwareConcurrency / deviceMemory 判档', async () => {
    hoisted.isTauri = false;
    vi.stubGlobal('navigator', { hardwareConcurrency: 4, deviceMemory: 16 });
    const p = await getHwProfile(true);
    expect(p.cores).toBe(4);
    expect(p.memGb).toBe(16);
    expect(p.tier).toBe('weak');
    expect(p.fromBackend).toBe(false);
  });

  it('拿不到任何信息时归为强档，label 用 ? 占位', async () => {
    hoisted.isTauri = false;
    vi.stubGlobal('navigator', {});
    const p = await getHwProfile(true);
    expect(p.cores).toBeNull();
    expect(p.memGb).toBeNull();
    expect(p.tier).toBe('strong');
    expect(p.label).toContain('?');
    expect(p.label).toContain('high-end');
  });
});

describe('缓存与 AI 上下文', () => {
  it('无 force 时复用缓存：同一次读取只探针一次', async () => {
    hoisted.probe.mockResolvedValue({ cpu_cores: 16, mem_total_bytes: 32 * GB });
    const first = await getHwProfile(true);
    const second = await getHwProfile();
    expect(second).toBe(first);
    expect(hoisted.probe.mock.calls.length).toBe(1);
  });

  it('perfContextLine 按档位给中文 prompt（@i18n-keep 不随界面语言变）', async () => {
    hoisted.probe.mockResolvedValue({ cpu_cores: 4, mem_total_bytes: 8 * GB });
    await getHwProfile(true);
    const weak = await perfContextLine();
    expect(weak).toContain('本机为低配');
    expect(weak).toContain('CPU 4 核');
    expect(weak).toContain('内存 8 GB');
    hoisted.probe.mockResolvedValue({ cpu_cores: 16, mem_total_bytes: 32 * GB });
    await getHwProfile(true);
    const strong = await perfContextLine();
    expect(strong).toContain('本机为高配');
    expect(strong).toContain('CPU 16 核');
  });
});
