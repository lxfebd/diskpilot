import { describe, expect, it } from 'vitest';
import { historyListState } from './HistoryPage';

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