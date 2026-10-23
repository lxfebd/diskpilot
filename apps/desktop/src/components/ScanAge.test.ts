import { describe, it, expect, beforeAll } from 'vitest';
import { setLang } from '../i18n';
import { summarizeScanAge, type ScanAgeRow } from './AssetOverview';

// 断言按中文原文写，先钉死语言。
beforeAll(() => setLang('zh'));

function row(root: string, days: number | null, never = false, clock_skew = false): ScanAgeRow {
  return { root, staleness: { never, clock_skew, days } };
}

describe('扫描时效汇总（R293：never / stale / skew / unread 四态分开）', () => {
  it('全未读到 = unread，不把「读不到」说成「刚扫过」', () => {
    const s = summarizeScanAge([
      { root: 'C:\\', staleness: null },
      { root: 'D:\\', staleness: null },
    ], 21);
    expect(s.state).toBe('unread');
    expect(s.unreadCount).toBe(2);
    expect(s.worstRoot).toBeNull();
  });

  it('从未扫过优先于过期：never 单独成态（措辞是「还没扫过」不是「很久没扫」）', () => {
    const s = summarizeScanAge([row('C:\\', 30), row('D:\\', null, true)], 21);
    expect(s.state).toBe('never');
    expect(s.worstRoot).toBe('D:\\');
    expect(s.neverCount).toBe(1);
    expect(s.staleCount).toBe(1); // C 也算过期，但 never 优先展示
  });

  it('stale：最久未扫的盘排最前，天数向下取整不虚报', () => {
    const s = summarizeScanAge([row('C:\\', 30), row('D:\\', 20)], 21);
    expect(s.state).toBe('stale');
    expect(s.worstRoot).toBe('C:\\');
    expect(s.worstDays).toBe(30);
    expect(s.staleCount).toBe(1); // D=20 未达 21，不算过期
  });

  it('skew：时间戳异常时天数不可信，不报 stale 也不编天数', () => {
    const s = summarizeScanAge([row('C:\\', null, false, true)], 21);
    expect(s.state).toBe('skew');
    expect(s.worstRoot).toBe('C:\\');
    expect(s.worstDays).toBeNull();
  });

  it('全在阈值内 = ok（哪怕部分盘 unread，仍按已知盘给结论并计数）', () => {
    const s = summarizeScanAge([row('C:\\', 3), { root: 'D:\\', staleness: null }], 21);
    expect(s.state).toBe('ok');
    expect(s.unreadCount).toBe(1);
  });

  it('空集合 = unread（没数据不等于健康）', () => {
    expect(summarizeScanAge([], 21).state).toBe('unread');
  });

  it('阈值边界：恰好 21 天算过期（与后端 collect_scan_stale d >= days 同口径）', () => {
    expect(summarizeScanAge([row('C:\\', 20)], 21).state).toBe('ok');
    expect(summarizeScanAge([row('C:\\', 21)], 21).state).toBe('stale');
    // 0 = 一天内刚扫过，必须是 ok
    expect(summarizeScanAge([row('C:\\', 0)], 21).state).toBe('ok');
  });
});
