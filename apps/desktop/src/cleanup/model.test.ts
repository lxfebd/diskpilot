import { describe, it, expect } from 'vitest';
import {
  aggregateScopeSizes,
  applySelectionMode,
  collectWxids,
  computeCoverage,
  computeTotalSelected,
  defaultDaysFor,
  detectVariants,
  scopeDayOverrides,
  type ScopeSize,
} from './model';
import type { Node, Scope } from '../types';

// 纯逻辑单测：node 环境，不渲染组件。model.ts 里所有调用 t() 的函数
// （formatLastActive 等）不在这批测试范围内——它们依赖 i18n 运行时，
// 而 i18n 的中文回退语义另有 coverage.test.ts 守卫。

function node(partial: Partial<Node>): Node {
  return {
    name: partial.name ?? 'x',
    path: partial.path ?? 'C:\\x',
    is_dir: partial.is_dir ?? true,
    size: partial.size ?? 0,
    file_count: partial.file_count ?? 0,
    children: partial.children ?? [],
    scaffold_id: partial.scaffold_id,
    top_extensions: partial.top_extensions ?? [],
    children_truncated: partial.children_truncated,
  };
}

function sz(scope_id: string, bytes: number, total = bytes): ScopeSize {
  return { scope_id, bytes, file_count: bytes, total_bytes: total, total_files: total };
}

describe('aggregateScopeSizes', () => {
  it('同 scope_id 跨多根累加', () => {
    const out = aggregateScopeSizes([
      [sz('a', 10, 20), sz('b', 5, 5)],
      [sz('a', 3, 7)],
    ]);
    const a = out.find((r) => r.scope_id === 'a')!;
    const b = out.find((r) => r.scope_id === 'b')!;
    expect(a.bytes).toBe(13);
    expect(a.total_bytes).toBe(27);
    expect(b.bytes).toBe(5);
    expect(b.total_bytes).toBe(5);
  });

  it('空输入返空', () => {
    expect(aggregateScopeSizes([])).toEqual([]);
  });
});

describe('detectVariants', () => {
  it('xwechat 路径 → 4.x', () => {
    const v = detectVariants([node({ path: 'C:\\xwechat_files\\abc' })]);
    expect(v.has('4.x')).toBe(true);
    expect(v.has('3.x')).toBe(false);
  });

  it('wechat files 路径 → 3.x', () => {
    const v = detectVariants([node({ path: 'D:/WeChat Files/wxid_x' })]);
    expect(v.has('3.x')).toBe(true);
    expect(v.has('4.x')).toBe(false);
  });

  it('两者都有 → 两个变体都进集合', () => {
    const v = detectVariants([
      node({ path: 'C:\\tencent\\xwechat\\a' }),
      node({ path: 'C:\\tencent\\wechat\\b' }),
    ]);
    expect(v.size).toBe(2);
  });
});

describe('collectWxids', () => {
  it('收齐 wxid_ 前缀子目录，去重升序', () => {
    const m = node({
      children: [
        node({ name: 'wxid_bbb', is_dir: true }),
        node({ name: 'wxid_aaa', is_dir: true }),
        node({ name: 'wxid_aaa', is_dir: true }),
        node({ name: 'not_wxid', is_dir: true }),
        node({ name: 'file.txt', is_dir: false }),
      ],
    });
    expect(collectWxids([m])).toEqual(['wxid_aaa', 'wxid_bbb']);
  });

  it('文件不收', () => {
    const m = node({ children: [node({ name: 'wxid_x', is_dir: false })] });
    expect(collectWxids([m])).toEqual([]);
  });
});

describe('scopeDayOverrides', () => {
  it('只保留与默认不同的项', () => {
    const overrides = scopeDayOverrides({ a: 30, b: 90, c: 7 }, { a: 30, b: 30, c: 7 });
    expect(overrides).toEqual({ b: 90 });
  });

  it('全默认 → 空对象（持久化时删条目）', () => {
    const d = { a: 30 };
    expect(scopeDayOverrides(d, d)).toEqual({});
  });
});

describe('defaultDaysFor', () => {
  it('只收 kind=days 的 scope', () => {
    const scopes: Scope[] = [
      { id: 'a', label: 'A', glob: '*', mode: 'recycle', prompt: { kind: 'days', default: 30 } },
      { id: 'b', label: 'B', glob: '*', mode: 'recycle', prompt: { kind: 'none' } },
      { id: 'c', label: 'C', glob: '*', mode: 'recycle', prompt: { kind: 'days', default: 90 } },
    ];
    expect(defaultDaysFor(scopes)).toEqual({ a: 30, c: 90 });
  });

  it('undefined → 空表', () => {
    expect(defaultDaysFor(undefined)).toEqual({});
  });
});

describe('computeTotalSelected', () => {
  it('非 conda：只数 bytes>0 的勾选项', () => {
    const sizes = [sz('a', 100), sz('b', 0), sz('c', 50)];
    const r = computeTotalSelected({
      isConda: false,
      selectedScopes: new Set(['a', 'b', 'c']),
      scopeSizes: sizes,
    });
    // b 是 0 字节，不计入 count
    expect(r).toEqual({ count: 2, bytes: 150 });
  });

  it('conda：环境 + 包缓存 scope 合计', () => {
    const sizes = [sz('tarballs', 200), sz('unused-packages', 80)];
    const r = computeTotalSelected({
      isConda: true,
      selectedScopes: new Set(['tarballs', 'unused-packages']),
      scopeSizes: sizes,
      condaEnvs: [
        { name: 'env1', path: 'p', size_bytes: 500, last_active_ts: null, is_base: false, default_checked: true },
      ],
      selectedEnvs: new Set(['env1']),
    });
    // 1 env (500) + 2 包缓存 (200+80)
    expect(r).toEqual({ count: 3, bytes: 780 });
  });

  it('conda 包缓存为 0 时不计入', () => {
    const r = computeTotalSelected({
      isConda: true,
      selectedScopes: new Set(['tarballs']),
      scopeSizes: [sz('tarballs', 0)],
      condaEnvs: [],
      selectedEnvs: new Set(),
    });
    expect(r).toEqual({ count: 0, bytes: 0 });
  });
});

describe('computeCoverage', () => {
  it('conda 恒 null', () => {
    expect(computeCoverage(true, [], null)).toBeNull();
  });

  it('folderTotal=0 → null（避免 0/0 误导）', () => {
    expect(computeCoverage(false, [node({ size: 0 })], null)).toBeNull();
  });

  it('正常：差额 = 文件夹总量 - scope 内总量', () => {
    const matches = [node({ size: 1000 })];
    const sizes = [sz('a', 300, 400)]; // total_bytes=400
    const c = computeCoverage(false, matches, sizes)!;
    expect(c.folderTotal).toBe(1000);
    expect(c.inScope).toBe(400);
    expect(c.outsideScope).toBe(600);
  });
});

describe('applySelectionMode', () => {
  type Item = { k: string; safe: boolean };
  const items: Item[] = [
    { k: 'a', safe: true },
    { k: 'b', safe: false },
    { k: 'c', safe: true },
  ];
  const eligible = (_it: Item) => true;
  const keyOf = (it: Item) => it.k;
  const isSafe = (it: Item) => it.safe;

  it('none → 空集', () => {
    expect(applySelectionMode('none', items, eligible, keyOf, isSafe).size).toBe(0);
  });

  it('all → 全部合格项', () => {
    const s = applySelectionMode('all', items, eligible, keyOf, isSafe);
    expect([...s].sort()).toEqual(['a', 'b', 'c']);
  });

  it('safe → 只勾 isSafe 的', () => {
    const s = applySelectionMode('safe', items, eligible, keyOf, isSafe);
    expect([...s].sort()).toEqual(['a', 'c']);
  });

  it('不合格项不计入（即使 safe）', () => {
    const s = applySelectionMode('all', items, (it) => it.k !== 'b', keyOf, isSafe);
    expect([...s].sort()).toEqual(['a', 'c']);
  });
});
