import { describe, it, expect, beforeAll } from 'vitest';
import type { SpacePoint } from '../api';
import type { Node } from '../types';
import { setLang } from '../i18n';
import { buildSpaceSeries, spaceWeekDelta, spaceSparkPath, driveUsagePct, isDriveCritical, aggregateByScaffold, reclaimByScaffoldIds, isTempHigh, disabledStartupCount, hasCheckedWechat, startupCheckState, scaffoldMainPaths, forecastFullDays, spaceRisk, truncatePaths, previewSummary, previewTotalBytes, previewState } from './AssetOverview';

// spaceWeekDelta 的返回值是 t() 产物，断言按中文原文写，必须先钉死语言。
beforeAll(() => setLang('zh'));

function pt(root: string, used_bytes: number, at: number, total_bytes = 1000): SpacePoint {
  return { root, total_bytes, used_bytes, at };
}

// 三次 mock 扫描（R6 验收）：C 盘 3 天前 400GB → 2 天前 420GB → 今天 470GB，
// D 盘只扫过一次 200GB。验证聚合按盘、曲线点序、增量计算全部正确。
const MOCK_HIST: Record<string, SpacePoint[]> = {
  'C:\\': [
    pt('C:\\', 400, 1_700_000_000),
    pt('C:\\', 420, 1_700_086_400),
    pt('C:\\', 470, 1_700_259_200),
  ],
  'C:\\Users': [pt('C:\\Users', 460, 1_700_172_800)], // 与 C:\ 同盘，应并入序列
  'D:\\': [pt('D:\\', 200, 1_700_000_000)],
};

describe('磁盘空间趋势（R6）', () => {
  it('buildSpaceSeries 按盘聚合 + 时间排序（C:\ 与 C:\Users 同盘）', () => {
    const series = buildSpaceSeries(MOCK_HIST);
    const c = series.find((s) => s.drive === 'C:\\');
    expect(c).toBeTruthy();
    expect(c!.points.length).toBe(4);
    expect(c!.points.map((p) => p.used_bytes)).toEqual([400, 420, 460, 470]);
    // 序列按最新已用降序：C 盘 470 在 D 盘 200 前
    expect(series[0].drive).toBe('C:\\');
    expect(series[1].drive).toBe('D:\\');
  });

  it('映射盘的快照并到它真正所在的卷：同一块盘不画两条曲线', () => {
    // 本机口径：subst D: J:\D。历史里既有 D:\ 也有 J:\ 的快照（同一块 KIOXIA）。
    const hist = { ...MOCK_HIST, 'J:\\': [pt('J:\\', 900, 1_700_000_000)] };
    const drives = [
      { path: 'C:\\', used_bytes: 1, mapped_to: '' },
      { path: 'D:\\', used_bytes: 1, mapped_to: 'J:\\D' },
      { path: 'J:\\', used_bytes: 1, mapped_to: '' },
    ];
    const series = buildSpaceSeries(hist, drives);
    expect(series.map((s) => s.drive)).not.toContain('D:\\');
    const j = series.find((s) => s.drive === 'J:\\')!;
    expect(j.points.map((p) => p.used_bytes)).toEqual([200, 900]);
  });

  it('目标卷不在盘列表里时，映射盘是碰到那块空间的唯一入口，保留自己的序列', () => {
    const drives = [{ path: 'D:\\', used_bytes: 1, mapped_to: 'J:\\D' }];
    const series = buildSpaceSeries({ 'D:\\': [pt('D:\\', 200, 1_700_000_000)] }, drives);
    expect(series.map((s) => s.drive)).toEqual(['D:\\']);
  });

  it('spaceWeekDelta 首尾差 = 本周新增（增长为正）', () => {
    const c = buildSpaceSeries(MOCK_HIST).find((s) => s.drive === 'C:\\')!;
    const { delta, days } = spaceWeekDelta(c.points);
    expect(delta).toBe(70); // 470 - 400
    expect(days).toMatch(/\d+ 天/);
  });

  it('spaceWeekDelta 释放为正增长显示 +；记录不足返回 0', () => {
    const c = buildSpaceSeries(MOCK_HIST).find((s) => s.drive === 'C:\\')!;
    const d = spaceWeekDelta([c.points[0]]);
    expect(d).toEqual({ delta: 0, days: '记录不足' });
  });

  it('spaceSparkPath 生成 M/L 折线（≥2 点），单点返回空串', () => {
    const c = buildSpaceSeries(MOCK_HIST).find((s) => s.drive === 'C:\\')!;
    const path = spaceSparkPath(c.points);
    expect(path).toMatch(/^M/);
    expect(path).toContain('L');
    expect(spaceSparkPath([c.points[0]])).toBe('');
  });

  it('空历史 → 空序列（前端显示引导文案）', () => {
    expect(buildSpaceSeries({})).toEqual([]);
  });

  it('R90 时间窗口过滤：只留窗口内点；全部=不截断', () => {
    const now = Date.now() / 1000;
    const hist = {
      'C:\\': [
        pt('C:\\', 400, now - 40 * 86400), // 40 天前：30 天窗口外
        pt('C:\\', 420, now - 20 * 86400), // 20 天前：窗口内
        pt('C:\\', 470, now - 2 * 86400),  // 2 天前：窗口内
      ],
    };
    const all = buildSpaceSeries(hist, [], 0);
    expect(all[0].points.length).toBe(3); // 全部 = 不截断（R89 后曲线持续积累）
    const r30 = buildSpaceSeries(hist, [], 30);
    expect(r30[0].points.map((p) => p.used_bytes)).toEqual([420, 470]);
    const r7 = buildSpaceSeries(hist, [], 7);
    expect(r7[0].points.map((p) => p.used_bytes)).toEqual([470]);
  });
});

// R98 空间耗尽预测：首尾增速线性外推「多少天后用满」。释放/持平/不足
// 2 点/零跨度 → null（不预测），已满 → 0。
describe('forecastFullDays（空间耗尽预测）', () => {
  it('按首尾增速外推剩余空间天数', () => {
    // total 1000，10 天内已用 400 → 500（+100 = +10/天），剩 500；
    // 500/10 = 50 天。
    const pts = [
      pt('C:\\', 400, 1_700_000_000, 1000),
      pt('C:\\', 500, 1_700_864_000, 1000),
    ];
    expect(forecastFullDays(pts)).toBeCloseTo(50, 6);
  });

  it('空间在释放（增速≤0）→ 不预测（null）', () => {
    const pts = [
      pt('C:\\', 500, 1_700_000_000, 1000),
      pt('C:\\', 400, 1_700_864_000, 1000),
    ];
    expect(forecastFullDays(pts)).toBeNull();
    // 持平同样不预测：不能把健康盘唱成满盘
    expect(forecastFullDays([pt('C:\\', 400, 1_700_000_000, 1000), pt('C:\\', 400, 1_700_864_000, 1000)])).toBeNull();
  });

  it('已满（剩余 ≤0）→ 0；不足 2 点 / 零跨度 → null', () => {
    expect(forecastFullDays([
      pt('C:\\', 1000, 1_700_000_000, 1000),
      pt('C:\\', 1100, 1_700_864_000, 1000),
    ])).toBe(0);
    expect(forecastFullDays([pt('C:\\', 400, 1_700_000_000, 1000)])).toBeNull();
    expect(forecastFullDays([
      pt('C:\\', 400, 1_700_000_000, 1000),
      pt('C:\\', 500, 1_700_000_000, 1000), // 同一时刻
    ])).toBeNull();
  });
});

// R119 耗尽风险分级：把 forecastFullDays 天数映射成视觉档位。
// null（不预测：释放中/不足）→ ok 不唱衰；0 或 ≤30 天 → critical；
// ≤90 天 → warn；其余 ok。
describe('spaceRisk（耗尽风险分级）', () => {
  it('null（释放中/不足）→ ok，不唱衰', () => {
    expect(spaceRisk(null)).toBe('ok');
  });

  it('已满 0 / 30 天内 → critical', () => {
    expect(spaceRisk(0)).toBe('critical');
    expect(spaceRisk(1)).toBe('critical');
    expect(spaceRisk(30)).toBe('critical');
  });

  it('31–90 天 → warn', () => {
    expect(spaceRisk(31)).toBe('warn');
    expect(spaceRisk(90)).toBe('warn');
  });

  it('>90 天 → ok（余量充足）', () => {
    expect(spaceRisk(91)).toBe('ok');
    expect(spaceRisk(500)).toBe('ok');
  });
});

describe('R92 分类主目录定位', () => {
  it('scaffoldMainPaths 取最外层命中里体量最大的目录', () => {
    // 树：C 盘根 → 微信(外层 500) → 子(微信缓存 200，嵌套不打标/不重复)，
    //     另一处 微信(400) → 应取 500 那条。
    const root: Node = {
      name: 'C:', path: 'C:\\', is_dir: true, size: 0, file_count: 0, top_extensions: [],
      children: [
        {
          name: 'WeChat Files', path: 'C:\\WeChat Files', is_dir: true, size: 500, file_count: 10,
          scaffold_id: 'wechat-pc', children: [], top_extensions: [],
        },
        {
          name: 'wechat', path: 'C:\\Users\\me\\wechat', is_dir: true, size: 400, file_count: 8,
          scaffold_id: 'wechat-pc', children: [], top_extensions: [],
        },
      ],
    };
    const m = scaffoldMainPaths(root);
    expect(m.get('wechat-pc')).toBe('C:\\WeChat Files');
  });

  it('scaffoldMainPaths 空树 → 空 map；未打标节点不产生键', () => {
    expect(scaffoldMainPaths(null)).toEqual(new Map());
    const plain: Node = { name: 'x', path: 'C:\\x', is_dir: true, size: 1, file_count: 0, children: [], top_extensions: [] };
    expect(scaffoldMainPaths(plain).size).toBe(0);
  });
});

describe('红盘救援阈值（低配救援横幅）', () => {
  it('driveUsagePct 计算百分比；total 无效返回 0', () => {
    expect(driveUsagePct(85, 100)).toBe(85);
    expect(driveUsagePct(500, 1000)).toBe(50);
    expect(driveUsagePct(100, 0)).toBe(0);
    expect(driveUsagePct(0, 0)).toBe(0);
  });

  it('isDriveCritical：>=85% 才算红盘，边界 85 恰为 true', () => {
    expect(isDriveCritical(85, 100)).toBe(true); // 恰好 85%
    expect(isDriveCritical(90, 100)).toBe(true);
    expect(isDriveCritical(849, 1000)).toBe(false); // 84.9%
    expect(isDriveCritical(60, 100)).toBe(false);
    expect(isDriveCritical(100, 0)).toBe(false); // 无容量数据不误报
  });
});

// ── 分类占用聚合 + 可回收潜力（R11：按选中盘同口径，与建议清理对齐） ──
function nd(size: number, files: number, scaffoldId: string | null, children: Node[] = []): Node {
  return {
    name: scaffoldId ?? 'plain',
    path: 'C:\\x',
    is_dir: true,
    size,
    file_count: files,
    children,
    scaffold_id: scaffoldId,
    top_extensions: [],
  };
}

describe('分类占用聚合 aggregateByScaffold（R11 按选中盘）', () => {
  it('父节点已打标则子节点不重复累计（同 scaffold 深嵌套只算最外层）', () => {
    const root = nd(100, 10, 'wechat-pc', [nd(60, 6, 'wechat-pc', [nd(40, 4, 'wechat-pc')])]);
    const m = aggregateByScaffold(root);
    expect(m.get('wechat-pc')).toEqual({ bytes: 100, files: 10 });
  });

  it('不同 scaffold 嵌套各自计入（子 scaffold 不吞掉父的字节）', () => {
    const root = nd(100, 10, 'game-cache', [nd(60, 6, 'wechat-pc')]);
    const m = aggregateByScaffold(root);
    expect(m.get('game-cache')).toEqual({ bytes: 100, files: 10 });
    expect(m.get('wechat-pc')).toEqual({ bytes: 60, files: 6 });
  });

  it('未打标路径下的打标子树正常计入；root 为 null 返回空表', () => {
    const root = nd(200, 20, null, [nd(30, 3, 'qq-pc')]);
    const m = aggregateByScaffold(root);
    expect(m.size).toBe(1);
    expect(m.get('qq-pc')).toEqual({ bytes: 30, files: 3 });
    expect(aggregateByScaffold(null).size).toBe(0);
  });
});

describe('可回收潜力按 scaffold 聚合 reclaimByScaffoldIds（R11）', () => {
  it('同 scaffold 多 scope 字节加总；不同 scaffold 分开', () => {
    const m = reclaimByScaffoldIds([
      { scaffoldId: 'wechat-pc', bytes: 10 },
      { scaffoldId: 'wechat-pc', bytes: 5 },
      { scaffoldId: 'qq-pc', bytes: 7 },
    ]);
    expect(m.get('wechat-pc')).toBe(15);
    expect(m.get('qq-pc')).toBe(7);
  });

  it('空列表 → 空表（分类卡不显示可回收标注）', () => {
    expect(reclaimByScaffoldIds([]).size).toBe(0);
  });
});

// 今日体检条纯函数（health-butler P0）
describe('温度档位判定 isTempHigh', () => {
  it('null/undefined → 不偏高', () => {
    expect(isTempHigh(null)).toBe(false);
    expect(isTempHigh(undefined)).toBe(false);
  });
  it('低于阈值 → 不偏高', () => {
    expect(isTempHigh(60)).toBe(false);
    expect(isTempHigh(79)).toBe(false);
  });
  it('等于/高于阈值 → 偏高', () => {
    expect(isTempHigh(80)).toBe(true);
    expect(isTempHigh(95)).toBe(true);
  });
});

describe('启动项禁用量 disabledStartupCount', () => {
  it('null/undefined/空 → 0', () => {
    expect(disabledStartupCount(null)).toBe(0);
    expect(disabledStartupCount(undefined)).toBe(0);
    expect(disabledStartupCount([])).toBe(0);
  });
  it('禁用项计数（只认 enabled=false）', () => {
    expect(disabledStartupCount([{ enabled: true }, { enabled: false }, { enabled: true }, { enabled: false }])).toBe(2);
  });
});

// R74 诚实性回归：启动项检查三态——「读失败」绝不能被折叠成「未检测」。
// 未读到 ≠ 不存在（延续 R55 起的同一族规）。
describe('startupCheckState（启动项检查三态）', () => {
  it('读到数据 → ok（刷新成功后旧错误翻回 ok）', () => {
    expect(startupCheckState([{ enabled: true }], null)).toBe('ok');
    expect(startupCheckState([{ enabled: false }], '旧错误')).toBe('ok');
  });

  it('读失败（startup=null + err 在）→ unread，绝不落到「未检测」', () => {
    expect(startupCheckState(null, 'backend down')).toBe('unread');
  });

  it('成功读到 0 项（[] 是真空结果，不是失败）→ ok', () => {
    expect(startupCheckState([], null)).toBe('ok');
  });

  it('真空未检测（无 err、无数据）→ none', () => {
    expect(startupCheckState(null, null)).toBe('none');
  });
});

// G-高3：微信缓存勾选判定（总览没有 wxid 选择器，勾到就引导去清理页）
describe('微信项勾选 hasCheckedWechat', () => {
  const wechatItem = { key: 'w1', scaffoldId: 'wechat-pc', scopeId: 'chat-media', label: '', desc: '', bytes: 100, files: 1 };
  const otherItem = { key: 'n1', scaffoldId: 'browser', scopeId: 'cache', label: '', desc: '', bytes: 200, files: 2 };

  it('未勾选微信项 → false', () => {
    expect(hasCheckedWechat([wechatItem, otherItem], { w1: false, n1: true })).toBe(false);
  });
  it('勾选了微信项 → true', () => {
    expect(hasCheckedWechat([wechatItem, otherItem], { w1: true, n1: false })).toBe(true);
  });
  it('空列表 / 空勾选 → false', () => {
    expect(hasCheckedWechat([], {})).toBe(false);
    expect(hasCheckedWechat([wechatItem], {})).toBe(false);
  });
});

describe('R102 预览截断 truncatePaths', () => {
  const paths = ['C:\\a\\1', 'C:\\a\\2', 'C:\\a\\3', 'C:\\a\\4'];

  it('未超限 → 全显、more=0', () => {
    expect(truncatePaths(paths, 6)).toEqual({ shown: paths, more: 0 });
  });
  it('超限 → 前 limit 条 + 余数', () => {
    const r = truncatePaths(paths, 2);
    expect(r.shown).toEqual(['C:\\a\\1', 'C:\\a\\2']);
    expect(r.more).toBe(2);
  });
  it('limit ≤ 0 = 不截断', () => {
    expect(truncatePaths(paths, 0).more).toBe(0);
    expect(truncatePaths(paths, -1).shown).toHaveLength(4);
  });
  it('空数组 → 空', () => {
    expect(truncatePaths([], 5)).toEqual({ shown: [], more: 0 });
  });
});

describe('R102 预览标题 previewSummary', () => {
  const zh = (o: { n: number }) => `${o.n} 个文件`;

  it('0 文件 → null（不渲染标题行）', () => {
    expect(previewSummary(0, 0, zh)).toBeNull();
  });
  it('有文件无字节 → 只有份数', () => {
    expect(previewSummary(3, null, zh)).toBe('3 个文件');
  });
  it('有文件有字节 → 份数 + 实测大小', () => {
    expect(previewSummary(3, 1024, zh)).toBe('3 个文件 · 1.00 KB');
  });
  it('文案函数透传（i18n 键由调用方注入）', () => {
    const en = (o: { n: number }) => `${o.n} file(s)`;
    expect(previewSummary(2, null, en)).toBe('2 file(s)');
  });
});

describe('R105 预览合计 previewTotalBytes', () => {
  it('空数组 → null', () => {
    expect(previewTotalBytes([])).toBeNull();
  });
  it('全部实测 → 求和', () => {
    expect(previewTotalBytes([{ bytes: 1024 }, { bytes: 2048 }, { bytes: 10 }])).toBe(3082);
  });
  it('部分缺失 → 按已知求和（缺失的不计入）', () => {
    expect(previewTotalBytes([{ bytes: 1024 }, { bytes: null }, { bytes: 512 }])).toBe(1536);
  });
  it('全部缺失 → null（后端未统计时不显示假合计）', () => {
    expect(previewTotalBytes([{ bytes: null }, { bytes: null }])).toBeNull();
  });
});

describe('R106 预览状态判定 previewState', () => {
  it('in-flight 优先 loading（即便留有旧 err 也不显示旧错误）', () => {
    expect(previewState(true, '上次失败', [])).toBe('loading');
    expect(previewState(true, null, undefined)).toBe('loading');
  });
  it('读失败 → error（失败 ≠ 空清单，诚实性铁律）', () => {
    expect(previewState(false, 'boom', [])).toBe('error');
  });
  it('无 err 且空清单 → empty（读到了但没匹配，可能刚清理过）', () => {
    expect(previewState(false, null, [])).toBe('empty');
    expect(previewState(false, null, undefined)).toBe('empty');
  });
  it('有清单 → has', () => {
    expect(previewState(false, null, [{ path: 'C:\\a\\1' }])).toBe('has');
  });
});
