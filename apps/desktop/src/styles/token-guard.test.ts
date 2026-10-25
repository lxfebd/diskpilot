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

// ── 裸 hex 语义色基线（R420）───────────────────────────────
// 主题机制是 applyTheme() 把整套语义 token 写成 <html> 内联变量；CSS 里写死的非主题
// hex 换主题时必然漏配（浅粉分隔线在深色主题下不变量就是 R420 改掉的例子）。规则：
// ① var(--x, #hex) 回退与 color-mix(...) 调用内不算裸色（token 决定，回退只是兜底）；
// ② #fff/#ffffff 视为恒定前景（白字/白底）自动放行；③ 其余裸 hex 必须落在 HEX_KEEP
// 册里（仪器色 / editorial 固定色，按文件分册）——新增裸语义色直接红：先改语义 token，
// 确属固定色的才允许进册并注明理由。
const HEX_KEEP: Record<string, string[]> = {
  // 代码块固定深色 editorial 卡（chat.css:339 注释：dark editorial card）——亮/暗主题下都保持深底浅字，
  // 故不随 token 走（与工具墙 donut 仪器色同理）。
  './chat.css': ['#1a1220', '#f5e8ee'],
  // GPU donut 仪表（黑场上的表盘绿）与退出 hover 的警示红——仪器用色，主题无关。
  './toolwall.css': ['#080612', '#7dfc9f', '#ff5252', '#ff8080'],
};

describe('裸 hex 语义色基线（R420）', () => {
  it('非白名单样式文件不出现未入册的裸 hex 语义色', () => {
    const offenders: string[] = [];
    const stripNaked = (cssRaw: string): string => {
      let s = cssRaw.replace(/\/\*[\s\S]*?\*\//g, ' ');
      for (let i = 0; i < 6; i++) s = s.replace(/var\([^()]*\)/g, '').replace(/color-mix\([^()]*\)/g, '');
      return s;
    };
    for (const [path, cssRaw] of Object.entries(SOURCES)) {
      if (WHITELIST.has(path)) continue;
      const hexes = (stripNaked(cssRaw).match(/#[0-9a-fA-F]{3,8}/g) ?? []).map(h => h.toLowerCase())
        .filter(h => h !== '#fff' && h !== '#ffffff');
      const kept = (HEX_KEEP[path] ?? []).map(h => h.toLowerCase());
      const bad = [...new Set(hexes.filter(h => !kept.includes(h)))];
      if (bad.length) offenders.push(`${path} 出现未入册裸 hex：${bad.join('、')}（改语义 token，或进册注明理由）`);
    }
    expect(offenders).toEqual([]);
  });
});

// ── 键盘焦点描边（R431）───────────────────────────────
// layout.css:226 的全局 ring（input/select:focus-visible）与 R405 的 textarea:focus-visible，
// 会被更晚加载的样式文件里等特异性的 outline: none 覆盖（.hp-search input 与
// input:focus-visible 同为 0,1,1，后写者赢）——键盘焦点就只剩容器边框换色。
// 规则：任何选择器里点了 input/textarea 的规则块，只要写了 outline: none，
// 同文件必须另有 :focus-visible 规则给出 2px accent-strong 描边（R348/R405 口径）。
describe('键盘焦点描边（R431）', () => {
  it('凡在 input/textarea 上抹掉 outline 的文件，都必须自带 :focus-visible 描边', () => {
    const RING = 'outline: 2px solid var(--accent-strong)';
    const offenders: string[] = [];
    for (const [path, cssRaw] of Object.entries(SOURCES)) {
      const src = cssRaw.replace(/\/\*[\s\S]*?\*\//g, ' ');
      const blocks = [...src.matchAll(/([^{}]+)\{([^}]*)\}/g)];
      const silenced = blocks.filter(
        (m) => /(^|[\s,>])input\b|(^|[\s,>])textarea\b/.test(m[1]) && /outline\s*:\s*none/.test(m[2]),
      );
      if (silenced.length === 0) continue;
      const ringed = blocks.some((m) => m[1].includes(':focus-visible') && m[2].includes(RING));
      if (!ringed) {
        const names = silenced.map((m) => m[1].trim().replace(/\s+/g, ' ')).join(' / ');
        offenders.push(path + '：' + names);
      }
    }
    expect(offenders).toEqual([]);
  });
});

// R456：ConfirmDialog 卡片此前没有 max-height/overflow——遮罩只设 padding-top: 15vh
// 且自身不滚动，小窗（150% 缩放的笔记本）或长正文时卡片底部超出视口，
// 确认/取消按钮被裁到屏外，只能靠 Esc 脱身。规则：卡片必须限制在视口内、正文自己滚动。
describe('确认弹窗小窗不撑破视口（R456）', () => {
  it('confirm-dialog-mask 下的卡片限制在视口内，正文自己滚动', () => {
    const chat = SOURCES['./chat.css'];
    const start = chat.indexOf('.confirm-dialog-mask .cli-confirm {');
    expect(start).toBeGreaterThan(-1);
    const rule = chat.slice(start, chat.indexOf('}', start) + 1);
    expect(rule).toContain('max-height: calc(100vh - 15vh - 24px)');
    expect(rule).toContain('overflow: hidden');
    expect(chat).toContain('.confirm-dialog-mask .cli-confirm-head,');
    expect(chat).toContain('.confirm-dialog-mask .cli-confirm-actions { flex-shrink: 0; }');
    expect(chat).toContain('.confirm-dialog-mask .cli-confirm-body { overflow-y: auto; min-height: 0; }');
  });
});

// R457：toast 文本可能携带无空格的 Windows 长路径 / 原始 OS 错误串——没有断行机会时
// 文字会溢出卡片、糊到页面上。规则：.toast-text 必须允许任意位置折行，且保持可收缩。
describe('toast 长文本不溢出卡片（R457）', () => {
  it('.toast-text 有 overflow-wrap 与收缩许可，卡片保持可点', () => {
    const ov = SOURCES['./overview.css'];
    const start = ov.indexOf('.toast-text {');
    expect(start).toBeGreaterThan(-1);
    const rule = ov.slice(start, ov.indexOf('}', start) + 1);
    expect(rule).toContain('overflow-wrap: anywhere');
    expect(rule).toContain('min-width: 0');
    const tstart = ov.indexOf('.toast {');
    const tcard = ov.slice(tstart, ov.indexOf('}', tstart) + 1);
    expect(tcard).toContain('pointer-events: auto');
  });
});
