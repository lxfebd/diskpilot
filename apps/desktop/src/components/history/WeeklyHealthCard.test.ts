import { describe, expect, it } from 'vitest';
import { t } from '../../i18n';
import { reportAgeLabel } from './WeeklyHealthCard';

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
