import { describe, expect, it } from 'vitest';
import { driverClassCounts, driverOnlyUpdates, fanChannelState, filterDrivers, wuUpdateCounts } from './SystemTools';
import type { FanStatus, FanProbe, InstalledDriver, DriverUpdate } from '../api';

// R71 诚实性回归：风扇可调通道三态——「状态探测失败」绝不能被渲染成
// 「当前无可用调速通道」。状态未读到 ≠ 不存在（同一族：未读到 ≠ 空）。
const PROBE: FanProbe = {
  channel: 'cli',
  writable: false,
  mode: 'cli',
  reason: '',
  cli: { name: 'fancmd', exe: 'fancmd.exe', exe_path: 'C:\\fancmd.exe', subcmd_tpl: '' },
};

function status(partial: Partial<FanStatus> = {}): FanStatus {
  return {
    probe: PROBE,
    temp_c: null,
    writable: false,
    has_snapshot: false,
    guard_running: false,
    note: '',
    ...partial,
  };
}

describe('fanChannelState（风扇可调通道三态）', () => {
  it('可写通道在位 → ok（无论 err 是否残留）', () => {
    expect(fanChannelState(status({ writable: true }), null)).toBe('ok');
    // 刷新成功后旧错误要能翻回 ok（新读取覆盖旧错误）
    expect(fanChannelState(status({ writable: true }), '旧错误')).toBe('ok');
  });

  it('探测失败（有 err）→ unread，绝不落到 none', () => {
    expect(fanChannelState(null, 'backend down')).toBe('unread');
    expect(fanChannelState(status({ writable: false }), 'no sensor')).toBe('unread');
  });

  it('探测成功但无通道（无 err、不可写）→ none（真空语义）', () => {
    expect(fanChannelState(null, null)).toBe('none');
    expect(fanChannelState(status({ writable: false }), null)).toBe('none');
  });
});

// R166：已装驱动筛选与分类计数——200 条上限下列表很长，按名找很难。
describe('filterDrivers（驱动筛选）', () => {
  const mk = (over: Partial<InstalledDriver>): InstalledDriver => ({
    name: 'X',
    provider: '',
    version: '1.0.0',
    date: '',
    class: '',
    ...over,
  });
  const list: InstalledDriver[] = [
    mk({ name: 'NVIDIA GeForce RTX 4070', provider: 'NVIDIA', version: '31.0.15', class: 'Display' }),
    mk({ name: 'Realtek Audio', provider: 'Realtek', version: '6.0.9', class: 'Media' }),
    mk({ name: 'Intel Wi-Fi', provider: 'Intel', version: '22.1.0', class: 'Net' }),
  ];

  it('空词 + null 分类 → 原样返回', () => {
    expect(filterDrivers(list, '', null)).toHaveLength(3);
    expect(filterDrivers(list, '   ', null)).toHaveLength(3);
  });

  it('分类筛选', () => {
    expect(filterDrivers(list, '', 'Display').map((d) => d.name)).toEqual(['NVIDIA GeForce RTX 4070']);
  });

  it('搜索匹配 name / provider / version，大小写不敏感', () => {
    expect(filterDrivers(list, 'geforce', null)).toHaveLength(1);
    expect(filterDrivers(list, 'REALTEK', null)).toHaveLength(1);
    expect(filterDrivers(list, '22.1.0', null)).toHaveLength(1);
  });

  it('搜索与分类可叠加；不改入参（纯函数）', () => {
    expect(filterDrivers(list, 'intel', 'Net').map((d) => d.name)).toEqual(['Intel Wi-Fi']);
    expect(filterDrivers(list, 'intel', 'Display')).toHaveLength(0);
    const input = [...list];
    filterDrivers(input, 'nvidia', null);
    expect(input).toHaveLength(3);
  });
});

describe('driverClassCounts（分类计数）', () => {
  const mk = (cls: string): InstalledDriver => ({
    name: 'X', provider: '', version: '', date: '', class: cls,
  });

  it('按条数降序，同数按名称升序', () => {
    expect(driverClassCounts([mk('Net'), mk('Display'), mk('Display')])).toEqual([
      { cls: 'Display', n: 2 },
      { cls: 'Net', n: 1 },
    ]);
  });

  it('空 class 归到空串键（同数时空串按字典序在前）', () => {
    expect(driverClassCounts([mk(''), mk('Net')])).toEqual([
      { cls: '', n: 1 },
      { cls: 'Net', n: 1 },
    ]);
  });

  it('空列表 → 空数组', () => {
    expect(driverClassCounts([])).toEqual([]);
  });
});

// R167：Windows Update 可选更新里 is_driver 此前未被使用——只看驱动的人要能筛。
describe('driverOnlyUpdates / wuUpdateCounts（可选更新筛选）', () => {
  const mk = (over: Partial<DriverUpdate>): DriverUpdate => ({
    title: 'X',
    kb: '',
    driver_provider: '',
    driver_version: '',
    category: '',
    is_driver: false,
    ...over,
  });
  const list: DriverUpdate[] = [
    mk({ title: 'NVIDIA 驱动更新', is_driver: true }),
    mk({ title: '语言包', is_driver: false }),
    mk({ title: 'Intel 网卡驱动', is_driver: true }),
  ];

  it('只挑 is_driver', () => {
    expect(driverOnlyUpdates(list).map((u) => u.title)).toEqual(['NVIDIA 驱动更新', 'Intel 网卡驱动']);
  });

  it('计数分别为总数与驱动数', () => {
    expect(wuUpdateCounts(list)).toEqual({ total: 3, drivers: 2 });
  });

  it('空列表 → 空数组与全 0', () => {
    expect(driverOnlyUpdates([])).toEqual([]);
    expect(wuUpdateCounts([])).toEqual({ total: 0, drivers: 0 });
  });

  it('不改入参数组（纯函数）', () => {
    const input = [...list];
    driverOnlyUpdates(input);
    expect(input).toHaveLength(3);
  });
});