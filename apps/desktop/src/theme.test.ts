import { describe, it, expect, vi, afterEach } from 'vitest';
import type { ThemeId } from './theme';
import {
  THEME_FONTS,
  applyTheme,
  initTheme,
  loadTheme,
  saveTheme,
  setPref,
  setKeepFiles,
  getKeepFiles,
  prefs,
} from './theme';

// ── 主题四张事实表互相对偶（R423）─────────────────────────────
// ThemeId 联合 / THEME_FONTS / PRESETS 必须同键：任何一张漏了某一个 id，
// 切到那个主题的字体/色板就静默回退 vercel——不报错，只错视觉（R420 曾修
// 过一回「换主题漏配」）。PRESETS 未导出，用 ?raw 抓源码做静态断言（与
// a11y.test.ts 同款手法）。所有正则刻意避开反斜杠，方便 .test.ts 直读。
const THEME_SRC: string = (import.meta.glob('./theme.ts', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>)['./theme.ts'] ?? '';

function parseThemeIds(src: string): string[] {
  const start = src.indexOf('export type ThemeId =');
  const end = src.indexOf(';', start);
  if (start < 0 || end < 0) return [];
  const block = src.slice(start, end);
  return [...block.matchAll(/'([a-z0-9]+)'/g)].map((m) => m[1]);
}

function parsePresetBlocks(src: string): Array<{ id: string; keys: string[]; radius: string | null }> {
  const start = src.indexOf('const PRESETS');
  const end = src.indexOf('const ALL_VARS');
  if (start < 0 || end < 0) return [];
  const body = src.slice(start, end);
  const lines = body.split(String.fromCharCode(10));
  const out: Array<{ id: string; keys: string[]; radius: string | null }> = [];
  let cur: { id: string; keys: string[]; radius: string | null } | null = null;
  for (const line of lines) {
    const L = line.endsWith(String.fromCharCode(13)) ? line.slice(0, -1) : line;
    const head = L.match(/^  ('?[a-z0-9]+'?): \{$/);
    if (head) {
      if (cur) out.push(cur);
      cur = { id: head[1].replace(/'/g, ''), keys: [], radius: null };
    } else if (cur) {
      for (const km of L.matchAll(/'(--[a-z0-9-]+)'/g)) cur.keys.push(km[1]);
      const rad = L.match(/'--radius': '([^']+)'/);
      if (rad) cur.radius = rad[1];
    }
  }
  if (cur) out.push(cur);
  return out;
}

const THEME_IDS = parseThemeIds(THEME_SRC);
const PRESETS = parsePresetBlocks(THEME_SRC);

describe('主题四张事实表对偶（R423）', () => {
  it('ThemeId 联合恰好 17 项且无重复；PRESETS / THEME_FONTS 与它逐键相等', () => {
    expect(THEME_IDS.length).toBe(17);
    expect(new Set(THEME_IDS).size).toBe(17);
    const presetIds = PRESETS.map((p) => p.id).sort();
    const fontIds = Object.keys(THEME_FONTS).sort();
    expect(presetIds).toEqual([...THEME_IDS].sort());
    expect(fontIds).toEqual([...THEME_IDS].sort());
  });

  it('17 个预设的公共语义键交集必须覆盖 R420 语义链（缺任何链环当场红）', () => {
    const intersection = PRESETS[0]?.keys.filter((k) => PRESETS.every((p) => p.keys.includes(k))) ?? [];
    const critical = [
      '--bg', '--surface', '--text', '--border',
      '--accent', '--accent-strong', '--accent-soft', '--accent-bg',
      '--risk-safe', '--risk-caution', '--risk-danger',
      '--ok', '--warn', '--err', '--line',
    ];
    expect(PRESETS.length).toBe(17);
    for (const k of critical) expect(intersection, '交集缺 ' + k).toContain(k);
  });

  it('每个预设必须全部包含当前交集键——任一主题删一个键就红，不许静默漏配', () => {
    const intersection = PRESETS[0]?.keys.filter((k) => PRESETS.every((p) => p.keys.includes(k))) ?? [];
    for (const p of PRESETS) {
      for (const k of intersection) expect(p.keys, p.id + ' 缺 ' + k).toContain(k);
    }
  });

  it('圆角口径分册：9 个主题自带 --radius，其余 8 个明确回退 tokens.css 的 1rem', () => {
    // tokens.css:72 注释「主题预设可覆盖；派生档位让整套 UI 的圆角语言跟随
    // 主题性格」。给某个主题补/改 --radius 是视觉决定，必须同步改这张表。
    const expected: Record<string, string> = {
      minimal: '0.75rem', doubao: '1.2rem', claude: '1rem', google: '1rem',
      barbie: '1.8rem', brand: '2rem', golden: '1.2rem', vibecamp: '1rem', '21th': '0',
    };
    const actual: Record<string, string> = {};
    const fallback: string[] = [];
    for (const p of PRESETS) {
      if (p.radius !== null) actual[p.id] = p.radius;
      else fallback.push(p.id);
    }
    expect(actual).toEqual(expected);
    expect(fallback.sort()).toEqual(['dark', 'motionfit', 'nerv', 'pro', 'steam', 'trae', 'vercel', 'volcengine']);
  });

  it('DARK_IDS 恰好 5 个深色主题，且都在 ThemeId 里', () => {
    const dark = ['nerv', 'motionfit', 'steam', 'dark', 'pro'];
    for (const d of dark) expect(THEME_IDS).toContain(d);
    expect(THEME_IDS.filter((id) => dark.includes(id)).length).toBe(5);
  });
});

// ── 行为单测：localStorage / document 都是函数内访问，stub 即可 ────
const THEME_KEY = 'diskpilot.theme';
const KEEP_KEY = 'diskpilot.general.keepFilesPerDir';

function stubStorage(map: Map<string, string>): void {
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => { map.set(k, v); },
    removeItem: (k: string) => { map.delete(k); },
  });
}

function stubDom(): {
  props: Record<string, string>; attrs: Record<string, string>;
  events: Array<{ type: string; detail: unknown }>;
} {
  const props: Record<string, string> = {};
  const attrs: Record<string, string> = {};
  const events: Array<{ type: string; detail: unknown }> = [];
  vi.stubGlobal('document', {
    documentElement: {
      style: {
        setProperty: (k: string, v: string) => { props[k] = v; },
        removeProperty: (k: string) => { delete props[k]; },
        colorScheme: '',
      },
      setAttribute: (k: string, v: string) => { attrs[k] = v; },
    },
  });
  vi.stubGlobal('window', {
    dispatchEvent: (e: { type: string; detail: unknown }) => { events.push({ type: e.type, detail: e.detail }); },
  });
  return { props, attrs, events };
}

afterEach(() => vi.unstubAllGlobals());

describe('loadTheme 回落矩阵（R423）', () => {
  it('无存值 → vercel 默认', () => {
    stubStorage(new Map());
    expect(loadTheme()).toEqual({ id: 'vercel', accent: null, fontScale: 1 });
  });
  it('坏 JSON → 回落默认不炸', () => {
    stubStorage(new Map([[THEME_KEY, '{oops']]));
    expect(loadTheme()).toEqual({ id: 'vercel', accent: null, fontScale: 1 });
  });
  it('未知 id / 非法 accent / 非法 fontScale 分别回落', () => {
    stubStorage(new Map([[THEME_KEY, JSON.stringify({ id: 'not-a-theme', accent: 'red', fontScale: 'x' })]]));
    expect(loadTheme()).toEqual({ id: 'vercel', accent: null, fontScale: 1 });
  });
  it('accent 只收 #rrggbb——#12345 与 123456 都不认', () => {
    stubStorage(new Map([[THEME_KEY, JSON.stringify({ id: 'vercel', accent: '#12345', fontScale: 1 })]]));
    expect(loadTheme().accent).toBeNull();
  });
  it('合法值原样透传', () => {
    stubStorage(new Map([[THEME_KEY, JSON.stringify({ id: 'nerv', accent: '#1a2b3c', fontScale: 1.2 })]]));
    expect(loadTheme()).toEqual({ id: 'nerv', accent: '#1a2b3c', fontScale: 1.2 });
  });
});

describe('applyTheme 写 DOM（R423）', () => {
  it('浅色主题：data-theme / data-mode=light / 色板落地', () => {
    const dom = stubDom();
    applyTheme({ id: 'vercel', accent: null, fontScale: 1 });
    expect(dom.attrs['data-theme']).toBe('vercel');
    expect(dom.attrs['data-mode']).toBe('light');
    expect(dom.props['--bg']).toBe('#fafafa');
    expect(dom.props['--accent']).toBe('#1447e6');
  });
  it('深色主题：data-mode=dark + colorScheme', () => {
    const dom = stubDom();
    applyTheme({ id: 'nerv', accent: null, fontScale: 1 });
    expect(dom.attrs['data-mode']).toBe('dark');
  });
  it('未知 id 回退 vercel 色板', () => {
    const dom = stubDom();
    applyTheme({ id: 'nope' as unknown as ThemeId, accent: null, fontScale: 1 });
    expect(dom.props['--accent']).toBe('#1447e6');
    expect(dom.props['--bg']).toBe('#fafafa');
  });
  it('自定义 accent：提亮档白底深字并覆盖 --ok/--risk-safe；压暗档白字不动状态色', () => {
    const dom = stubDom();
    applyTheme({ id: 'vercel', accent: '#3ca0ff', fontScale: 1 });
    expect(dom.props['--pink']).toBe('#3ca0ff');
    expect(dom.props['--accent']).toBe('#3ca0ff');
    expect(dom.props['--on-accent']).toBe('#1a1a1a');
    expect(dom.props['--ok']).toBe('#3ca0ff');
    expect(dom.props['--risk-safe']).toBe('#3ca0ff');
    applyTheme({ id: 'vercel', accent: '#123456', fontScale: 1 });
    expect(dom.props['--on-accent']).toBe('#ffffff');
    expect(dom.props['--ok']).toBe('#16a34a'); // 亮度 0.185 太暗，状态色保持 preset
  });
  it('fontScale 钳制 0.8–1.3 并写 --font-scale', () => {
    const dom = stubDom();
    applyTheme({ id: 'vercel', accent: null, fontScale: 2 });
    expect(dom.props['--font-scale']).toBe('1.3');
    applyTheme({ id: 'vercel', accent: null, fontScale: 0.4 });
    expect(dom.props['--font-scale']).toBe('0.8');
  });
});

describe('saveTheme / initTheme（R423）', () => {
  it('写 localStorage + 应用主题 + 广播 diskpilot:theme-changed', () => {
    const map = new Map<string, string>();
    stubStorage(map);
    const dom = stubDom();
    saveTheme({ id: 'dark', accent: null, fontScale: 1.1 });
    expect(JSON.parse(map.get(THEME_KEY) ?? '')).toEqual({ id: 'dark', accent: null, fontScale: 1.1 });
    expect(dom.attrs['data-theme']).toBe('dark');
    expect(dom.events.length).toBe(1);
    expect(dom.events[0].type).toBe('diskpilot:theme-changed');
    expect((dom.events[0].detail as { id: string }).id).toBe('dark');
  });
  it('initTheme 按已存配置应用（启动无闪变路径）', () => {
    stubStorage(new Map([[THEME_KEY, JSON.stringify({ id: 'steam', accent: null, fontScale: 1 })]]));
    const dom = stubDom();
    initTheme();
    expect(dom.attrs['data-theme']).toBe('steam');
    expect(dom.attrs['data-mode']).toBe('dark');
  });
});

describe('通用偏好与保留数量（R423）', () => {
  it('getKeepFiles 钳制 50–2000，非法回落 500', () => {
    stubStorage(new Map());
    expect(getKeepFiles()).toBe(500);
    for (const [raw, want] of [['49', 500], ['50', 50], ['2000', 2000], ['2001', 500], ['abc', 500]]) {
      stubStorage(new Map([[KEEP_KEY, String(raw)]]));
      expect(getKeepFiles()).toBe(want);
    }
    stubStorage(new Map([[KEEP_KEY, 'NaN']]));
    expect(getKeepFiles()).toBe(500);
  });
  it('setKeepFiles 写回字符串', () => {
    const map = new Map<string, string>();
    stubStorage(map);
    setKeepFiles(123);
    expect(map.get(KEEP_KEY)).toBe('123');
  });
  it('webEnabled 默认开；存 0 关、存 1 与怪异值按开（非 0 即开）', () => {
    stubStorage(new Map());
    expect(prefs.webEnabled).toBe(true);
    stubStorage(new Map([['diskpilot.agent.web', '0']]));
    expect(prefs.webEnabled).toBe(false);
    stubStorage(new Map([['diskpilot.agent.web', '1']]));
    expect(prefs.webEnabled).toBe(true);
    stubStorage(new Map([['diskpilot.agent.web', 'yes']]));
    expect(prefs.webEnabled).toBe(true);
  });
  it('setPref 写 1/0', () => {
    const map = new Map<string, string>();
    stubStorage(map);
    setPref('showThinking', false);
    expect(map.get('diskpilot.agent.thinking')).toBe('0');
    setPref('showThinking', true);
    expect(map.get('diskpilot.agent.thinking')).toBe('1');
  });
  it('存储抛异常：读取回落默认、写入静默', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => { throw new Error('quota'); },
      setItem: () => { throw new Error('quota'); },
      removeItem: () => { throw new Error('quota'); },
    });
    stubDom();
    expect(getKeepFiles()).toBe(500);
    expect(prefs.webEnabled).toBe(true);
    expect(loadTheme()).toEqual({ id: 'vercel', accent: null, fontScale: 1 });
    expect(() => saveTheme({ id: 'vercel', accent: null, fontScale: 1 })).not.toThrow();
  });
});
