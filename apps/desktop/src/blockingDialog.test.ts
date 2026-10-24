import { describe, it, expect } from 'vitest';
//
// 铁律 6 的机器守卫：破坏性命令不许用阻塞式对话框（window.confirm/alert），
// 确认必须走界面内两步确认（ConfirmDialog）。window.prompt 只允许一个例外——
// 浏览器预览没有原生目录选择器，App.tsx 的 pickDirectory 在 !isTauri 分支里
// 用它输入路径；这个回退要出现必须带着 isTauri 判定一起写。
const RAW = import.meta.glob('./**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

describe('破坏性命令不许用阻塞式对话框（铁律 6）', () => {
  it('window.confirm / alert 全仓 0 处；window.prompt 仅允许 !isTauri 预览回退', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW)) {
      if (path.includes('.test.')) continue;
      const lines = src.split('\n');
      lines.forEach((line, i) => {
        const m = line.match(/window\.(confirm|alert|prompt)\s*\(/);
        if (!m) return;
        if (m[1] === 'prompt') {
          // 例外：浏览器预览选目录。判定上下文里必须有 !isTauri（8 行内）。
          const ctx = lines.slice(Math.max(0, i - 8), i).join('\n');
          if (ctx.includes('!isTauri')) return;
          violations.push(`${path}:${i + 1} window.prompt 不在 !isTauri 预览分支里`);
          return;
        }
        violations.push(`${path}:${i + 1} window.${m[1]} 是阻塞式对话框，改走界面内确认`);
      });
    }
    expect(violations).toEqual([]);
  });
});
