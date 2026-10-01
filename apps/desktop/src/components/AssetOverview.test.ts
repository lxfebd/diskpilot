import { describe, it, expect, beforeAll } from 'vitest';
import type { SpacePoint } from '../api';
import type { Node } from '../types';
import { setLang } from '../i18n';
import { buildSpaceSeries, spaceWeekDelta, spaceSparkPath, driveUsagePct, isDriveCritical, aggregateByScaffold, reclaimByScaffoldIds, isTempHigh, disabledStartupCount } from './AssetOverview';

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
