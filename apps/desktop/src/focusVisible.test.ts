import { describe, it, expect } from 'vitest';

// R403：自定义可聚焦控件的焦点可见性守卫。
// 原生 button/input 有浏览器默认 focus ring，且 app-shell.css 统一过 :focus-visible 口径；
// 但 role="button" / tabIndex={0} 的 div/span/li 什么都没有——键盘 Tab 过去时屏幕上看不出
// 焦点在哪（R314/R322/R368/R371 都是逐个补的，没有机器守卫）。这里把「可键盘聚焦的标签」
// 与「CSS 里带 :focus-visible 的类名」对账，谁没有焦点样式就报出文件行号。
const RAW_TSX = import.meta.glob('./**/*.tsx', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;
const RAW_CSS = import.meta.glob('./**/*.css', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

// 任何写了 :focus-visible 的选择器里出现的类名，都算「这个类有焦点样式」。
function focusVisibleClasses(css: Record<string, string>): Set<string> {
  const out = new Set<string>();
  for (const raw of Object.values(css)) {
    const text = raw.replace(/\/\*[\s\S]*?\*\//g, ' ');
    for (const m of text.matchAll(/([^{}]+)\{/g)) {
      const sel = m[1];
      if (!sel.includes(':focus-visible')) continue;
      for (const c of sel.matchAll(/\.([\w-]+)/g)) out.add(c[1]);
    }
  }
  return out;
}

// 去掉 JSX 注释与行注释，避免注释里的 role="button" 冒充真标签。
function stripComments(src: string): string {
  return src
    .replace(/\{\s*\/\*[\s\S]*?\*\/\s*\}/g, (m) => m.replace(/[^\n]/g, ' '))
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
    .replace(/(^|[^:'"`])\/\/[^\n]*/g, (_m, pre: string) => pre);
}

// 从 < 开始找标签结束的 >：跟踪花括号深度与引号，避免被 => 或字符串里的 > 截断。
function tagEndOf(src: string, start: number): number {
  let depth = 0;
  let quote: string | null = null;
  for (let i = start; i < src.length; i++) {
    const ch = src[i];
    if (quote) {
      if (ch === quote) quote = null;
      else if (ch === '\\') i++;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === '`') {
      quote = ch;
      continue;
    }
    if (ch === '{') depth++;
    else if (ch === '}') {
      depth--;
      continue;
    } else if (ch === '>' && depth <= 0) return i;
  }
  return -1;
}

const CLASS_TOKEN = /^[a-z][\w-]*$/;
function classTokens(value: string): string[] {
  return value.split(/[\s{}+()?:]+/).filter((t) => CLASS_TOKEN.test(t));
}

function offenders(tsx: Record<string, string>, css: Record<string, string>): string[] {
  const covered = focusVisibleClasses(css);
  const bad: string[] = [];
  for (const [path, raw] of Object.entries(tsx)) {
    if (/\.test\.tsx$/.test(path)) continue;
    const src = stripComments(raw);
    for (const m of src.matchAll(/<([A-Za-z][\w.]*)\b/g)) {
      const end = tagEndOf(src, m.index);
      if (end < 0) continue;
      const tag = src.slice(m.index, end + 1);
      if (!/tabIndex=\{0\}/.test(tag) && !/role="button"/.test(tag)) continue;
      // 本标签上的静态类名；没有就看紧邻的父层 className（如 li 在 ul[className] 里）。
      let tokens: string[] = [];
      const own = /className=\s*(?:{\s*)?(['"`])([\s\S]*?)\1/.exec(tag);
      if (own) tokens = classTokens(own[2]);
      if (tokens.length === 0) {
        const near = /className=\s*(?:{\s*)?(['"`])([\s\S]*?)\1/.exec(
          src.slice(Math.max(0, m.index - 400), m.index),
        );
        if (near) tokens = classTokens(near[2]);
      }
      const line = src.slice(0, m.index).split('\n').length;
      const where = `${path}:${line} <${m[1]}>`;
      if (tokens.length === 0) bad.push(`${where} 找不到对应的类名，无法确认焦点样式`);
      else if (!tokens.some((t) => covered.has(t))) {
        bad.push(`${where} 类名 ${tokens.join('/')} 没有 :focus-visible 规则`);
      }
    }
  }
  return bad;
}

describe('键盘焦点要看得见（R403）', () => {
  it('源码与样式都扫到了（守卫不是空转）', () => {
    expect(Object.keys(RAW_TSX).length).toBeGreaterThan(20);
    expect(focusVisibleClasses(RAW_CSS).size).toBeGreaterThanOrEqual(5);
  });

  it('每个 tabIndex={0} / role=button 的控件都有 :focus-visible 样式', () => {
    expect(offenders(RAW_TSX, RAW_CSS)).toEqual([]);
  });

  it('能抓到没有焦点样式的类（自检）', () => {
    const fakeTsx = {
      './fake.tsx': '<div className="no-focus" role="button" tabIndex={0} />',
    };
    const fakeCss = { './a.css': '.other:focus-visible { outline: 1px solid red; }' };
    expect(offenders(fakeTsx, fakeCss)).toEqual([
      './fake.tsx:1 <div> 类名 no-focus 没有 :focus-visible 规则',
    ]);
  });
});
