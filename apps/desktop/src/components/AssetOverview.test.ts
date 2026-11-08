import { describe, it, expect, beforeAll } from 'vitest';
import type { SpacePoint } from '../api';
import type { Node } from '../types';
import { setLang } from '../i18n';
import { DRIVE_WARN_PCT } from '../drives';
import { buildSpaceSeries, spaceWeekDelta, spaceSparkPath, driveUsagePct, isDriveCritical, pickWorstRedDrive, aggregateByScaffold, reclaimByScaffoldIds, isTempHigh, disabledStartupCount, hasCheckedWechat, startupCheckState, scaffoldMainPaths, forecastFullDays, forecastFullInfo, linearFit, forecastConfidence, isOfficialPlugin, diagSummaryLine, loadDiagSummary, parseRegHunterJson, buildQueueMd, spaceRisk, spaceTrendSamples, truncatePaths, previewSummary, previewTotalBytes, previewState, sortCleanItemsByBytes, sortPreviewItemsByBytes, pruneStalePreviews, mergeCheckedState, diffTrend, shouldShowDiff, biggestChange, diffGapDate, dupRunningCount } from './AssetOverview';

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

// R520 官方诊断插件判定（体检格只列官方索引来源的插件）
describe('isOfficialPlugin（R520 插件体检）', () => {
  it('官方索引来源 → true，第三方/本地来源 → false', () => {
    expect(isOfficialPlugin('https://github.com/lxfebd/diskpilot-plugin-index/releases/...')).toBe(true);
    expect(isOfficialPlugin('https://evil.example/plugin.zip')).toBe(false);
    expect(isOfficialPlugin('local')).toBe(false);
    expect(isOfficialPlugin('')).toBe(false);
  });
});

// R524 诊断摘要行：取 stdout 首个非空行并截断
describe('diagSummaryLine / loadDiagSummary（R524 上次诊断摘要）', () => {
  it('首个非空行作为摘要，超 60 字截断带省略号', () => {
    expect(diagSummaryLine('\n  计划任务审计（只读）: 共 283 项  \n第二行')).toBe('计划任务审计（只读）: 共 283 项');
    const long = 'x'.repeat(100);
    expect(diagSummaryLine(long)).toHaveLength(60);
    expect(diagSummaryLine(long).endsWith('…')).toBe(true);
    expect(diagSummaryLine('')).toBe('');
  });
  it('无 localStorage 或内容损坏时 loadDiagSummary 返回 null 不抛错', () => {
    expect(loadDiagSummary()).toBeNull();
  });
});

describe('parseRegHunterJson（R530 待人工确认入队解析）', () => {
  it('首行摘要 + 第二行 JSON 数组解析出失效项', () => {
    const out = '注册表审计（只读）: 失效卸载 2 条\\n[{"kind":"uninstall","key":"HKLM\\\\X","name":"某软件","reasons":["路径不存在"]},{"kind":"run","key":"HKCU\\\\Run","name":"foo","cmd_path":"C:\\\\gone\\\\a.exe"}]';
    const items = parseRegHunterJson(out);
    expect(items).toHaveLength(2);
    expect(items[0]).toMatchObject({ kind: 'uninstall', name: '某软件' });
    expect(items[1].cmd_path).toBe('C:\\gone\\a.exe');
  });
  it('没有 JSON 行 / JSON 损坏 / 非数组 → 空数组不抛错', () => {
    expect(parseRegHunterJson('纯文本没有数组')).toEqual([]);
    expect(parseRegHunterJson('摘要\\n[{\\"bad')).toEqual([]);
    expect(parseRegHunterJson('摘要\\n{"not":"array"}')).toEqual([]);
    expect(parseRegHunterJson('')).toEqual([]);
  });
  it('缺 key/kind 的条目被过滤', () => {
    const out = '摘要\\n[{"name":"no key"},{"kind":"uninstall","key":"K","name":"ok"}]';
    expect(parseRegHunterJson(out)).toHaveLength(1);
  });
});

describe('buildQueueMd（R533 清单导出报告）', () => {
  const L = {
    title: 'DiskPilot 待人工确认清单',
    ts: '生成时间：',
    total: '共 {n} 条。',
    note: '**未做任何删除或处置**',
    colSource: '来源',
    colKind: '类型',
    colName: '名称',
    colKey: '定位键',
    detailHead: '详情',
    sourceLine: '来源：{source}（{kind}）',
    keyLine: '键：',
    diagLine: '诊断详情：',
  };
  it('生成表头 + 条目表 + 详情块，管道符转义', () => {
    const md = buildQueueMd(
      [
        { id: 'q1', at: 0, source: 'reg-hunter', kind: 'uninstall', key: 'HKLM\\X|Y', name: '某|软件', detail: '{"reasons":["路径不存在"]}' },
        { id: 'q2', at: 0, source: 'svc-hunter', kind: 'service', key: 'svc1', name: '可疑服务', detail: '{}' },
      ],
      new Date('2026-11-08T12:00:00+08:00').getTime(),
      L,
    );
    expect(md).toContain('# DiskPilot 待人工确认清单');
    expect(md).toContain('共 2 条。');
    expect(md).toContain('| reg-hunter | uninstall | 某\\|软件 | `HKLM\\X\\|Y` |');
    expect(md).toContain('| svc-hunter | service | 可疑服务 | `svc1` |');
    expect(md).toContain('### 某\\|软件');
    expect(md).toContain('路径不存在');
    // 明确声明未处置
    expect(md).toContain('未做任何删除或处置');
  });
  it('空清单也能生成合法报告', () => {
    const md = buildQueueMd([], Date.now(), L);
    expect(md).toContain('共 0 条。');
    expect(md).not.toContain('## 详情');
  });
});

// R519 最小二乘回归 + 预计填满日期 + r2 置信度
describe('linearFit / forecastFullInfo（R519 回归预测）', () => {
  it('完美线性数据 r2=1、斜率正确', () => {
    const pts = [
      pt('C:\\', 100, 1000),
      pt('C:\\', 200, 1000 + 86400),
      pt('C:\\', 300, 1000 + 2 * 86400),
    ];
    const fit = linearFit(pts);
    expect(fit).not.toBeNull();
    expect(fit!.slopePerSec).toBeCloseTo(100 / 86400, 10);
    expect(fit!.r2).toBeCloseTo(1, 10);
  });

  it('噪声数据 r2 < 1，但斜率仍朝正', () => {
    const pts = [
      pt('C:\\', 100, 1000),
      pt('C:\\', 190, 1000 + 86400), // 偏离直线
      pt('C:\\', 300, 1000 + 2 * 86400),
    ];
    const fit = linearFit(pts);
    expect(fit!.r2).toBeLessThan(1);
    expect(fit!.r2).toBeGreaterThan(0.8);
    expect(fit!.slopePerSec).toBeGreaterThan(0);
  });

  it('forecastFullInfo 给出预计填满日期（now + days）', () => {
    // total 1000，每天 +100 字节 → 剩 500 需要 5 天
    const nowMs = 1_700_000_000_000;
    const pts = [
      pt('C:\\', 400, 1_700_000_000, 1000),
      pt('C:\\', 500, 1_700_000_000 + 86400, 1000),
    ];
    const info = forecastFullInfo(pts, nowMs);
    expect(info).not.toBeNull();
    expect(info!.days).toBeCloseTo(5, 6);
    expect(info!.dateMs).toBeCloseTo(nowMs + 5 * 86400 * 1000, -3);
    expect(info!.r2).toBeCloseTo(1, 10);
  });

  it('forecastConfidence 叠加 r2：rich 但 r2<0.7 → low', () => {
    expect(forecastConfidence('rich', 0.9)).toBe('high');
    expect(forecastConfidence('rich', 0.5)).toBe('low');
    expect(forecastConfidence('sparse', 0.99)).toBe('low');
    expect(forecastConfidence('none', 0.99)).toBe('none');
    // 兼容旧调用（不传 r2）
    expect(forecastConfidence('rich')).toBe('high');
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

// R145 采样充分度：趋势/外推可信度取决于采样点数——2 点算出的增速不足为据。
describe('spaceTrendSamples（空间趋势采样充分度）', () => {
  it('≥5 点 → rich（趋势可信）', () => {
    expect(spaceTrendSamples([1, 2, 3, 4, 5].map((at) => ({ at })))).toBe('rich');
    expect(spaceTrendSamples(Array.from({ length: 30 }, (_, i) => ({ at: i })))).toBe('rich');
  });

  it('2–4 点 → sparse（仅供参考）', () => {
    expect(spaceTrendSamples([{ at: 1 }, { at: 2 }])).toBe('sparse');
    expect(spaceTrendSamples([{ at: 1 }, { at: 2 }, { at: 3 }, { at: 4 }])).toBe('sparse');
  });

  it('不足 2 点 → none（不出结论）', () => {
    expect(spaceTrendSamples([])).toBe('none');
    expect(spaceTrendSamples([{ at: 7 }])).toBe('none');
  });

  it('非有限 at 被滤掉后再判档', () => {
    expect(spaceTrendSamples([{ at: Number.NaN }, { at: 1 }])).toBe('none');
    expect(
      spaceTrendSamples([{ at: 1 }, { at: 2 }, { at: 3 }, { at: 4 }, { at: Number.NaN }]),
    ).toBe('sparse');
  });
});

// R148 耗尽预测可信度：外推建立在首末两点，样本少时结论要降级。
describe('forecastConfidence（耗尽预测可信度）', () => {
  it('样本充分 → high（照常显示预测）', () => {
    expect(forecastConfidence('rich')).toBe('high');
  });
  it('样本少 → low（标注「样本少，粗估 N 天满」）', () => {
    expect(forecastConfidence('sparse')).toBe('low');
  });
  it('样本不足 → none（不出预测）', () => {
    expect(forecastConfidence('none')).toBe('none');
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

// R305：体检条红盘格点下去该选中「最满的那块红盘」，不是开始全盘扫描。
describe('红盘格落点（pickWorstRedDrive）', () => {
  const d = (path: string, used: number, total: number) => ({
    path,
    used_bytes: used,
    total_bytes: total,
    free_bytes: total - used,
  });

  it('多块红盘取使用率最高的那块（不是第一块）', () => {
    const worst = pickWorstRedDrive([d('C:\\', 80, 100), d('D:\\', 95, 100), d('E:\\', 88, 100)]);
    expect(worst?.path).toBe('D:\\');
  });

  it('没有红盘返回 null；84.9% 不算红盘', () => {
    expect(pickWorstRedDrive([d('C:\\', 60, 100), d('D:\\', 849, 1000)])).toBeNull();
    expect(pickWorstRedDrive([])).toBeNull();
  });

  it('容量没读到的盘不参与排序（不能拿 0 当最满，也不能当红盘）', () => {
    expect(pickWorstRedDrive([d('C:\\', 0, 0), d('D:\\', 90, 100)])?.path).toBe('D:\\');
    expect(pickWorstRedDrive([d('C:\\', 999, 0)])).toBeNull();
  });

  it('映射盘按物理卷去重：D: 是 J:\\D 的 subst 时只算 J:（与红盘计数同口径）', () => {
    const drives = [
      { ...d('D:\\', 96, 100), mapped_to: 'J:\\D' },
      d('J:\\', 90, 100),
    ];
    // 映射盘挂在目标卷上，真正那块盘是 J:
    expect(pickWorstRedDrive(drives)?.path).toBe('J:\\');
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
  const wechatItem = { key: 'w1', scaffoldId: 'wechat-pc', scopeId: 'chat-media', label: '', desc: '', bytes: 100, files: 1, path: 'C:\\WeChat Files' };
  const otherItem = { key: 'n1', scaffoldId: 'browser', scopeId: 'cache', label: '', desc: '', bytes: 200, files: 2, path: 'C:\\Chrome' };

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

describe('R126 建议清理排序 sortCleanItemsByBytes', () => {
  const item = (key: string, bytes: number) => ({ key, scaffoldId: 's', scopeId: key, label: key, desc: '', bytes, files: 1, path: 'C:\\Dir' });
  it('按可回收字节降序——最大收益排最前', () => {
    const sorted = sortCleanItemsByBytes([item('small', 100), item('big', 10_000), item('mid', 500)]);
    expect(sorted.map((s) => s.key)).toEqual(['big', 'mid', 'small']);
  });
  it('不修改入参数组（纯函数）', () => {
    const input = [item('a', 2), item('b', 1)];
    sortCleanItemsByBytes(input);
    expect(input.map((i) => i.key)).toEqual(['a', 'b']);
  });
  it('空数组幂等', () => {
    expect(sortCleanItemsByBytes([])).toEqual([]);
  });
});

describe('R128 dry-run 预览排序 sortPreviewItemsByBytes', () => {
  const mk = (path: string, bytes: number | null) => ({ path, bytes });
  it('按字节降序——大文件排最前', () => {
    const sorted = sortPreviewItemsByBytes([mk('a', 10), mk('b', 5000), mk('c', 300)]);
    expect(sorted.map((s) => s.path)).toEqual(['b', 'c', 'a']);
  });
  it('bytes 为 null 的项沉底且保持相对顺序', () => {
    const sorted = sortPreviewItemsByBytes([
      mk('unknown1', null),
      mk('big', 9000),
      mk('unknown2', null),
      mk('small', 5),
    ]);
    expect(sorted.map((s) => s.path)).toEqual(['big', 'small', 'unknown1', 'unknown2']);
  });
  it('不修改入参数组（纯函数）', () => {
    const input = [mk('a', 2), mk('b', 1)];
    sortPreviewItemsByBytes(input);
    expect(input.map((i) => i.path)).toEqual(['a', 'b']);
  });
});

// R249：预览缓存按存活 key 剪枝——清过的项消失后其 dry-run 明细/徽标作废。
describe('pruneStalePreviews（陈旧预览剪枝）', () => {
  const previews = { 'a:s1': { lastClean: 100 }, 'b:s2': { lastClean: 200 }, 'c:s3': { lastClean: null } };

  it('剪掉不在存活集里的 key，保留仍在列表的', () => {
    const out = pruneStalePreviews(previews, new Set(['a:s1', 'c:s3']));
    expect(Object.keys(out).sort()).toEqual(['a:s1', 'c:s3']);
    expect(out['a:s1']).toEqual({ lastClean: 100 });
  });

  it('全部存活 → 返回原对象（引用不变，免重渲染）', () => {
    expect(pruneStalePreviews(previews, new Set(['a:s1', 'b:s2', 'c:s3']))).toBe(previews);
  });

  it('空缓存 → 原对象', () => {
    const empty: Record<string, { lastClean: number | null }> = {};
    expect(pruneStalePreviews(empty, new Set(['x']))).toBe(empty);
    expect(Object.keys(pruneStalePreviews(empty, new Set()))).toHaveLength(0);
  });

  it('清过的项消失 → 其预览被剪；还原重现时重新拉取（不展示陈旧 dry-run）', () => {
    const out = pruneStalePreviews(previews, new Set(['b:s2']));
    expect(Object.keys(out)).toEqual(['b:s2']);
  });
});

// R254：建议项重拉后合并勾选——新 key 默认勾选，已有 key 保留用户选择，
// 不再每次强制全选（避免用户取消微信项后一次清理/重扫被全勾回来误清）。
describe('mergeCheckedState（勾选状态跨重拉保留）', () => {
  it('空勾选 + 新 items → 全部默认勾选', () => {
    expect(mergeCheckedState({}, ['a:s1', 'b:s2'])).toEqual({ 'a:s1': true, 'b:s2': true });
  });

  it('已有 key 保留原选择（含手动取消的 false）', () => {
    const out = mergeCheckedState({ 'a:s1': false, 'b:s2': true }, ['a:s1', 'b:s2', 'c:s3']);
    expect(out).toEqual({ 'a:s1': false, 'b:s2': true, 'c:s3': true });
  });

  it('消失的 key 不再出现在新状态', () => {
    const out = mergeCheckedState({ 'a:s1': false, 'z:gone': true }, ['a:s1']);
    expect(out).toEqual({ 'a:s1': false });
  });

  it('不修改原对象（纯函数）', () => {
    const input = { 'a:s1': false };
    mergeCheckedState(input, ['a:s1', 'b:s2']);
    expect(input).toEqual({ 'a:s1': false });
  });
});
// health-butler P0：建议清理跨次对比。核心纪律——没有上次快照（total_delta
// = null）时不得显示任何"涨跌"措辞，更不能把 null 当 0 说成"持平"。
describe('建议清理跨次对比（health-butler P0）', () => {
  it('diffTrend：正差=多出，取绝对值', () => {
    expect(diffTrend(3_000_000_000)).toEqual({ trend: 'up', abs: 3_000_000_000 });
  });

  it('diffTrend：负差=更少，绝对值取正（显示不带负号）', () => {
    expect(diffTrend(-1_500)).toEqual({ trend: 'down', abs: 1_500 });
  });

  it('diffTrend：0 = 持平', () => {
    expect(diffTrend(0)).toEqual({ trend: 'flat', abs: 0 });
  });

  it('diffTrend：null = 无从比较（没有上次，绝不当成 0）', () => {
    expect(diffTrend(null)).toEqual({ trend: 'none', abs: 0 });
  });

  it('shouldShowDiff：有方向（含持平）才显示环比区', () => {
    const base = { at: 10, prev_at: 5, gap_days: 1, scaffolds: [], new_scaffolds: [], gone_scaffolds: [] };
    expect(shouldShowDiff({ ...base, total_delta: 1024 })).toBe(true);
    expect(shouldShowDiff({ ...base, total_delta: 0 })).toBe(true);
    // 首次快照：diff 对象存在但没有差值 → 不显示徽标（另走"首次"文案）。
    expect(shouldShowDiff({ ...base, total_delta: null })).toBe(false);
    expect(shouldShowDiff(null)).toBe(false);
  });
});

// R337：跨次对比里的「主要变化」必须真是最大变化——后端按 id 字典序给过一份，
// 前端不能再直接取 [0]。
describe('主要变化（R337 biggestChange）', () => {
  const sc = (id: string, prev: number, now: number) => ({
    scaffold_id: id,
    prev_bytes: prev,
    now_bytes: now,
    delta: now - prev,
  });

  it('取 |delta| 最大的一项，而不是数组第一项', () => {
    const top = biggestChange([sc('browser', 500, 501), sc('wechat', 100, 500), sc('conda', 0, 99)]);
    expect(top?.scaffold_id).toBe('wechat');
    expect(top?.delta).toBe(400);
  });

  it('减少得最多的也算最大变化（按绝对值比）', () => {
    const top = biggestChange([sc('browser', 10, 20), sc('wechat', 900, 100)]);
    expect(top?.scaffold_id).toBe('wechat');
    expect(top?.delta).toBe(-800);
  });

  it('同 |delta| 按 id 升序，结果稳定', () => {
    expect(biggestChange([sc('zeta', 200, 100), sc('alpha', 200, 100)])?.scaffold_id).toBe('alpha');
  });

  it('全为 0 / 空 / null → null（不报假的主要变化）', () => {
    expect(biggestChange([sc('wechat', 100, 100)])).toBeNull();
    expect(biggestChange([])).toBeNull();
    expect(biggestChange(null)).toBeNull();
    expect(biggestChange(undefined)).toBeNull();
  });

  it('前后值一起带出，看得出从多少变到多少', () => {
    expect(biggestChange([sc('wechat', 100, 500)])).toEqual({
      scaffold_id: 'wechat',
      prev_bytes: 100,
      now_bytes: 500,
      delta: 400,
    });
  });
});

// R338：环比「N 天前」要能对上日历——后端 prev_at 前端此前没读。
describe('环比日期（R338 diffGapDate）', () => {
  it('有上次快照 → 给 YYYY-MM-DD', () => {
    expect(diffGapDate(1_700_000_000, 3)).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });

  it('没有上次（prev_at 0 / null）→ null，只显示天数', () => {
    expect(diffGapDate(0, 3)).toBeNull();
    expect(diffGapDate(null, 3)).toBeNull();
    expect(diffGapDate(undefined, 3)).toBeNull();
  });

  it('gapDays 为 null（没有上次）→ null', () => {
    expect(diffGapDate(1_700_000_000, null)).toBeNull();
  });
});

// R326：红盘判定用未取整的百分比，并且与盘卡片/环形图共用 drives.ts 的同一个常量。
describe('红盘判定口径（R326）', () => {
  it('84.6% 不算红盘（取整成 85 也不算）', () => {
    expect(isDriveCritical(846, 1000)).toBe(false);
    expect(isDriveCritical(850, 1000)).toBe(true);
    expect(isDriveCritical(849, 1000)).toBe(false);
  });

  it('阈值取自共享常量，组件里不再另写字面量', () => {
    expect(isDriveCritical(DRIVE_WARN_PCT, 100)).toBe(true);
    expect(isDriveCritical(DRIVE_WARN_PCT - 1, 100)).toBe(false);
  });

  it('容量没读到：不判红盘（0 不是健康，只是没读数）', () => {
    expect(isDriveCritical(5, 0)).toBe(false);
  });
describe('重复文件跳过数（R340 dupRunningCount）', () => {
  it('多组累加：running 里每个路径都算一个被跳过的重复文件', () => {
    expect(dupRunningCount([{ running: ['a.exe', 'b.exe'] }, { running: ['c.exe'] }])).toBe(3);
  });
  it('null / 空数组 → 0', () => {
    expect(dupRunningCount(null)).toBe(0);
    expect(dupRunningCount(undefined)).toBe(0);
    expect(dupRunningCount([])).toBe(0);
  });
  it('running 缺失或为空数组 → 0（没有跳过项就不加）', () => {
    expect(dupRunningCount([{ running: undefined }, { running: [] }])).toBe(0);
  });
});

});
