// 操作历史纯逻辑守卫：过滤 / 汇总 / 按天分组。
// 时间用例统一「本地构造 Date → toISOString 再喂回 dayKeyOf 同一代码路径」，
// 避免时区差导致测试只在某些机器上红。
import { describe, it, expect } from 'vitest';
import {
  filterUndoEntries,
  summarizeUndo,
  groupUndoByDay,
  dayGroupLabel,
} from './model';
import type { UndoEntry } from '../types';

function entry(partial: Partial<UndoEntry> & { timestamp: string }): UndoEntry {
  return {
    action: 'recycle',
    source: 'C:\\tmp\\a.log',
    reason: '测试',
    ...partial,
  };
}

/** 本地 Date → RFC3339，喂给 dayKeyOf 后落回同一本地日期（时区安全）。 */
const iso = (d: Date) => d.toISOString();

describe('filterUndoEntries', () => {
  const entries: UndoEntry[] = [
    entry({ timestamp: iso(new Date(2026, 8, 29, 10, 0)), action: 'recycle', source: 'C:\\Cache\\A\\x.tmp', reason: '缓存' }),
    entry({ timestamp: iso(new Date(2026, 8, 28, 9, 0)), action: 'quarantine', source: 'C:\\Games\\B\\save.dat', destination: 'D:\\Quarantine\\save.dat', reason: '误删防护' }),
    entry({ timestamp: iso(new Date(2026, 8, 27, 8, 0)), action: 'delete', source: 'C:\\Logs\\c.log', reason: '老日志' }),
  ];

  it('按动作过滤', () => {
    const out = filterUndoEntries(entries, { action: 'quarantine', query: '' });
    expect(out).toHaveLength(1);
    expect(out[0].action).toBe('quarantine');
  });

  it('关键词匹配 source / destination / reason，大小写不敏感', () => {
    expect(filterUndoEntries(entries, { action: 'all', query: 'save.dat' })).toHaveLength(1);
    expect(filterUndoEntries(entries, { action: 'all', query: 'QUARANTINE' })).toHaveLength(1);
    expect(filterUndoEntries(entries, { action: 'all', query: '日志' })).toHaveLength(1);
  });

  it('query 全空白 = 不过滤；动作 + 关键词可叠加', () => {
    expect(filterUndoEntries(entries, { action: 'all', query: '   ' })).toHaveLength(3);
    expect(filterUndoEntries(entries, { action: 'recycle', query: 'save' })).toHaveLength(0);
    expect(filterUndoEntries(entries, { action: 'recycle', query: 'x.tmp' })).toHaveLength(1);
  });
});

describe('summarizeUndo', () => {
  it('计数与实测字节求和', () => {
    const s = summarizeUndo([
      entry({ timestamp: '', action: 'recycle', bytes_freed: 100 }),
      entry({ timestamp: '', action: 'recycle', bytes_freed: 23 }),
      entry({ timestamp: '', action: 'quarantine', bytes_freed: null }),
      entry({ timestamp: '', action: 'delete' }),
    ]);
    expect(s.total).toBe(4);
    expect(s.byAction).toEqual({ recycle: 2, quarantine: 1, delete: 1 });
    expect(s.bytesFreed).toBe(123);
  });

  it('所有条目都缺实测值时 bytesFreed 为 null', () => {
    const s = summarizeUndo([entry({ timestamp: '' }), entry({ timestamp: '', bytes_freed: null })]);
    expect(s.bytesFreed).toBeNull();
    expect(s.total).toBe(2);
  });

  it('空列表', () => {
    expect(summarizeUndo([])).toEqual({
      total: 0,
      bytesFreed: null,
      byAction: { recycle: 0, quarantine: 0, delete: 0 },
    });
  });
});

describe('groupUndoByDay', () => {
  it('按本地日期分组，组保持输入顺序（最新在前）', () => {
    const groups = groupUndoByDay([
      entry({ timestamp: iso(new Date(2026, 8, 29, 23, 0)), source: 'a' }),
      entry({ timestamp: iso(new Date(2026, 8, 29, 1, 0)), source: 'b' }),
      entry({ timestamp: iso(new Date(2026, 8, 28, 12, 0)), source: 'c' }),
    ]);
    expect(groups.map((g) => g.key)).toEqual(['2026-09-29', '2026-09-28']);
    expect(groups[0].items.map((e) => e.source)).toEqual(['a', 'b']);
    expect(groups[1].items.map((e) => e.source)).toEqual(['c']);
  });

  it('时间戳解析失败的条目单独成空键组，不抛错', () => {
    const groups = groupUndoByDay([entry({ timestamp: 'not-a-date' })]);
    expect(groups).toHaveLength(1);
    expect(groups[0].key).toBe('');
    expect(groups[0].items).toHaveLength(1);
  });
});

describe('dayGroupLabel', () => {
  const t = (k: string) => k;

  it('今天 / 昨天返回 i18n 键求值结果', () => {
    const now = new Date(2026, 8, 29, 18, 0);
    expect(dayGroupLabel('2026-09-29', now, t)).toBe('history.today');
    expect(dayGroupLabel('2026-09-28', now, t)).toBe('history.yesterday');
  });

  it('其余日期直接显示 YYYY-MM-DD；空键走未知日期文案', () => {
    const now = new Date(2026, 8, 29, 18, 0);
    expect(dayGroupLabel('2026-09-01', now, t)).toBe('2026-09-01');
    expect(dayGroupLabel('', now, t)).toBe('history.unknownDay');
  });

  it('跨月/跨年边界按日期字符串判断，不受月份天数影响', () => {
    const now = new Date(2026, 2, 1, 0, 30); // 3 月 1 日
    expect(dayGroupLabel('2026-02-28', now, t)).toBe('history.yesterday');
  });
});
