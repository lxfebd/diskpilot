import { describe, it, expect } from 'vitest';

// seg 分段控件：基础 .seg 是 3 列网格（layout.css），任何 seg-N 变体使用前
// 必须有对应的 .seg-N 规则，否则选项会按 3 列栅格布局（个数不为 3 时留空列/换行）。

const RAW_TSX = import.meta.glob('./**/*.tsx', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const RAW_CSS = import.meta.glob('./**/*.css', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

describe('seg-N 变体都要有网格规则（R397）', () => {
  it('TSX 里出现的每个 seg-N 都必须在某个 css 里有 .seg-N 规则', () => {
    const layoutCss = Object.entries(RAW_CSS)
      .filter(([p]) => p.includes('/styles/'))
      .map(([, s]) => s)
      .join('\n');
    const violations: string[] = [];
    for (const [rel, src] of Object.entries(RAW_TSX)) {
      if (rel.includes('.test.')) continue;
      const seen = new Set<string>();
      for (const m of src.matchAll(/seg-(\d+)/g)) {
        const n = m[1];
        if (seen.has(n)) continue;
        seen.add(n);
        if (!new RegExp('\\.seg-' + n + '\\s*\\{').test(layoutCss)) {
          violations.push(rel + ':' + src.slice(0, m.index!).split('\n').length + ': seg-' + n);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});
