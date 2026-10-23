import { describe, expect, it } from 'vitest';
import { historyListState, historyTruncated } from './HistoryPage';

// R73 诚实性回归：列表体三态分支顺序——读失败（loadErr）必须在空态之前
// 判定。entries 折叠成 [] 时若 loadErr 在，必须显示「未读到」而不是
// 「还没有清理记录」。未读到 ≠ 空。
describe('historyListState（历史列表体三态）', () => {
  it('entries===null → loading（即使有 err？不：err 时 entries 已是 [] 或 null 由调用方决定）', () => {
    expect(historyListState(null, null)).toBe('loading');
  });

  it('读失败 + 空折叠 → unread（核心不变量：失败优先于空态）', () => {
    expect(historyListState([], 'backend down')).toBe('unread');
  });

  it('读失败 + 有残留数据 → unread（失败优先于非空展示？否——按约定失败即 unread）', () => {
    expect(historyListState([{ a: 1 }], 'backend down')).toBe('unread');
  });

  it('成功空数组 → empty（真空态，失败与空被区分）', () => {
    expect(historyListState([], null)).toBe('empty');
  });

  it('成功有数据 → nonempty', () => {
    expect(historyListState([{ a: 1 }], null)).toBe('nonempty');
  });
});
// R310：台账拉到上限时，「共 N 条 / 最早日期」只是已加载部分——必须明说截断。
// 后端只给 take(limit)、没有 total 可查，所以「恰好等量」保守按可能截断处理。
describe('historyTruncated（拉取上限截断判定）', () => {
  it('null（未加载）→ false：还没读到就别喊截断', () => {
    expect(historyTruncated(null, 500)).toBe(false);
  });

  it('少于上限 → false', () => {
    expect(historyTruncated(new Array(12).fill(0), 500)).toBe(false);
    expect(historyTruncated([], 500)).toBe(false);
  });

  it('恰好等于上限 → true（无 total 可查，保守视为可能还有更早记录）', () => {
    expect(historyTruncated(new Array(500).fill(0), 500)).toBe(true);
  });

  it('超过上限 → true', () => {
    expect(historyTruncated(new Array(900).fill(0), 500)).toBe(true);
  });

  it('上限非正 → false（0/负数不是有效拉取上限，不据此报截断）', () => {
    expect(historyTruncated(new Array(5).fill(0), 0)).toBe(false);
    expect(historyTruncated(new Array(5).fill(0), -1)).toBe(false);
  });
});