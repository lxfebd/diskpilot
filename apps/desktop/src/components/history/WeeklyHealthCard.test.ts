import { describe, expect, it } from 'vitest';
import { t } from '../../i18n';
import { nextInspectionDue, reportAgeLabel, weeklyDelta } from './WeeklyHealthCard';
import type { WeeklyReport } from '../../api';

const base = (over: Partial<WeeklyReport>): WeeklyReport => ({
  ts: 1_800_000_000,
  drives: [],
  total_bytes: 100,
  temp_max_c: 60,
  hw_snapshots: 1,
  hw_delta: [],
  ...over,
});

// 周巡检简报卡的纯函数：距今天数文案（「刚刚」/「N 天前」/具体日期）。
describe('reportAgeLabel', () => {
  const now = 1_800_000_000; // 2027-01-16 附近

  it('零时间戳返回空字符串', () => {
    expect(reportAgeLabel(0, now, t)).toBe('');
  });

  it('一天内显示「刚刚」', () => {
    expect(reportAgeLabel(now - 3600, now, t)).toBe(t('history.weekly.justNow'));
  });

  it('30 天内显示「N 天前」', () => {
    expect(reportAgeLabel(now - 3 * 86400, now, t)).toBe(t('history.weekly.daysAgo', { n: 3 }));
  });

  it('超过 30 天显示具体日期', () => {
    // now - 60 天
    expect(reportAgeLabel(now - 60 * 86400, now, t)).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});

// R95 周报环比：两份都有才对比，字段缺失不编造。
describe('weeklyDelta', () => {
  it('缺上一份 → null（无从对比）', () => {
    expect(weeklyDelta(base({}), null)).toBeNull();
    expect(weeklyDelta(null, base({}))).toBeNull();
    expect(weeklyDelta(null, null)).toBeNull();
  });

  it('可清理量环比：本周-上周', () => {
    const cur = base({ total_bytes: 150 });
    const prev = base({ total_bytes: 100 });
    expect(weeklyDelta(cur, prev)?.cleanBytes).toBe(50);
    expect(weeklyDelta(prev, cur)?.cleanBytes).toBe(-50);
    expect(weeklyDelta(cur, base({ total_bytes: 150 }))?.cleanBytes).toBe(0);
  });

  it('上周无可清数据 → cleanBytes null（不编造差值）', () => {
    const cur = base({ total_bytes: 150 });
    const prev = base({ total_bytes: 0 });
    expect(weeklyDelta(cur, prev)?.cleanBytes).toBeNull();
  });

  it('温度环比：任一缺失 → tempDelta null；都有则求差', () => {
    const cur = base({ temp_max_c: 75 });
    const prev = base({ temp_max_c: 60 });
    expect(weeklyDelta(cur, prev)?.tempDelta).toBe(15);
    expect(weeklyDelta(cur, base({ temp_max_c: null }))?.tempDelta).toBeNull();
    expect(weeklyDelta(base({ temp_max_c: null }), prev)?.tempDelta).toBeNull();
    expect(weeklyDelta(cur, base({ temp_max_c: 75 }))?.tempDelta).toBe(0);
  });
});

// R151：巡检到期——周报按周节奏，满 7 天该重新生成。
describe('nextInspectionDue（巡检到期）', () => {
  const now = 1_800_000_000;
  const DAY = 86400;

  it('刚生成 → 未到期，剩余 7 天', () => {
    expect(nextInspectionDue(now, now)).toEqual({ due: false, daysLeft: 7 });
  });

  it('过了 3 天 → 未到期，剩余 4 天', () => {
    expect(nextInspectionDue(now - 3 * DAY, now)).toEqual({ due: false, daysLeft: 4 });
  });

  it('恰好 7 天 → 到期（daysLeft 0）', () => {
    expect(nextInspectionDue(now - 7 * DAY, now)).toEqual({ due: true, daysLeft: 0 });
  });

  it('超过 7 天 → 到期，daysLeft 不为负', () => {
    expect(nextInspectionDue(now - 20 * DAY, now)).toEqual({ due: true, daysLeft: 0 });
  });

  it('时间戳无效（0 / 负数 / NaN）→ null（不编造到期日）', () => {
    expect(nextInspectionDue(0, now)).toBeNull();
    expect(nextInspectionDue(-5, now)).toBeNull();
    expect(nextInspectionDue(Number.NaN, now)).toBeNull();
  });
});
