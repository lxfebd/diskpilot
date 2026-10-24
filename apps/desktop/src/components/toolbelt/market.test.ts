import { describe, expect, it } from 'vitest';
import { communityComingSoon, emptiedAllDedupe, communityListState, sortInstalledPlugins, versionNotes, verifyCounts, verifyRows } from './market';
import type { RegistryPlugin, RegistryVerifyOut } from '../../api';

function p(partial: Partial<RegistryPlugin>): RegistryPlugin {
  return {
    id: 'x',
    name: 'X',
    version: '1.0.0',
    author: '',
    description: '',
    category: '',
    risk: '',
    tags: [],
    url: '',
    sha256: '',
    signer: '',
    downloads: 0,
    ...partial,
  };
}

// 社区索引「全占位」判断：有条目但无一提供下载地址（等待上架引导）。
describe('communityComingSoon（索引全占位判断）', () => {
  it('索引已接入且全部条目 url 空 → true', () => {
    const c = [p({ url: '' }), p({ url: '' })];
    expect(communityComingSoon(c, true)).toBe(true);
  });

  it('有可装条目（url 非空）→ false', () => {
    const c = [p({ url: 'https://example.com/a.zip' }), p({ url: '' })];
    expect(communityComingSoon(c, true)).toBe(false);
  });

  it('空列表 → false（空市场走 communityEmpty 文案）', () => {
    expect(communityComingSoon([], true)).toBe(false);
  });

  it('未接入索引 → false', () => {
    const c = [p({ url: '' })];
    expect(communityComingSoon(c, false)).toBe(false);
  });
});

// 空列表是否源于「内置去重」：索引接入成功但条目全被 filter_builtin_dupes 隐藏。
// 此情形应给"索引接入成功但全内置"引导，而非冷冰冰的「社区注册表为空」。
describe('emptiedAllDedupe（空列表源于内置去重判断）', () => {
  it('索引接入 + 空列表 + 有隐藏 → true（全去重）', () => {
    expect(emptiedAllDedupe([], 30, true)).toBe(true);
  });

  it('索引接入 + 空列表 + 无隐藏 → false（真没条目）', () => {
    expect(emptiedAllDedupe([], 0, true)).toBe(false);
  });

  it('未接入索引 → false', () => {
    expect(emptiedAllDedupe([], 30, false)).toBe(false);
  });

  it('列表非空 → false（有展示条目不算全去重）', () => {
    expect(emptiedAllDedupe([p({ url: '' })], 5, true)).toBe(false);
  });
});

// R70 诚实性回归：社区列表三态——读失败（null + warn）必须显示「未读到」，
// 绝不折叠成 [] 伪装「社区为空」。未读到 ≠ 空。
describe('communityListState（社区列表三态）', () => {
  it('null 无 warn → loading', () => {
    expect(communityListState(null, null)).toBe('loading');
  });

  it('null + warn → unread（读失败 ≠ 空市场）', () => {
    expect(communityListState(null, 'backend down')).toBe('unread');
  });

  it('空数组（成功读到 0 项）→ ready（真空空市场）', () => {
    expect(communityListState([], null)).toBe('ready');
  });

  it('有数据 → ready', () => {
    expect(communityListState([p({ url: 'https://example.com/a.zip' })], null)).toBe('ready');
  });
});

// R162：已装台账排序——有新版优先，其余按安装时间倒序。
describe('sortInstalledPlugins（已装台账排序）', () => {
  const mk = (id: string, name: string, installed_at: number) => ({ id, name, installed_at });
  const plist = [
    mk('old', 'A-old', 100),
    mk('new', 'B-new', 300),
    mk('mid', 'C-mid', 200),
  ];

  it('无新版时按安装时间倒序（最近装的在前）', () => {
    expect(sortInstalledPlugins(plist, () => false).map((x) => x.id)).toEqual(['new', 'mid', 'old']);
  });

  it('有新版的一律排最前（哪怕装得最早）', () => {
    expect(
      sortInstalledPlugins(plist, (x) => x.id === 'old').map((x) => x.id),
    ).toEqual(['old', 'new', 'mid']);
  });

  it('多个有新版时内部仍按安装时间倒序', () => {
    expect(
      sortInstalledPlugins(plist, (x) => x.id !== 'mid').map((x) => x.id),
    ).toEqual(['new', 'old', 'mid']);
  });

  it('不改入参数组（纯函数）', () => {
    const input = [mk('a', 'A', 100), mk('b', 'B', 200)];
    sortInstalledPlugins(input, () => false);
    expect(input.map((x) => x.id)).toEqual(['a', 'b']);
  });
});

// R163：取某版本的更新说明——决定更新前该看到「这版改了什么」。
describe('versionNotes（更新说明提取）', () => {
  const plug = {
    version: '1.3.0',
    versions: [
      { version: '1.2.0', notes: '修了导出崩溃' },
      { version: '1.3.0', notes: '新增批量模式' },
      { version: '1.4.0' },
    ],
  };

  it('缺省取插件当前版本（= 待更新的新版）的说明', () => {
    expect(versionNotes(plug)).toBe('新增批量模式');
  });

  it('可指定版本取对应说明', () => {
    expect(versionNotes(plug, '1.2.0')).toBe('修了导出崩溃');
  });

  it('版本无 notes / 版本对不上 / 无 versions → null（不编造）', () => {
    expect(versionNotes(plug, '1.4.0')).toBeNull();
    expect(versionNotes(plug, '9.9.9')).toBeNull();
    expect(versionNotes({ version: '1.0.0' })).toBeNull();
  });

  it('空白 notes 视为无说明', () => {
    expect(versionNotes({ version: '1.0.0', versions: [{ version: '1.0.0', notes: '   ' }] })).toBeNull();
  });
});

type VerifyItem = RegistryVerifyOut['items'][number];

function vItem(partial: Partial<VerifyItem>): VerifyItem {
  return {
    id: 'x',
    name: 'X',
    version: '1.0.0',
    has_url: true,
    https_url: true,
    has_sha256: true,
    has_signer: true,
    verified: true,
    issue: null,
    ...partial,
  };
}

describe('索引校验结果的计数（R345 verifyCounts）', () => {
  it('全部字段完备 → ok 等于总数、noSha 与 issue 为 0', () => {
    const c = verifyCounts([vItem({ id: 'a' }), vItem({ id: 'b' }), vItem({ id: 'c' })]);
    expect(c).toEqual({ total: 3, issue: 0, noSha: 0, ok: 3 });
  });

  it('有 issue 的条目不计入 ok，但仍计入 total', () => {
    const c = verifyCounts([vItem({ id: 'a' }), vItem({ id: 'b', issue: '下载地址必须 https' })]);
    expect(c).toEqual({ total: 2, issue: 1, noSha: 0, ok: 1 });
  });

  it('未声明 sha256 的单列一类（能装但装完无法比对哈希）', () => {
    const c = verifyCounts([vItem({ id: 'a', has_sha256: false }), vItem({ id: 'b' })]);
    expect(c).toEqual({ total: 2, issue: 0, noSha: 1, ok: 1 });
  });

  it('issue 与 noSha 可同时命中同一条；空 / null / undefined 全部为 0', () => {
    expect(verifyCounts([vItem({ has_sha256: false, issue: '未提供签名者公钥' })])).toEqual({ total: 1, issue: 1, noSha: 1, ok: 0 });
    expect(verifyCounts([])).toEqual({ total: 0, issue: 0, noSha: 0, ok: 0 });
    expect(verifyCounts(null)).toEqual({ total: 0, issue: 0, noSha: 0, ok: 0 });
    expect(verifyCounts(undefined)).toEqual({ total: 0, issue: 0, noSha: 0, ok: 0 });
  });
});

describe('索引校验要列出来的条目（R345 verifyRows）', () => {
  it('字段完备的条目不列，保持后端顺序只列有问题的', () => {
    const rows = verifyRows([
      vItem({ id: 'ok1' }),
      vItem({ id: 'bad', name: '坏插件', issue: '下载地址必须 https' }),
      vItem({ id: 'ok2' }),
      vItem({ id: 'nosha', name: '无哈希' }),
    ]);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toEqual({ id: 'bad', name: '坏插件', version: '1.0.0', kind: 'issue', detail: '下载地址必须 https' });
  });

  it('只缺 sha256 的条目单独成行，detail 留空由文案补', () => {
    const rows = verifyRows([vItem({ id: 'a', name: 'A', has_sha256: false })]);
    expect(rows).toEqual([{ id: 'a', name: 'A', version: '1.0.0', kind: 'noSha', detail: '' }]);
  });

  it('同一条既缺 url 又缺 sha256 只出一行（issue 优先），不重复念', () => {
    const rows = verifyRows([vItem({ id: 'a', has_url: false, has_sha256: false, issue: '尚未提供下载地址（即将上架）' })]);
    expect(rows).toHaveLength(1);
    expect(rows[0].kind).toBe('issue');
  });

  it('空白 issue 视为没问题；空集返回空数组', () => {
    expect(verifyRows([vItem({ issue: '   ' })])).toEqual([]);
    expect(verifyRows([])).toEqual([]);
  });
});
