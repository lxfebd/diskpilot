import { describe, it, expect } from 'vitest';
import { fallbackByNameContains, collectScaffoldCards, type ScaffoldCard } from './matches';
import type { Node, Scaffold } from '../types';

// 目录树工厂：默认空目录，dir(name, children, scaffoldId, size, fileCount)。
const dir = (name: string, children: Node[] = [], scaffoldId?: string | null, size = 0, fileCount = 0): Node => ({
  name, path: '/' + name, is_dir: true, size, file_count: fileCount, children,
  top_extensions: [], scaffold_id: scaffoldId ?? null,
});
const file = (name: string, size = 0, fileCount = 0): Node => ({
  name, path: '/' + name, is_dir: false, size, file_count: fileCount, children: [],
  top_extensions: [], scaffold_id: null,
});

const sc = (id: string, name: string, over: Partial<Scaffold> = {}): Scaffold => ({
  id, name, risk: 'low', disclaimer: '', detect: [], match: {}, scopes: [], ...over,
});

describe('fallbackByNameContains', () => {
  it('name_contains 为空时返回空数组（连匹配名字也不查）', () => {
    const root = dir('root', [dir('Cache', [])]);
    expect(fallbackByNameContains(root, sc('a', 'A'))).toEqual([]);
  });

  it('大小写不敏感子串命中，命中后不再下钻（subCache 藏在命中目录里）', () => {
    const root = dir('root', [dir('Cache', [dir('subCache', [])])]);
    const r = fallbackByNameContains(root, sc('a', 'A', { match: { name_contains: ['cache'] } }));
    expect(r.map((n) => n.name)).toEqual(['Cache']);
  });

  it('下钻深度上限：第 4 层命中、第 5 层不命中（root 为第 0 层）', () => {
    // 深度 4：root(0)→d2(1)→d3(2)→d4(3)→Cache(4) 命中
    const root = dir('root', [dir('d2', [dir('d3', [dir('d4', [dir('Cache', [])])])])]);
    expect(fallbackByNameContains(root, sc('a', 'A', { match: { name_contains: ['cache'] } })).map((n) => n.name)).toEqual(['Cache']);
    // 深度 5：root(0)→d2(1)→d3(2)→d4(3)→d5(4)→Cache(5) 不再下钻
    const root5 = dir('root', [dir('d2', [dir('d3', [dir('d4', [dir('d5', [dir('Cache', [])])])])])]);
    expect(fallbackByNameContains(root5, sc('a', 'A', { match: { name_contains: ['cache'] } })).map((n) => n.name)).toEqual([]);
  });

  it('root 自身（第 0 层）永不参与名字命中', () => {
    const root = dir('CacheRoot', [dir('Cache', [])]);
    const r = fallbackByNameContains(root, sc('a', 'A', { match: { name_contains: ['cache'] } }));
    expect(r.map((n) => n.name)).toEqual(['Cache']);
  });

  it('非目录节点不参与命中', () => {
    const root = dir('root', [file('Cache.exe')]);
    const r = fallbackByNameContains(root, sc('a', 'A', { match: { name_contains: ['cache'] } }));
    expect(r).toEqual([]);
  });

  it('多关键字任一命中即收，互不干扰', () => {
    const root = dir('root', [dir('One', []), dir('Two', [])]);
    const r = fallbackByNameContains(root, sc('a', 'A', { match: { name_contains: ['one', 'two'] } }));
    expect(r.map((n) => n.name).sort()).toEqual(['One', 'Two']);
  });
});

describe('collectScaffoldCards', () => {
  it('root 为 null 时返回全部脚本卡片、命中为空、按名字排序（未扫描也能看到能力清单）', () => {
    const cards = collectScaffoldCards(null, [sc('b', 'Beta'), sc('a', 'Alpha')]);
    expect(cards.map((c) => c.scaffold.name)).toEqual(['Alpha', 'Beta']);
    for (const c of cards) {
      expect(c.matches).toEqual([]);
      expect(c.totalSize).toBe(0);
      expect(c.totalFiles).toBe(0);
    }
  });

  it('重复出现的 scaffold id 只保留首次出现的一张卡', () => {
    const cards = collectScaffoldCards(null, [sc('x', 'X'), sc('x', 'Dup')]);
    expect(cards.length).toBe(1);
    expect(cards[0].scaffold.name).toBe('X');
  });

  it('detect 命中：目录带 scaffold_id 即计入，root 自身也算', () => {
    const root = dir('root', [dir('DetectDir', [], 's1'), file('f')], 's1');
    const cards = collectScaffoldCards(root, [sc('s1', 'S1')]);
    expect(cards[0].matches.map((n) => n.name).sort()).toEqual(['DetectDir', 'root']);
  });

  it('R228：命中后继续下钻，嵌套目录各算各的脚本，互不重复', () => {
    const nested = dir('Nested', [], 's2');
    const root = dir('root', [dir('A', [nested], 's1')]);
    const cards = collectScaffoldCards(root, [sc('s1', 'S1'), sc('s2', 'S2')]);
    const s1 = cards.find((c) => c.scaffold.id === 's1')!;
    const s2 = cards.find((c) => c.scaffold.id === 's2')!;
    expect(s1.matches.map((n) => n.name)).toEqual(['A']);
    expect(s2.matches.map((n) => n.name)).toEqual(['Nested']);
  });

  it('detect 无命中时才走名字回退，回退结果参与统计', () => {
    const root = dir('root', [dir('CacheDir', []), dir('Other', [])]);
    const cards = collectScaffoldCards(root, [sc('s1', 'S1', { match: { name_contains: ['cache'] } })]);
    expect(cards[0].matches.map((n) => n.name)).toEqual(['CacheDir']);
  });

  it('detect 已有命中时不再叠加名字回退', () => {
    const root = dir('root', [dir('DetectDir', [], 's1'), dir('AlsoCache', [])]);
    const cards = collectScaffoldCards(root, [sc('s1', 'S1', { match: { name_contains: ['cache'] } })]);
    expect(cards[0].matches.map((n) => n.name)).toEqual(['DetectDir']);
  });

  it('卡片合计 = 命中目录体量与文件数求和（size 降序排列命中）', () => {
    const root = dir('root', [
      dir('Big', [file('f', 200, 3)], 's1', 200, 3),
      dir('Small', [file('f', 50, 1)], 's1', 50, 1),
    ]);
    const cards = collectScaffoldCards(root, [sc('s1', 'S1')]);
    const c = cards[0];
    expect(c.matches.map((n) => n.name)).toEqual(['Big', 'Small']);
    expect(c.totalSize).toBe(250);
    expect(c.totalFiles).toBe(4);
  });

  it('最终排序：有命中在前，其次体量降序，再按名字', () => {
    const root = dir('root', [dir('B', [], 's-b'), dir('A', [], 's-a')]);
    const cards = collectScaffoldCards(root, [sc('s-x', 'X'), sc('s-a', 'A'), sc('s-b', 'B')]);
    expect(cards.map((c) => c.scaffold.name)).toEqual(['A', 'B', 'X']);
  });
});

// 类型哨兵：确认 ScaffoldCard 结构可用（编译期验证导出的 interface）。
const _cardCheck: ScaffoldCard[] = [];
void _cardCheck;
