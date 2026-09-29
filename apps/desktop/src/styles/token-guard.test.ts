// ── 样式 token 守卫 ────────────────────────────────────────────────
// 白名单 tokens.css / dark.css 之外禁止新增硬编码 rgba( 颜色：字面色值
// 绕过 token 体系，暗色主题必然漏配。存量（2026-09-29 实测基线）钉死
// 只许减不许增——清一处就把基线同步减一，新增则直接红。
// 源文件用 ?raw 抓取（vite 原生能力，不引 node:fs / @types/node）；
// 依赖 vitest.config.ts 的 css:true——默认 css:false 会把 CSS 导入换成空模块。
// 计数按出现次数（非行数）：同一行多个 rgba( 各算一处。
import { describe, expect, it } from 'vitest';

const SOURCES = import.meta.glob('./*.css', {
  eager: true,
  as: 'raw',
}) as Record<string, string>;

const WHITELIST = new Set(['./tokens.css', './dark.css']);

const BASELINE: Record<string, number> = {
  './chat.css': 14,
  './toolwall.css': 6,
  './settings.css': 5,
  './overview.css': 3,
  './cleanup.css': 2,
  './layout.css': 2,
  './steam.css': 1,
};

describe('样式 token 守卫', () => {
  it('非白名单样式文件的硬编码 rgba( 数不超过基线', () => {
    const offenders: string[] = [];
    for (const [path, css] of Object.entries(SOURCES)) {
      if (WHITELIST.has(path)) continue;
      const count = (css.match(/rgba\(/g) ?? []).length;
      const allowed = BASELINE[path] ?? 0;
      if (count > allowed) offenders.push(`${path}：${count} 处 > 基线 ${allowed}`);
    }
    expect(offenders).toEqual([]);
  });
});
