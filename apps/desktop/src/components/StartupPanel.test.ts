import { describe, expect, it } from 'vitest';
import type { StartupItem } from '../api';
import { filterStartupItems, groupStartupByLocation, startupCounts } from './StartupPanel';

const mk = (over: Partial<StartupItem>): StartupItem => ({
  id: 'x',
  name: 'X',
  command: '',
  location: 'registry_hkcu',
  enabled: true,
  target: '',
  ...over,
});

const items: StartupItem[] = [
  mk({ id: 'a', name: 'WeChat', command: 'C:\\WeChat\\WeChat.exe', enabled: true }),
  mk({ id: 'b', name: 'Steam', command: 'C:\\Steam\\steam.exe', enabled: false }),
  mk({ id: 'c', name: 'NVIDIA', target: 'nvcpl.dll', enabled: true }),
];

// R164：启动项筛选——几十项时逐行找太累。
describe('filterStartupItems（启动项筛选）', () => {
  it('空词 + all → 原样返回', () => {
    expect(filterStartupItems(items, '', 'all')).toHaveLength(3);
    expect(filterStartupItems(items, '   ', 'all')).toHaveLength(3);
  });

  it('状态筛：enabled / disabled', () => {
    expect(filterStartupItems(items, '', 'enabled').map((x) => x.id)).toEqual(['a', 'c']);
    expect(filterStartupItems(items, '', 'disabled').map((x) => x.id)).toEqual(['b']);
  });

  it('搜索匹配 name / command / target，大小写不敏感', () => {
    expect(filterStartupItems(items, 'wechat', 'all').map((x) => x.id)).toEqual(['a']);
    expect(filterStartupItems(items, 'STEAM.EXE', 'all').map((x) => x.id)).toEqual(['b']);
    expect(filterStartupItems(items, 'nvcpl', 'all').map((x) => x.id)).toEqual(['c']);
  });

  it('搜索与状态筛可叠加', () => {
    expect(filterStartupItems(items, 'wechat', 'disabled')).toHaveLength(0);
    expect(filterStartupItems(items, 'wechat', 'enabled').map((x) => x.id)).toEqual(['a']);
  });

  it('不改入参数组（纯函数）', () => {
    const input = [...items];
    filterStartupItems(input, 'steam', 'all');
    expect(input).toHaveLength(3);
  });
});

// R164：启用/停用计数——顶部一句看清「开机自启几项」。
describe('startupCounts（启用统计）', () => {
  it('分别计数', () => {
    expect(startupCounts(items)).toEqual({ on: 2, off: 1 });
  });

  it('空列表 → 全 0', () => {
    expect(startupCounts([])).toEqual({ on: 0, off: 0 });
  });

  it('全开 / 全关', () => {
    expect(startupCounts([mk({ enabled: true }), mk({ enabled: true })])).toEqual({ on: 2, off: 0 });
    expect(startupCounts([mk({ enabled: false })])).toEqual({ on: 0, off: 1 });
  });
});

// R165：来源分组——用户级在前、机器级在后（后者影响全机，风险更高）。
describe('groupStartupByLocation（来源分组）', () => {
  const mixed: StartupItem[] = [
    mk({ id: 'm1', location: 'registry_hklm' }),
    mk({ id: 'u1', location: 'registry_hkcu' }),
    mk({ id: 'f1', location: 'folder_machine' }),
    mk({ id: 'u2', location: 'folder_user' }),
    mk({ id: 'x1', location: 'weird_source' }),
  ];

  it('按固定顺序出组：HKCU → 用户夹 → HKLM → 公共夹 → 其他', () => {
    expect(groupStartupByLocation(mixed).map((g) => g.location)).toEqual([
      'registry_hkcu',
      'folder_user',
      'registry_hklm',
      'folder_machine',
      'other',
    ]);
  });

  it('组内保持原顺序；未知来源归 other（不丢项）', () => {
    const g = groupStartupByLocation(mixed);
    expect(g[0].items.map((x) => x.id)).toEqual(['u1']);
    expect(g[4].items.map((x) => x.id)).toEqual(['x1']);
  });

  it('空输入 → 空数组；没有的组不出现', () => {
    expect(groupStartupByLocation([])).toEqual([]);
    expect(groupStartupByLocation([mk({ id: 'a', location: 'registry_hkcu' })])).toHaveLength(1);
  });

  it('不改入参数组（纯函数）', () => {
    const input = [...mixed];
    groupStartupByLocation(input);
    expect(input.map((x) => x.id)).toEqual(['m1', 'u1', 'f1', 'u2', 'x1']);
  });
});