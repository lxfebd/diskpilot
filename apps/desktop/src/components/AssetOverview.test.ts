import { describe, it, expect, beforeAll } from 'vitest';
import type { SpacePoint } from '../api';
import { setLang } from '../i18n';
import { buildSpaceSeries, spaceWeekDelta, spaceSparkPath, driveUsagePct, isDriveCritical } from './AssetOverview';

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
