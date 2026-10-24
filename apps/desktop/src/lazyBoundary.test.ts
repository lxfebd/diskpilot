import { describe, it, expect } from 'vitest';

// R394：这些是懒加载（React.lazy + Suspense）挂载的页面/面板。Suspense 只接 pending，
// 不接错误——懒加载 chunk 取不到、渲染抛错时，没有 ErrorBoundary 就会让整棵树崩掉。
// 守卫用源码静态检查：每个挂载点前面近处必须有 <ErrorBoundary。
const RAW_TSX = import.meta.glob('./**/*.tsx', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

const SURFACES = [
  'AssetOverview',
  'Toolbelt',
  'CleanupPage',
  'OptimizerPage',
  'HistoryPage',
  'ChatPanel',
  'Studio',
  'Settings',
];

describe('懒加载的页面/面板必须有错误兜底（R394）', () => {
  it('App.tsx 里每个懒加载挂载点都在 ErrorBoundary 之内', () => {
    const app = RAW_TSX['./App.tsx'];
    expect(app).toBeTruthy();
    const missing: string[] = [];
    for (const name of SURFACES) {
      const re = new RegExp('<' + name + '\\b', 'g');
      for (const m of app.matchAll(re)) {
        const before = app.slice(Math.max(0, (m.index ?? 0) - 400), m.index ?? 0);
        // 只看本挂载点之前最近的 <Suspense → 判定它有没有 ErrorBoundary 包裹
        if (!before.includes('<ErrorBoundary')) {
          missing.push(name + ' @' + app.slice(0, m.index ?? 0).split('\n').length);
          break;
        }
      }
    }
    expect(missing).toEqual([]);
  });
});
