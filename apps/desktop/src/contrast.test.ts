import { describe, expect, it } from 'vitest';

// R475：状态色与亮底小标签的文字对比度守卫。
// 背景：--mint/--sun/--lavender/--accent/--err 都会当实心底色画在徽标、chip、
// 横幅、气泡上，而 --ink 是「主题正文色」——5 个深色主题里它是白字，于是白字
// 落在荧光绿/亮黄上只剩 1.1:1（WCAG 正文要 4.5）。R475 引入 --on-tint（亮底
// 深字）与 --on-danger（白/深字按对比度取优）两个 token 收敛这类组合；本文件
// 既算色板对比度，也守住「CSS 里不许再出现 ink 落在实心亮底上」这条线。

const SRC = import.meta.glob('./theme.ts', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const CSS = import.meta.glob('./styles/*.css', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const NL = String.fromCharCode(10);
const CR = String.fromCharCode(13);

/** hex → 三个 0-255 通道；非 #rrggbb 返回 null。 */
function rgb(hex: string): [number, number, number] | null {
  const s = hex.trim().replace('#', '');
  if (!/^[0-9a-fA-F]{6}$/.test(s)) return null;
  return [0, 2, 4].map((i) => parseInt(s.slice(i, i + 2), 16)) as [number, number, number];
}

function luminance(hex: string): number {
  const c = rgb(hex);
  if (!c) throw new Error('bad hex: ' + hex);
  const ch = (v: number) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2]);
}

function contrast(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  const hi = Math.max(la, lb);
  const lo = Math.min(la, lb);
  return (hi + 0.05) / (lo + 0.05);
}

function themeSource(): string {
  const key = Object.keys(SRC).find((k) => k.endsWith('theme.ts'));
  if (!key) throw new Error('theme.ts 未被 glob 到');
  return SRC[key];
}

/** 解析 theme.ts 的 PRESETS：该文件是 CRLF，逐行先剥回车；一行可有多条键值对。 */
function presets(): Record<string, Record<string, string>> {
  const lines = themeSource().split(NL).map((l) => (l.endsWith(CR) ? l.slice(0, -1) : l));
  const start = lines.findIndex((l) => l.includes('const PRESETS'));
  const end = lines.findIndex((l, i) => i > start && l.startsWith('};'));
  if (start < 0 || end <= start) throw new Error('PRESETS 块未找到');
  const out: Record<string, Record<string, string>> = {};
  let cur: Record<string, string> | null = null;
  for (let i = start; i < end; i++) {
    const line = lines[i];
    const head = line.match(/^  '?([a-z0-9]+)'?: \{$/);
    if (head) {
      cur = {};
      out[head[1]] = cur;
      continue;
    }
    if (!cur) continue;
    const re = new RegExp("'(--[a-z0-9-]+)': '([^']+)'", 'g');
    let m: RegExpExecArray | null;
    while ((m = re.exec(line)) !== null) cur[m[1]] = m[2];
  }
  return out;
}

const P = presets();
const IDS = Object.keys(P);

describe('主题色板的文字对比度（R475）', () => {
  it('解析器不空转：17 个预设都有必需 token', () => {
    expect(IDS.length).toBe(17);
    const need = ['--ink', '--bg', '--card', '--paper', '--accent', '--mint', '--sun', '--lavender', '--err'];
    for (const id of IDS) for (const k of need) expect(P[id][k], id + ' 缺 ' + k).toBeTruthy();
  });

  it('正文色 --ink 在三种底色上都 ≥4.5', () => {
    const bad: string[] = [];
    for (const id of IDS) {
      for (const bg of ['--bg', '--card', '--paper']) {
        const r = contrast(P[id]['--ink'], P[id][bg]);
        if (r < 4.5) bad.push(id + ': ink on ' + bg + ' = ' + r.toFixed(2));
      }
    }
    expect(bad).toEqual([]);
  });

  it('--on-accent 在主色底上 ≥4.5（白字/深字取对比度更优者）', () => {
    const bad: string[] = [];
    for (const id of IDS) {
      const accent = P[id]['--accent'];
      const on = contrast(accent, '#ffffff') > contrast(accent, '#101010') ? '#ffffff' : '#101010';
      const r = contrast(accent, on);
      if (r < 4.5) bad.push(id + ': on-accent ' + on + ' on ' + accent + ' = ' + r.toFixed(2));
    }
    expect(bad).toEqual([]);
  });

  it('--on-tint 在 mint/sun/lavender 亮底上 ≥4.5', () => {
    const bad: string[] = [];
    for (const id of IDS) {
      for (const tint of ['--mint', '--sun', '--lavender']) {
        const r = contrast(P[id][tint], '#101010');
        if (r < 4.5) bad.push(id + ': on-tint on ' + tint + ' = ' + r.toFixed(2));
      }
    }
    expect(bad).toEqual([]);
  });

  it('--on-danger 在 --err 上 ≥4.5（白字/深字取优）', () => {
    const bad: string[] = [];
    for (const id of IDS) {
      const err = P[id]['--err'];
      const best = Math.max(contrast(err, '#101010'), contrast(err, '#ffffff'));
      if (best < 4.5) bad.push(id + ': on-danger best = ' + best.toFixed(2));
    }
    expect(bad).toEqual([]);
  });

  it('applyTheme 真的写出这两个 token', () => {
    const src = themeSource();
    expect(src).toContain("el.setProperty('--on-tint', '#101010');");
    expect(src).toContain("el.setProperty('--on-danger', white > dark ? '#ffffff' : '#101010');");
    expect(src).toContain("contrastRatio(accent, '#101010')");
    expect(src).toContain("contrastRatio(a, '#101010')");
    expect(src).toContain('function contrastRatio(a: string, b: string): number');
  });

  it('CSS 里不再有「--ink 落在实心亮底上」的组合', () => {
    const bright = ['--mint', '--sun', '--lavender', '--accent', '--err'];
    const offenders: string[] = [];
    for (const key of Object.keys(CSS)) {
      const path = key.replace('./styles/', '');
      // 去掉注释：按 /* 切段，保留每段 */ 之后的内容
      const noComment = CSS[key]
        .split('/*')
        .map((s, i) => (i === 0 ? s : s.slice(s.indexOf('*/') + 2)))
        .join(' ');
      const re = /([^{}]+)\{([^{}]*)\}/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(noComment)) !== null) {
        const sel = m[1].replace(/\s+/g, ' ').trim();
        const body = m[2];
        if (!/(?:^|;|\s)color:\s*var\(--ink\)/.test(body)) continue;
        for (const b of bright) {
          if (body.includes('background: var(' + b + ')')) offenders.push(path + ' :: ' + sel + ' → ink on ' + b);
        }
      }
    }
    expect(offenders).toEqual([]);
  });

  it('危险按钮（risk-danger 底）不用写死的白字——白字在高饱和红上不足 4.5', () => {
    const offenders: string[] = [];
    for (const key of Object.keys(CSS)) {
      const path = key.replace('./styles/', '');
      const noComment = CSS[key]
        .split('/*')
        .map((s, i) => (i === 0 ? s : s.slice(s.indexOf('*/') + 2)))
        .join(' ');
      const re = /([^{}]+)\{([^{}]*)\}/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(noComment)) !== null) {
        const sel = m[1].replace(/\s+/g, ' ').trim();
        const body = m[2];
        if (!body.includes('background: var(--risk-danger)')) continue;
        if (body.includes('color: #fff') || body.includes('color: #ffffff')) offenders.push(path + ' :: ' + sel);
      }
    }
    expect(offenders).toEqual([]);
  });
});
