import { describe, it, expect } from 'vitest';
import {
  aggregateScopeSizes,
  applySelectionMode,
  cleanRatio,
  collectWxids,
  computeCoverage,
  computeTotalSelected,
  condaDryBytes,
  defaultDaysFor,
  detectVariants,
  driveBreakdown,
  cleanedHitStats,
  lastCleanByPath,
  sampleEntriesByBytes,
  samplePathsByBytes,
  scopeDayOverrides,
  sortScopesByBytes,
  tallyDrive,
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

describe('condaDryBytes', () => {
  const envs = [
    { name: 'env1', path: 'C:\\conda\\envs\\env1', size_bytes: 500, last_active_ts: null, is_base: false, default_checked: true },
    { name: 'env2', path: 'C:\\conda\\envs\\env2', size_bytes: 800, last_active_ts: null, is_base: false, default_checked: true },
  ];

  it('只按 dry-run 命中的 env 累加，未命中的勾选 env 不计入', () => {
    // 勾了 env1+env2，但 dry-run 只命中 env1（env2 未过期）→ 只算 env1 的 500
    const r = condaDryBytes(['C:\\conda\\envs\\env1'], envs, [], new Set());
    expect(r).toEqual({ bytes: 500, count: 1 });
  });

  it('尾部斜杠路径也匹配得上元数据', () => {
    const r = condaDryBytes(['C:\\conda\\envs\\env1\\'], envs, [], new Set());
    expect(r.bytes).toBe(500);
  });

  it('包缓存 scope（tarballs/unused-packages）按 scopeSizes 缓存计入', () => {
    const sizes = [sz('tarballs', 200), sz('unused-packages', 80)];
    const r = condaDryBytes(['C:\\conda\\envs\\env2'], envs, sizes, new Set(['tarballs', 'unused-packages']));
    // env2(800) + tarballs(200) + unused-packages(80)
    expect(r).toEqual({ bytes: 1080, count: 3 });
  });

  it('未勾选的包缓存 scope 不计入', () => {
    const sizes = [sz('tarballs', 200)];
    const r = condaDryBytes([], envs, sizes, new Set());
    expect(r).toEqual({ bytes: 0, count: 0 });
  });

  it('命中路径对不上 envs 元数据（已消失/路径变了）→ 不计字节', () => {
    const r = condaDryBytes(['C:\\conda\\envs\\gone'], envs, [], new Set());
    expect(r).toEqual({ bytes: 0, count: 0 });
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

// 预览窗的「这批文件落在哪几块盘」：整批口径，不受 samplePaths 前 80 条截断影响。
describe('tallyDrive / driveBreakdown（清理会动到哪几块盘）', () => {
  it('按盘计数，文件数多的排前面', () => {
    const m = new Map<string, number>();
    for (const p of [
      'C:\\Users\\me\\Downloads\\a.tmp',
      'C:\\Users\\me\\Downloads\\b.tmp',
      'F:\\Media\\movie.mkv',
      'c:\\lower\\case\\ok.txt',
    ]) {
      tallyDrive(m, p);
    }
    expect(driveBreakdown(m)).toEqual([
      { letter: 'C', files: 3 },
      { letter: 'F', files: 1 },
    ]);
  });

  it('UNC / 相对路径不计入（没有本地物理盘）', () => {
    const m = new Map<string, number>();
    tallyDrive(m, '\\\\nas\\share\\f.txt');
    tallyDrive(m, 'relative\\path');
    tallyDrive(m, '');
    expect(driveBreakdown(m)).toEqual([]);
  });

  it('同文件数按字母升序（展示稳定）', () => {
    const m = new Map<string, number>([
      ['J', 5],
      ['C', 5],
      ['F', 5],
    ]);
    expect(driveBreakdown(m).map((d) => d.letter)).toEqual(['C', 'F', 'J']);
  });
});

// R130：dry-run 预览采样——全量命中按实际释放字节降序取前 cap 条（大文件优先）。
describe('samplePathsByBytes（预览路径按字节降序）', () => {
  const mk = (source: string, bytes_freed?: number | null) => ({ source, bytes_freed });
  it('按字节降序取前 cap 条——大文件优先展示', () => {
    expect(
      samplePathsByBytes(
        [mk('C:\\a\\1', 100), mk('C:\\b\\2', 50_000), mk('C:\\c\\3', 2_000)],
        2,
      ),
    ).toEqual(['C:\\b\\2', 'C:\\c\\3']);
  });
  it('bytes_freed 缺失的沉底且保持相对顺序', () => {
    expect(
      samplePathsByBytes(
        [mk('u1', null), mk('big', 9000), mk('u2', null), mk('small', 5)],
        10,
      ),
    ).toEqual(['big', 'small', 'u1', 'u2']);
  });
  it('不修改入参数组（纯函数）', () => {
    const input = [mk('a', 2), mk('b', 1)];
    samplePathsByBytes(input, 10);
    expect(input.map((i) => i.source)).toEqual(['a', 'b']);
  });
  it('默认 cap = DRY_RUN_SAMPLE_CAP（80）', () => {
    const many = Array.from({ length: 100 }, (_, i) => mk(`f${i}`, i));
    const out = samplePathsByBytes(many);
    expect(out).toHaveLength(80);
    expect(out[0]).toBe('f99');
  });
});

// R131：条目版本 sampleEntriesByBytes——预览窗行尾要显示单文件大小，
// 与 samplePathsByBytes 同排序规则但保留字节。
describe('sampleEntriesByBytes（预览条目按字节降序，保字节）', () => {
  it('降序取前 cap 条且保留字节', () => {
    const out = sampleEntriesByBytes(
      [
        { source: 'C:\\a\\1', bytes_freed: 100 },
        { source: 'C:\\b\\2', bytes_freed: 50_000 },
        { source: 'C:\\c\\3', bytes_freed: 2_000 },
      ],
      2,
    );
    expect(out).toEqual([
      { source: 'C:\\b\\2', bytes_freed: 50_000 },
      { source: 'C:\\c\\3', bytes_freed: 2_000 },
    ]);
  });
  it('bytes_freed 缺失的沉底且保持相对顺序', () => {
    const out = sampleEntriesByBytes(
      [
        { source: 'u1', bytes_freed: null },
        { source: 'big', bytes_freed: 9000 },
        { source: 'u2' },
        { source: 'small', bytes_freed: 5 },
      ],
      10,
    );
    expect(out.map((e) => e.source)).toEqual(['big', 'small', 'u1', 'u2']);
  });
  it('与 samplePathsByBytes 同序（路径数组 = 条目映射）', () => {
    const entries = [
      { source: 'a', bytes_freed: 2 },
      { source: 'b', bytes_freed: 9 },
      { source: 'c', bytes_freed: null },
    ];
    expect(sampleEntriesByBytes(entries, 10).map((e) => e.source)).toEqual(
      samplePathsByBytes(entries, 10),
    );
  });
  it('不修改入参数组（纯函数）', () => {
    const input = [{ source: 'a', bytes_freed: 2 }, { source: 'b', bytes_freed: 1 }];
    sampleEntriesByBytes(input, 10);
    expect(input.map((i) => i.source)).toEqual(['a', 'b']);
  });
});

// R156：按目录聚合「上次清理时刻」——scope 行在勾选前显示该目录多久没清。
describe('lastCleanByPath（上次清理时刻按目录聚合）', () => {
  const iso = (secs: number) => new Date(secs * 1000).toISOString();

  it('目录前缀匹配，取每个目录的最新时刻', () => {
    const out = lastCleanByPath(
      [
        { timestamp: iso(1000), source: 'C:\\App\\logs\\a.log' },
        { timestamp: iso(2000), source: 'C:\\App\\logs\\b.log' },
        { timestamp: iso(1500), source: 'C:\\App\\cache\\c.tmp' },
      ],
      ['C:\\App\\logs', 'C:\\App\\cache'],
    );
    expect(out).toEqual({ 'C:\\App\\logs': 2000, 'C:\\App\\cache': 1500 });
  });

  it('大小写与斜杠不敏感；嵌套目录取最长匹配根', () => {
    const out = lastCleanByPath(
      [{ timestamp: iso(500), source: 'c:/app/logs/deep/x.log' }],
      ['C:\\App', 'C:\\App\\logs'],
    );
    expect(out).toEqual({ 'C:\\App\\logs': 500 });
  });

  it('不落在任何目录下 / 时间戳不可解析 → 不计入', () => {
    expect(
      lastCleanByPath(
        [
          { timestamp: iso(100), source: 'D:\\Other\\x.log' },
          { timestamp: 'nope', source: 'C:\\App\\logs\\y.log' },
        ],
        ['C:\\App\\logs'],
      ),
    ).toEqual({});
  });

  it('相似前缀不误匹配（C:\\App 不吞 C:\\Apple）', () => {
    const out = lastCleanByPath(
      [{ timestamp: iso(700), source: 'C:\\Apple\\x.log' }],
      ['C:\\App'],
    );
    expect(out).toEqual({});
  });
});

// R157：命中目录覆盖统计——多命中目录时「只清过几个」要能看出来。
describe('cleanedHitStats（命中目录覆盖）', () => {
  it('全部目录都清过 → cleaned = total', () => {
    expect(cleanedHitStats({ 'C:\\a': 100, 'C:\\b': 200 }, ['C:\\a', 'C:\\b'])).toEqual({
      cleaned: 2,
      total: 2,
      latestTs: 200,
    });
  });

  it('只清过部分 → 如实报覆盖数（不假装整个 scope 都清过）', () => {
    expect(cleanedHitStats({ 'C:\\a': 100 }, ['C:\\a', 'C:\\b', 'C:\\c'])).toEqual({
      cleaned: 1,
      total: 3,
      latestTs: 100,
    });
  });

  it('台账未读到（null）→ cleaned 0 且 latestTs null（不编造）', () => {
    expect(cleanedHitStats(null, ['C:\\a'])).toEqual({ cleaned: 0, total: 1, latestTs: null });
  });

  it('一个目录都没清过 → cleaned 0', () => {
    expect(cleanedHitStats({}, ['C:\\a', 'C:\\b'])).toEqual({
      cleaned: 0,
      total: 2,
      latestTs: null,
    });
  });
});

// R174：可清理占比——三个数字要读一遍才知道「能清掉几成」，条让比例一眼可见。
describe('cleanRatio（可清理占比）', () => {
  it('正常比例', () => {
    expect(cleanRatio(25, 100)).toBe(0.25);
    expect(cleanRatio(100, 100)).toBe(1);
    expect(cleanRatio(0, 100)).toBe(0);
  });

  it('total ≤ 0 / 非有限 → null（没有分母就不给比例）', () => {
    expect(cleanRatio(10, 0)).toBeNull();
    expect(cleanRatio(10, -5)).toBeNull();
    expect(cleanRatio(10, Number.NaN)).toBeNull();
    expect(cleanRatio(Number.NaN, 100)).toBeNull();
  });

  it('脏数据（可清 > 总）钳到 1，不画出超长条', () => {
    expect(cleanRatio(150, 100)).toBe(1);
    expect(cleanRatio(-20, 100)).toBe(0);
  });
});

describe('sortScopesByBytes（R177 清理分类组排序）', () => {
  const items = [
    { id: 'a', b: 10 },
    { id: 'b', b: 50 },
    { id: 'c', b: 30 },
  ];
  it('按可清大小降序', () => {
    const out = sortScopesByBytes(items, (x) => x.b);
    expect(out.map((x) => x.id)).toEqual(['b', 'c', 'a']);
  });

  it('不改入参（原数组顺序保持）', () => {
    const before = items.map((x) => x.id);
    sortScopesByBytes(items, (x) => x.b);
    expect(items.map((x) => x.id)).toEqual(before);
  });

  it('大小相同维持原顺序（稳定）', () => {
    const tie = [
      { id: 'x', b: 20 },
      { id: 'y', b: 20 },
      { id: 'z', b: 5 },
    ];
    expect(sortScopesByBytes(tie, (x) => x.b).map((x) => x.id)).toEqual(['x', 'y', 'z']);
  });

  it('sizeOf 缺失（取不到）按 0 排到末尾', () => {
    const out = sortScopesByBytes([{ id: 'p', b: 0 }, { id: 'q', b: 7 }], (x) => x.b);
    expect(out.map((x) => x.id)).toEqual(['q', 'p']);
  });
});
