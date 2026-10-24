import { describe, it, expect } from 'vitest';

// R407：内联样式里的语义色不许写死 hex——一律走 tokens.css 的 var(--…)。
// 此前状态徽标、可写警告条、未知脚手架提示各自写死 #e05c51 / #fff3cd / #ffc107 / #b45309，
// 换主题（尤其深色）时它们不跟着反相，也不再受设计系统统一调整。
// 允许的形式：var(--token) 或 var(--token, #fallback) 这类带回退的变量引用；
// 调色板数据（categoryColor / 文件类型色 / 主题色板 / 图表系列色）不在 style 对象里，不受影响。
const RAW_TSX = import.meta.glob('./**/*.tsx', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

// 例外：坏点测试的覆盖层必须渲染纯白/纯黑/纯 RGB，才能让用户肉眼判死点——
// 这里写死是测试语义，不是偷懒（换成 token 反而测不出。注释里同样不写那些字面量，
// 免得被自己的正则命中）。
const INLINE_COLOR_EXEMPT = ['./components/toolbelt/gpu-donut.tsx'];

const WHOLE_HEX = /:\s*'(#[0-9a-fA-F]{3,8})'/g;
const SHORTHAND_HEX = /(?:border|outline)[\w-]*\s*:\s*'[^'#]+#[0-9a-fA-F]{3,8}[^']*'/g;

function inlineColorOffenders(tsx: Record<string, string>): string[] {
  const bad: string[] = [];
  for (const [path, raw] of Object.entries(tsx)) {
    if (/\.test\.tsx$/.test(path)) continue;
    if (INLINE_COLOR_EXEMPT.includes(path)) continue;
    const src = raw.replace(/\{\s*\/\*[\s\S]*?\*\/\s*\}/g, ' ');
    for (const m of src.matchAll(/style=\{\{([\s\S]{0,400}?)\}\}/g)) {
      const body = m[1];
      for (const hit of body.matchAll(WHOLE_HEX)) {
        const line = src.slice(0, m.index + hit.index).split('\n').length;
        bad.push(path + ':' + line + ' 内联 ' + hit[0].trim() + ' 写死颜色，改用 var(--token)');
      }
      for (const hit of body.matchAll(SHORTHAND_HEX)) {
        const line = src.slice(0, m.index + hit.index).split('\n').length;
        bad.push(path + ':' + line + ' 内联 ' + hit[0].trim().slice(0, 40) + ' 写死颜色，改用 var(--token)');
      }
    }
  }
  return bad;
}

describe('内联样式不许写死语义色（R407）', () => {
  it('扫到了足够多源码（守卫不是空转）', () => {
    expect(Object.keys(RAW_TSX).length).toBeGreaterThan(20);
  });

  it('style={{…}} 里没有裸 hex 颜色', () => {
    expect(inlineColorOffenders(RAW_TSX)).toEqual([]);
  });

  it('能抓到写死的颜色（自检）', () => {
    const fake = {
      './fake.tsx': "<span style={{ color: '#b45309', borderLeft: '4px solid #ffc107' }} />",
    };
    const hits = inlineColorOffenders(fake);
    expect(hits.length).toBe(2);
    const token = { './ok.tsx': "<span style={{ color: 'var(--risk-caution)' }} />" };
    expect(inlineColorOffenders(token)).toEqual([]);
  });
});
