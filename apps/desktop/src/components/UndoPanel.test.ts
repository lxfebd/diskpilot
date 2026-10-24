import { describe, it, expect } from 'vitest';
import { undoCountInfo, UNDO_PANEL_LIMIT } from './UndoPanel';

describe('台账面板计数（R390 undoCountInfo）', () => {
  it('null / undefined 视为 0 条、不截断', () => {
    expect(undoCountInfo(null)).toEqual({ n: 0, truncated: false });
    expect(undoCountInfo(undefined)).toEqual({ n: 0, truncated: false });
  });

  it('不足上限显示真实条数、不截断', () => {
    expect(undoCountInfo(Array.from({ length: 10 }))).toEqual({ n: 10, truncated: false });
  });

  it('恰好 50 条保守视为截断（可能还有更早的没拉出来）', () => {
    const r = undoCountInfo(Array.from({ length: UNDO_PANEL_LIMIT }));
    expect(r).toEqual({ n: 50, truncated: true });
  });

  it('超过 50 条如实报告 n 并标记截断', () => {
    expect(undoCountInfo(Array.from({ length: 200 }))).toEqual({ n: 200, truncated: true });
  });

  it('limit=0（不拉取的守卫态）不算截断', () => {
    expect(undoCountInfo(Array.from({ length: 50 }), 0)).toEqual({ n: 50, truncated: false });
  });
});
