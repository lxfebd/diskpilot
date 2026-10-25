import { describe, it, expect, vi } from 'vitest';
import { activateOnKey, clearSearchOnEscape, isContextMenuKey, contextMenuAnchor, cycleIndex, isActivationKey } from './a11y';

describe('键盘激活口径（R314）', () => {
  it('只有 Enter 与空格算激活键（对齐原生 button）', () => {
    expect(isActivationKey('Enter')).toBe(true);
    expect(isActivationKey(' ')).toBe(true);
    expect(isActivationKey('a')).toBe(false);
    expect(isActivationKey('Escape')).toBe(false);
    expect(isActivationKey('Tab')).toBe(false);
    expect(isActivationKey('')).toBe(false);
  });

  it('Enter / 空格触发 handler 并阻止默认行为（空格不滚页面）', () => {
    for (const key of ['Enter', ' ']) {
      const handler = vi.fn();
      const preventDefault = vi.fn();
      activateOnKey(handler)({ key, preventDefault });
      expect(handler).toHaveBeenCalledTimes(1);
      expect(preventDefault).toHaveBeenCalledTimes(1);
    }
  });

  it('其他键既不触发也不拦（不能把 Tab / 方向键吃掉）', () => {
    const handler = vi.fn();
    const preventDefault = vi.fn();
    for (const key of ['Tab', 'a', 'ArrowDown', 'Escape']) {
      activateOnKey(handler)({ key, preventDefault });
    }
    expect(handler).not.toHaveBeenCalled();
    expect(preventDefault).not.toHaveBeenCalled();
  });

  it('事件对象没有 preventDefault 也不炸（合成事件最小形状）', () => {
    const handler = vi.fn();
    expect(() => activateOnKey(handler)({ key: 'Enter' })).not.toThrow();
    expect(handler).toHaveBeenCalledTimes(1);
  });
});

// ── 图标按钮的可访问名字守卫（R318）─────────────────────────────
// 只有图标的按钮（关闭 X、移除 X）对读屏软件是空的：必须靠 aria-label 说清
// 它是干什么的。这里扫全部组件源码，单图标、且没有 aria-label / title 的直接拦下。
const RAW_TSX = import.meta.glob('./**/*.tsx', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

// R458：这次断言的是 a11y.ts 自身（.ts 不在上面的 .tsx glob 里）
const RAW_A11Y = import.meta.glob('./a11y.ts', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

describe('图标按钮的可访问名字（R318）', () => {
  it('没有「只有图标、既无 aria-label 也无 title」的 button', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      if (/\.test\.tsx$/.test(path)) continue;
      const re = /<button\b([^>]*)>\s*(<[A-Z]\w*\b[^>]*\/>)\s*<\/button>/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(src))) {
        const attrs = m[1];
        if (/aria-label|aria-labelledby|title=/.test(attrs)) continue;
        const line = src.slice(0, m.index).split('\n').length;
        violations.push(`${path}:${line} ${m[2]}`);
      }
    }
    expect(violations).toEqual([]);
  });
});

// ── aria-label 必须走文案表（R319）─────────────────────────────
// 出现过 `aria-label="close"` / `aria-label="loading"`：中文界面里读屏软件念英文，
// 而且绕开了 zh/en 双表。这里禁止任何字面量 aria-label，必须 `aria-label={t('…')}`。
describe('aria-label 走文案表（R319）', () => {
  it('源码里没有字面量 aria-label', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      if (/\.test\.tsx$/.test(path)) continue;
      const re = /aria-label=\s*("([^"]*)"|'([^']*)')/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(src))) {
        const val = (m[2] ?? m[3] ?? '').trim();
        if (!val) continue;
        const line = src.slice(0, m.index).split('\n').length;
        violations.push(`${path}:${line} aria-label="${val}"`);
      }
    }
    expect(violations).toEqual([]);
  });
});

// ── 弹窗遮罩要说自己是对话框（R343）─────────────────────────
// 全仓 14 处 `className="modal-bg"` 遮罩此前只有鼠标 onClick 关闭，没有角色/模态语义：
// 读屏软件不知道弹出的是对话框，也不会把焦点限制在弹窗内。通用 ConfirmDialog 早已是
// role="alertdialog" aria-modal，这里把其余弹窗拉齐，并守住不许退回。
describe('弹窗遮罩的对话框语义（R343）', () => {
  it('每处 modal-bg 都带 role="dialog" / aria-modal="true" / aria-label', () => {
    const violations: string[] = [];
    let scanned = 0;
    for (const [path, src] of Object.entries(RAW_TSX)) {
      if (/\.test\.tsx$/.test(path)) continue;
      const re = /<div\b[^>]*className="[^"]*modal-bg[^"]*"[^>]*>/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(src))) {
        const tag = m[0];
        scanned += 1;
        const missing: string[] = [];
        if (!/role="dialog"/.test(tag)) missing.push('role');
        if (!/aria-modal="true"/.test(tag)) missing.push('aria-modal');
        if (!/aria-label=/.test(tag)) missing.push('aria-label');
        if (missing.length === 0) continue;
        const line = src.slice(0, m.index).split('\n').length;
        violations.push(`${path}:${line} 缺 ${missing.join('/')}`);
      }
    }
    expect(scanned).toBeGreaterThan(0); // 正则失配不能变成空转通过
    expect(violations).toEqual([]);
  });
});

// ── 弹窗的 Tab 循环真的挂上了（R347）────────────────────────────
// aria-modal 只是一句对读屏软件的承诺，浏览器不会替我们兑现。真正的 Tab 循环在
// apps/desktop/src/dialogFocus.ts 里，由 App 入口全局挂一次。这里守住「入口确实调用了它」：
// 被删掉时不会有任何测试变红，只会让键盘用户又开始 Tab 到背景页上去。
describe('弹窗焦点循环已挂载（R347）', () => {
  it('App 入口调用 useModalFocusTrap()', () => {
    const entry = Object.entries(RAW_TSX).find(([p]) => /(^|\/)App\.tsx$/.test(p));
    expect(entry).toBeTruthy();
    expect(entry?.[1]).toContain('useModalFocusTrap();');
  });
});

// ── 键盘焦点不许被 outline:none 抹掉（R348）────────────────────────
// 全仓出现过两类「焦点看不见」：`input, select { outline: none }` 这种全局重置没配
// :focus-visible 替身；单条 `<selector>:focus { outline: none; }` 既没给 border-color /
// box-shadow，也没补 :focus-visible。两者都让键盘用户看不出焦点在哪，且不会有断言变红。
const RAW_CSS = import.meta.glob('./**/*.css', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

describe('键盘焦点不许被 outline:none 抹掉（R348）', () => {
  it(':focus 规则里的 outline:none 必须有可见替身或 :focus-visible', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_CSS)) {
      const lines = src.split('\n');
      lines.forEach((line, i) => {
        if (!/:focus\b(?!-visible)/.test(line)) return;
        if (!/outline:\s*none/.test(line)) return;
        if (/border-color|box-shadow/.test(line)) return;
        const sel = line.slice(0, line.indexOf(':focus')).trim();
        if (lines.slice(i + 1).join('\n').includes(sel + ':focus-visible')) return;
        violations.push(`${path}:${i + 1} ${sel}:focus 抹掉了描边但没有可见替身`);
      });
    }
    expect(violations).toEqual([]);
  });

  it('全局 input/select 的 outline:none 必须配 :focus-visible 替身', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_CSS)) {
      if (!/input\s*,\s*select\s*\{[^}]*outline:\s*none/.test(src)) continue;
      if (/input:focus-visible/.test(src)) continue;
      violations.push(`${path} 全局 input/select 重置缺 :focus-visible 替身`);
    }
    expect(violations).toEqual([]);
  });
});

// ── Esc 只走一个栈，不许再手写 window keydown（R356）────────────────
// R349 把 Esc 收成一个栈：叠层时只有最上层响应。之后还散着几处自己挂
// window keydown 判 Escape 的实现（甜甜圈压测、坏点测试、创意工坊弹窗），
// 叠了确认框时一次 Esc 会把两层一起关掉。这里守住「不再长回来」。
// EXEMPT：SteamInspector 的三级 Esc（退出搜索框 → 关详情栏 → 交给外层 Modal）
// 必须比关窗先跑，属于有意手写；改动它要连 Modal 层的委派一起动。
const ESCAPE_EXEMPT = ['SteamInspector.tsx'];

describe('Esc 不许绕过 R349 的栈（R356）', () => {
  it('手写 window keydown 里不再出现 Escape 分支', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      if (!src.includes("addEventListener('keydown'")) continue;
      if (!/key\s*===\s*'Escape'|case 'Escape'/.test(src)) continue;
      if (ESCAPE_EXEMPT.some((name) => path.endsWith('/' + name))) continue;
      violations.push(`${path} 自己判 Escape——请改用 useEscapeKey（见 escapeKey.ts）`);
    }
    expect(violations).toEqual([]);
  });
});

// ── 减少动态效果（R357）──────────────────────────────────────────────
// 系统「减少动态效果」开着时，CSS 动画/过渡要停、JS 的平滑滚动要退回瞬间到位。
// 两者都不会让任何断言变红（只是晕动用户难受），所以这里守住口径。
describe('减少动态效果（R357）', () => {
  it('全局 CSS 有 prefers-reduced-motion 兜底块（停动画 / 停过渡 / 停平滑滚动）', () => {
    const cssAll = Object.values(RAW_CSS).join('\n');
    const block = cssAll.match(/@media\s*\(\s*prefers-reduced-motion:\s*reduce\s*\)\s*\{([\s\S]*?)\}/);
    expect(block).toBeTruthy();
    expect(block?.[1]).toMatch(/animation-duration/);
    expect(block?.[1]).toMatch(/transition-duration/);
    expect(block?.[1]).toMatch(/scroll-behavior/);
  });

  it('JS 不再裸写 behavior: smooth——一律经 scrollBehavior() 判断', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      if (!src.includes("behavior: 'smooth'")) continue;
      violations.push(`${path} 裸写 smooth——改 import { scrollBehavior } from '../motion'`);
    }
    expect(violations).toEqual([]);
  });
});

// ── 页面内提示文案也是 live region（R359）────────────────────────────
// toast 在 R319 补了 role="status"；但页面里还有第二条反馈通道——操作完就地
// 冒出的「已启用 / 已切换 / 已还原」文案，读屏软件同样一声不吭。
describe('页面内提示文案（R359）', () => {
  it('用 useFlashText 的组件必须有 role="status" 容器', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      if (src.includes('useFlashText(') && !src.includes('role="status"')) {
        violations.push(`${path} 用了 useFlashText 却没给提示容器 role="status"`);
      }
    }
    expect(violations).toEqual([]);
  });

  it('就地状态提示 {msg && <div|p ...>} 一律 role="status"', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/\{msg && <(div|p)\b([^>]*)>/g)) {
        if (!/role="status"/.test(m[2])) violations.push(`${path} 的 msg 容器缺 role="status"`);
      }
    }
    expect(violations).toEqual([]);
  });
});

// ── 外链交给系统浏览器（R360）──────────────────────────────────────
// Tauri/WebView2 里 `<a target="_blank">` 会被静默取消：wry 没注册 on_new_window
// 时直接 `args.SetHandled(true)`，既不建新窗也不交给系统浏览器——点了没反应。
// 外链一律 onClick preventDefault + openExternal（Rust 侧再走协议白名单）。
describe('外链交给系统浏览器（R360）', () => {
  it('不再用 target="_blank"', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      if (src.includes('target="_blank"')) {
        violations.push(`${path} 用了 target="_blank"——桌面端会被吞，改 onClick + openExternal`);
      }
    }
    expect(violations).toEqual([]);
  });

  it('每个 <a> 都拦下默认导航并走 openExternal', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<a\s[^>]*>/g)) {
        // 页内锚点（href="#..."，如 R363 的跳转链接）不触发默认导航，豁免
        if (/href="#/.test(m[0])) continue;
        if (!m[0].includes('onClick=')) violations.push(`${path} 的 <a> 没有 onClick`);
      }
    }
    expect(violations).toEqual([]);
  });
});
// ── 表单控件要有可访问名字（R361）─────────────────────────────────
// 读屏软件念「复选框，未选中」时用户不知道是哪一项；placeholder 不算可访问名字
// （它只是输入提示，且有些控件根本没有）。规则：<input> / <textarea> 必须带
// aria-label / aria-labelledby，或整块包在 <label> 里让 label 的文本当名字。
// 例外：type="file" 的隐藏输入（display:none，本来就不进无障碍树）。
describe('表单控件的可访问名字（R361）', () => {
  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let i = start; i < src.length; i++) {
      const c = src[i];
      if (quote) {
        if (c === '\\') i++;
        else if (c === quote) quote = '';
        continue;
      }
      if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return i;
    }
    return -1;
  };

  it('每个 input / textarea 都有名字或包在 label 里', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<(input|textarea)\b/g)) {
        const start = m.index ?? 0;
        const end = tagEnd(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        if (/type="file"/.test(tag)) continue;
        if (/aria-label=|aria-labelledby=/.test(tag)) continue;
        const before = src.slice(0, start);
        const depth = (before.match(/<label\b/g)?.length ?? 0) - (before.match(/<\/label>/g)?.length ?? 0);
        if (depth > 0) continue;
        violations.push(`${path}:${before.split('\n').length}`);
      }
    }
    expect(violations).toEqual([]);
  });
});
// ── 进度条要把进度报给读屏（R362）─────────────────────────────────
// 扫描 / 清理 / 压测 / 下载 / 更新的进度条若只是「彩色块 + 旁边文字」，
// 读屏软件不知道这是动态进度。规则：名字带 progress 或 -fill 且按百分比
// 撑宽度的填充条，必须有 role="progressbar"（带 aria-valuenow），或明确
// aria-hidden（纯装饰，如按钮里那段满格填充）。
describe('进度条要把进度报给读屏（R362）', () => {
  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let i = start; i < src.length; i++) {
      const c = src[i];
      if (quote) {
        if (c === '\\') i++;
        else if (c === quote) quote = '';
        continue;
      }
      if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return i;
    }
    return -1;
  };

  it('progress / 填充条都有 role=progressbar 或明确装饰豁免', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<(div|span)\b/g)) {
        const start = m.index ?? 0;
        const end = tagEnd(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        if (!/progress|\-fill/.test(tag)) continue;
        if (!/width:/.test(tag)) continue;
        if (/role="progressbar"/.test(tag) || /aria-hidden/.test(tag)) continue;
        violations.push(`${path}:${src.slice(0, start).split('\n').length}`);
      }
    }
    expect(violations).toEqual([]);
  });
});
// ── 页面主区能接住程序化焦点（R363）───────────────────────────────
// 切页后 App 会把焦点送给 <main>，读屏软件才知道「换页了、内容从这开始」。
// 前提是每个页面的 <main> 都能接收程序化焦点（tabIndex=-1）且带着跳转锚点
// 的 id；跳转链接本身也要在（Tab 第一击就到主内容）。
describe('页面主区能接住程序化焦点（R363）', () => {
  const RAW_CSS = import.meta.glob('./styles/app-shell.css', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let i = start; i < src.length; i++) {
      const c = src[i];
      if (quote) {
        if (c === '\\') i++;
        else if (c === quote) quote = '';
        continue;
      }
      if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return i;
    }
    return -1;
  };

  it('每个 <main> 都有锚点 id 且 tabIndex=-1', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<main\b/g)) {
        const start = m.index ?? 0;
        const end = tagEnd(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        if (!/id="main-content"/.test(tag) || !/tabIndex=\{-1\}/.test(tag)) {
          violations.push(`${path}:${src.slice(0, start).split('\n').length}`);
        }
      }
    }
    expect(violations).toEqual([]);
  });

  it('跳转链接指向主内容，且样式表里真有这个类', () => {
    const app = RAW_TSX['./App.tsx'] ?? '';
    expect(app).toContain('className="skip-link" href="#main-content"');
    const css = Object.values(RAW_CSS).join('\n');
    expect(css).toContain('.skip-link');
  });
});
describe('cycleIndex（R365）', () => {
  it('普通移动带环绕：右移到尾回到 0、左移到头回到末尾', () => {
    expect(cycleIndex(0, 4, 1)).toBe(1);
    expect(cycleIndex(3, 4, 1)).toBe(0);
    expect(cycleIndex(0, 4, -1)).toBe(3);
    expect(cycleIndex(2, 4, -1)).toBe(1);
  });

  it('current 越界/未选中时从最近的端点起算', () => {
    expect(cycleIndex(-1, 4, 1)).toBe(0);
    expect(cycleIndex(-1, 4, -1)).toBe(3);
    expect(cycleIndex(9, 4, 1)).toBe(0);
    expect(cycleIndex(9, 4, -1)).toBe(3);
  });

  it('空列表没有可落的位置（-1）', () => {
    expect(cycleIndex(0, 0, 1)).toBe(-1);
    expect(cycleIndex(-1, 0, -1)).toBe(-1);
  });
});

// R365：设置窗的 8 个分栏必须是真正的 tabs——读屏要能听到选中态、
// 方向键要能移动。守卫落在 Settings.tsx：tablist 存在、每个 tab 有 aria-selected，
// 且每一处 settings-pane 内容面板都带着自己的 tabpanel 身份。
describe('设置窗分栏是真正的 tabs（R365）', () => {
  it('Settings.tsx 有 tablist，tab 生成带 aria-selected 与 roving tabindex', () => {
    const app = RAW_TSX['./components/Settings.tsx'] ?? '';
    expect(app).toContain('role="tablist"');
    expect(app).toContain('aria-selected={on}');
    expect(app).toContain('tabIndex={on ? 0 : -1}');
    expect(app).toContain('onTabKey(e, idx)');
    expect(app).toContain('id={`settings-tab-${tb.id}`}');
  });

  it('每处 settings-pane 内容面板都有 tabpanel 身份与自己的 aria-labelledby', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<div\b([^>]*className="settings-pane[^"]*"[^>]*)>/g)) {
        const tag = m[0];
        if (!/role="tabpanel"/.test(tag) || !/aria-labelledby="settings-tab-/.test(tag)) {
          violations.push(`${path}:${src.slice(0, m.index ?? 0).split('\n').length}`);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});
// R366：16 处 seg-opt 分段单选按钮（语言/主题/字号/间隔/阈值/传输方式等）
// 此前只有 active 类——读屏听不出「当前选的是哪一个」。每个按钮补 aria-pressed
//（按下=当前项），守卫确保以后新增的分段按钮也带上。
describe('分段单选控件带 aria-pressed（R366）', () => {
  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let j = start; j < src.length; j++) {
      const c = src[j];
      if (quote) {
        if (c === '\\') j++;
        else if (c === quote) quote = '';
        continue;
      }
      if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return j;
    }
    return -1;
  };

  it('每个 seg-opt 按钮都要报按下状态', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<button\b/g)) {
        const start = m.index ?? 0;
        const end = tagEnd(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        if (!/seg-opt/.test(tag)) continue;
        if (!/aria-pressed=/.test(tag)) {
          violations.push(path + ':' + src.slice(0, start).split('\n').length);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});
// R367：展开/收起按钮（带 chevron 图标的切换控件）此前只有图标方向变化，
// 读屏听不出「现在展开还是收起」——树行、清理预览、工具用法、社区仓库、
// 工具箱卡片、市场进阶区/已安装台账都要报 aria-expanded。
describe('展开/收起控件报展开状态（R367）', () => {
  const RAW_TSX = import.meta.glob('./**/*.tsx', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
  it('带 chevron 的按钮必须报 aria-expanded', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (let i = 0; i < src.length; ) {
        const j = src.indexOf('<button', i);
        if (j < 0) break;
        i = j + 7;
        const nxt = src[j + 7] ?? ' ';
        if (/[\w-]/.test(nxt)) continue;
        let brace = 0;
        let quote = '';
        let end = -1;
        for (let k = j; k < src.length; k++) {
          const c = src[k];
          if (quote) { if (c === '\\') k++; else if (c === quote) quote = ''; continue; }
          if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
          if (c === '{') brace++;
          else if (c === '}') brace--;
          else if (c === '>' && brace <= 0) { end = k; break; }
        }
        if (end < 0) continue;
        const closeIdx = src.indexOf('</button>', end);
        if (closeIdx < 0) continue;
        const openTag = src.slice(j, end + 1);
        const children = src.slice(end + 1, closeIdx);
        if (!/Chevron(Down|Up|Right|Left)/.test(children)) continue;
        if (!/aria-expanded=/.test(openTag)) {
          violations.push(path + ':' + src.slice(0, j).split('\n').length);
        }
      }
    }
    expect(violations).toEqual([]);
  });
  it('树行目录节点带展开状态', () => {
    const tree = RAW_TSX['./components/TreeView.tsx'] ?? '';
    expect(tree).toContain('aria-expanded={node.is_dir ? open : undefined}');
  });
});
// R368：右键菜单此前只能鼠标唤出——文件树行 / 文件列表行的「打开位置、复制路径、
// 移入回收站」键盘用户完全够不到。菜单键 / Shift+F10 是 Windows 通行入口。
describe('右键菜单的键盘入口（R368）', () => {
  it('isContextMenuKey 认菜单键与 Shift+F10', () => {
    expect(isContextMenuKey({ key: 'ContextMenu' })).toBe(true);
    expect(isContextMenuKey({ key: 'F10', shiftKey: true })).toBe(true);
    expect(isContextMenuKey({ key: 'F10' })).toBe(false);
    expect(isContextMenuKey({ key: 'Enter' })).toBe(false);
    expect(isContextMenuKey({ key: 'F10', shiftKey: false })).toBe(false);
  });
  it('contextMenuAnchor 给出行内锚点', () => {
    expect(contextMenuAnchor(null)).toBeNull();
    const el = { getBoundingClientRect: () => ({ left: 100, top: 40, width: 0, height: 0 }) } as unknown as Element;
    expect(contextMenuAnchor(el)).toBeNull();
    const el2 = { getBoundingClientRect: () => ({ left: 100, top: 40, width: 600, height: 24 }) } as unknown as Element;
    expect(contextMenuAnchor(el2)).toEqual({ x: 112, y: 62 });
  });
  it('用了 onContextMenu 的组件都要有键盘等价入口', () => {
    const EXEMPT = new Set(['./components/ContextMenu.tsx']);
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      if (EXEMPT.has(path)) continue;
      if (/onContextMenu/.test(src) && !/isContextMenuKey/.test(src)) {
        violations.push(path);
      }
    }
    expect(violations).toEqual([]);
  });
});
// R369：除 seg-opt 外还有 32 个按钮用「active 类」表达选中态——筛选芯片、排序列头、
// 主题色板、服务页签、工具分类、模态页签等。active 只是视觉，读屏听不出选中的一项；
// 全部补 aria-pressed（按下=当前项），守卫禁止未来再出现裸 active 类按钮。
describe('active 类按钮都要报选中态（R369）', () => {
  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let j = start; j < src.length; j++) {
      const c = src[j];
      if (quote) { if (c === '\\') j++; else if (c === quote) quote = ''; continue; }
      if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return j;
    }
    return -1;
  };
  it('className 含 active 的按钮必须带 aria 状态', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<button\b/g)) {
        const start = m.index ?? 0;
        const end = tagEnd(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        if (!/' active'/.test(tag)) continue;
        if (!/aria-pressed=|aria-selected=|aria-current=|aria-checked=/.test(tag)) {
          violations.push(path + ':' + src.slice(0, start).split('\n').length);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});
// R370：页面里有 5 个 <aside>（左右侧栏、清理脚本列表、Steam 筛选/详情）都没有名字——
// 读屏软件只能念「补充地标」，叠在一起就分不清哪个是哪个。nav 早就带了 aria-label，
// 守卫要求每个 aside / nav 都必须有 aria-label 或 aria-labelledby。
describe('地标要有名字（R370）', () => {
  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let j = start; j < src.length; j++) {
      const c = src[j];
      if (quote) { if (c === '\\') j++; else if (c === quote) quote = ''; continue; }
      if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return j;
    }
    return -1;
  };
  it('每个 aside / nav 都带 aria-label', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<(aside|nav)\b/g)) {
        const start = m.index ?? 0;
        const end = tagEnd(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        if (!/aria-label=|aria-labelledby=/.test(tag)) {
          violations.push(path + ':' + src.slice(0, start).split('\n').length + ' <' + m[1] + '>');
        }
      }
    }
    expect(violations).toEqual([]);
  });
});

// R371：分隔条原本只有鼠标拖动（onMouseDown）与双击复位，键盘用户没法调面板宽窄。
// 补 role=separator + tabIndex + 方向键（splitterDelta）后，才是一个键盘可用的分隔条控件。
describe('分隔条要能键盘调宽（R371）', () => {
  it('Splitter 是可聚焦的 separator，方向键交给 splitterDelta', () => {
    const src = RAW_TSX['./components/Splitter.tsx'] ?? '';
    expect(src).toContain('role="separator"');
    expect(src).toContain('aria-orientation="vertical"');
    expect(src).toContain('tabIndex={0}');
    expect(src).toContain('splitterDelta');
  });

  it('分隔条有键盘聚焦样式（outline 不许被吞掉）', () => {
    const cssAll = Object.values(RAW_CSS).join('\n');
    expect(cssAll).toMatch(/\.splitter:focus-visible\s*\{/);
  });
});

// R372：SVG 图（环形图/趋势线/树图）对读屏要么是自描述的图（role=img + aria-label），
// 要么是旁边已有等价文字的纯装饰（aria-hidden）——不能什么都不说地挂在页面上。
describe('SVG 图要么有名字要么是装饰（R372）', () => {
  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let i = start; i < src.length; i++) {
      const c = src[i];
      if (quote) {
        if (c === '\\') i++;
        else if (c === quote) quote = '';
        continue;
      }
      if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return i;
    }
    return -1;
  };
  it('每个 <svg> 都有 aria-hidden 或 (role + aria-label)', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<svg\b/g)) {
        const start = m.index ?? 0;
        const end = tagEnd(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        const ok = /aria-hidden/.test(tag) || (/role=/.test(tag) && /aria-label=/.test(tag));
        if (!ok) violations.push(path + ':' + src.slice(0, start).split('\n').length);
      }
    }
    expect(violations).toEqual([]);
  });
});
// R373：排序列头的方向（升/降）只有箭头图标可见，读屏只能听出 aria-pressed。
// 当前列必须经 sortHeaderLabel 补 aria-label，把「列名，升/降序」念出来。
describe('排序列头报出升降序（R373）', () => {
  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let j = start; j < src.length; j++) {
      const c = src[j];
      if (quote) {
        if (c === '\\') j++;
        else if (c === quote) quote = '';
        continue;
      }
      if (c === '"' || c === "'" || c === '`') {
        quote = c;
        continue;
      }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return j;
    }
    return -1;
  };
  it('sortable 按钮带 aria-pressed 时必有来自 sortHeaderLabel 的 aria-label', () => {
    const violations: string[] = [];
    const src = RAW_TSX['./components/FileView.tsx'] ?? '';
    expect(src).toContain('sortHeaderLabel(');
    for (const m of src.matchAll(/<button\b/g)) {
      const start = m.index ?? 0;
      const end = tagEnd(src, start);
      if (end < 0) continue;
      const tag = src.slice(start, end + 1);
      if (!/'sortable'/.test(tag)) continue;
      if (!/aria-pressed=/.test(tag)) continue;
      if (!/aria-label=\{sortHeaderLabel\(/.test(tag)) {
        violations.push(String(src.slice(0, start).split('\n').length));
      }
    }
    expect(violations).toEqual([]);
  });
});
// R384：错误横幅此前只是红色小字，读屏软件不会主动念——扫描失败、台账加载失败、
// 连接测试失败、安装失败都静默。错误样式的文本容器必须带 role="alert"（静态说明
// 用 role="note" 区分，纯装饰用 aria-hidden）。这条守卫兜住将来新加的错误行。
describe('错误横幅要能被读屏念出来（R384）', () => {
  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let j = start; j < src.length; j++) {
      const c = src[j];
      if (quote) {
        if (c === '\\') j++;
        else if (c === quote) quote = '';
        continue;
      }
      if (c === '"' || c === "\'" || c === '`') {
        quote = c;
        continue;
      }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return j;
    }
    return -1;
  };
  it("错误样式的 div/p/span 必须带 role 或 aria-hidden", () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<(div|p|span)\b/g)) {
        const start = m.index ?? 0;
        const end = tagEnd(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        const cm = tag.match(/className="([^"]*)"/);
        if (!cm) continue;
        const isErr = cm[1].split(/\s+/).some((tok) => tok === 'err' || tok === 'error' || tok.endsWith('-err'));
        if (!isErr) continue;
        const ok = /role="alert"/.test(tag) || /role="note"/.test(tag) || /aria-hidden/.test(tag);
        if (!ok) violations.push(path + ':' + src.slice(0, start).split('\n').length);
      }
    }
    expect(violations).toEqual([]);
  });
})

// ── 扫描错误横幅要能手动关闭（R389）──────────────────────────────
describe('错误横幅要能手动关闭（R389）', () => {
  it('App.tsx 的 banner error 里有关闭按钮（点了直接清空 err）', () => {
    const appSrc = RAW_TSX['./App.tsx'] ?? '';
    const at = appSrc.indexOf("className=\"banner error\"");
    expect(at).toBeGreaterThan(-1);
    const block2 = appSrc.slice(at, at + 900);
    expect(block2).toContain('setErr(null)');
    expect(block2).toContain('banner-dismiss');
    expect(block2).toContain('aria-label={t(');
  });
  it('关闭按钮有可见的键盘焦点样式（layout.css）', () => {
    const css = Object.values(RAW_CSS).join('\n');
    expect(css).toMatch(/\.banner-dismiss:focus-visible\s*\{/);
  });
});
// ── 字段校验错误要报给读屏（R391）────────────────────────────────
describe('字段校验错误要报给读屏（R391）', () => {
  // 与 R361 守卫同款标签截取：花括号/引号跟踪，字段值里的 > 不会把标签截断。
  const tagEndOf = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let i = start; i < src.length; i++) {
      const c = src[i];
      if (quote) {
        if (c === '\\') i++;
        else if (c === quote) quote = '';
        continue;
      }
      if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return i;
    }
    return -1;
  };

  it('带 aria-invalid 的输入框必须同时带 aria-describedby（不能只标错不给去向）', () => {
    const violations: string[] = [];
    for (const [rel, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<(input|textarea)\b/g)) {
        const start = m.index ?? 0;
        const end = tagEndOf(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        if (!/aria-invalid/.test(tag)) continue;
        if (!/aria-describedby/.test(tag)) violations.push(rel + ':' + src.slice(0, start).split('\n').length);
      }
    }
    expect(violations).toEqual([]);
  });

  it('aria-describedby 指向的字面 id 必须在同文件里存在（不许指向虚空）', () => {
    const violations: string[] = [];
    for (const [rel, src] of Object.entries(RAW_TSX)) {
      const ids = new Set([...src.matchAll(/id="([\w-]+)"/g)].map((x) => x[1]));
      for (const m of src.matchAll(/aria-describedby=["']([\w-]+)["']/g)) {
        if (!ids.has(m[1])) violations.push(rel + ':' + m[1]);
      }
    }
    expect(violations).toEqual([]);
  });
});

describe('右键菜单触发器要声明会开菜单（R396）', () => {
  // ContextMenu 自己是菜单本体，它的 onContextMenu 只是吞掉浏览器原生菜单。
  const EXEMPT = ['ContextMenu.tsx'];
  const tagEndOf = (s: string, start: number): number => {
    let depth = 0;
    let quote = '';
    for (let i = start; i < s.length; i++) {
      const c = s[i];
      if (quote) { if (c === quote) quote = ''; continue; }
      if (c === '"' || c === "'") { quote = c; continue; }
      if (c === '{') depth++;
      else if (c === '}') depth--;
      else if (c === '>' && depth <= 0) return i;
    }
    return -1;
  };

  it('带 onContextMenu 的元素必须声明 aria-haspopup="menu"', () => {
    const violations: string[] = [];
    for (const [rel, src] of Object.entries(RAW_TSX)) {
      if (EXEMPT.some((x) => rel.endsWith(x))) continue;
      for (const m of src.matchAll(/onContextMenu=/g)) {
        const idx = m.index ?? 0;
        const start = src.lastIndexOf('<', idx);
        if (start < 0) continue;
        const end = tagEndOf(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        if (!/aria-haspopup="menu"/.test(tag)) {
          violations.push(rel + ':' + src.slice(0, idx).split('\n').length);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});

describe('聊天记录是动态内容，要被读屏播报（R399）', () => {
  it('ChatPanel 的对话区必须有 role=log，输入中提示有 role=status', () => {
    // 先剥掉 JSX 注释再断言——注释里引用的字面量不能算数（自检时就栽过一次）。
    const chat = (RAW_TSX['./components/ChatPanel.tsx'] ?? '')
      .replace(/\{\s*\/\*[\s\S]*?\*\/\s*\}/g, '')
      .replace(/<!--[\s\S]*?-->/g, '');
    expect(chat).toContain('<div className="chat-scroll" role="log"');
    expect(chat).toContain('className="chat-typing" role="status"');
    // R401：流式更新期间要声明 aria-busy，读屏不会急着把拼到一半的内容念完。
    expect(chat).toContain('aria-busy={chatBusy || undefined}');
  });
});

// ── 输入法组合期间的 Enter 不许触发发送（R404）──────────────────
// 拼音/日文/韩文 IME 选候选词时按 Enter 是「确认候选字」，不是「发送」；
// 没有组合状态位的话，中文用户打回车会把打一半的句子直接发出去。
describe('输入法组合期间的 Enter 不许发送（R404）', () => {
  it('聊天输入框带组合状态位 + isComposing 双保险', () => {
    const src = RAW_TSX['./components/ChatPanel.tsx'];
    expect(src).toBeTruthy();
    expect(src).toContain('onCompositionStart');
    expect(src).toContain('onCompositionEnd');
    expect(src).toContain('e.nativeEvent.isComposing');
    expect(src).toContain('composingRef.current || e.nativeEvent.isComposing');
  });
});


// ── 聊天区贴底跟随，不许退回无条件滚动（R406）────────────────────────
// ChatPanel 曾在 turns 变化时无条件 scrollTo(scrollHeight)：用户往上翻历史会被
// 新消息拽回底部。现在 onScroll 维护贴底位、滚动 effect 先看它——删掉任何一半
// 都会让「翻历史」重新变成不可能，所以断言两半都在。
describe('聊天区贴底跟随（R406）', () => {
  it('onScroll 维护贴底位 + 滚动 effect 先看贴底位', () => {
    const entry = Object.entries(RAW_TSX).find(([p]) => p.endsWith('components/ChatPanel.tsx'));
    expect(entry).toBeTruthy();
    const src = entry?.[1] ?? '';
    expect(src).toContain('onScroll=');
    expect(src).toContain('stickNearBottom(e.currentTarget)');
    expect(src).toContain('if (!stickRef.current) return;');
    expect(src).toContain('import { stickNearBottom }');
  });

  it('滚动 effect 不是无条件滚动（stickRef 守卫在 scrollTo 之前）', () => {
    const entry = Object.entries(RAW_TSX).find(([p]) => p.endsWith('components/ChatPanel.tsx'));
    const src = entry?.[1] ?? '';
    const guard = src.indexOf('if (!stickRef.current) return;');
    const scroll = src.indexOf('scrollerRef.current?.scrollTo');
    expect(guard).toBeGreaterThan(0);
    expect(scroll).toBeGreaterThan(guard);
  });
});

// ── 工具墙的确认弹层都支持 Esc 取消（R414）────────────────────────
// R142 只给启动确认 / exe 详情 / AI 工具详情三处接了 useEscapeKey；后来加的
// 「导出确认 / 回收确认」没有——同一个界面上有的弹窗按 Esc 能关、有的不能，
// 用户只会觉得键盘失灵（而且它们背后就是导出 / 删目录这类危险动作）。
describe('工具墙弹层 Esc 取消（R414）', () => {
  const entry = Object.entries(RAW_TSX).find(([p]) => p.endsWith('components/toolbelt/toolwall.tsx'));
  const src = entry?.[1] ?? '';
  const escBlock = (() => {
    const start = src.indexOf('useEscapeKey(');
    const end = src.lastIndexOf('useEscapeKey(');
    return start >= 0 && end > start ? src.slice(start, end + 220) : '';
  })();

  it('五个弹层状态都能被 Esc 取消', () => {
    expect(escBlock).toBeTruthy();
    for (const setter of [
      'setConfirmLaunch(null)',
      'setDetail(null)',
      'setAgentDetail(null)',
      'setExportAsk(null)',
      'setRecycleAsk(false)',
    ]) {
      expect(escBlock).toContain(setter);
    }
  });

  it('导出 / 回收进行中不响应 Esc（与启动确认同一口径）', () => {
    expect(src).toContain('useEscapeKey(() => setExportAsk(null), exportAsk !== null && !exporting)');
    expect(src).toContain('useEscapeKey(() => setRecycleAsk(false), recycleAsk && !recycling)');
  });
});
// R416：跑分历史是「最新插最前」的动态列表——key={i} 会让新记录跑完后所有条目
// 错位一格，React 按 index 复用 <details> DOM，用户展开的历史漂到别的记录上。
describe('跑分历史的条目身份（R416）', () => {
  const RAW = import.meta.glob('./components/toolbelt/bench.tsx', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
  it('两处 details 列表都用稳定身份 key，不用 index', () => {
    const src = RAW['./components/toolbelt/bench.tsx'] ?? '';
    const detailsKeys = src.match(/<details key=\{[^}]+\}>/g) ?? [];
    expect(detailsKeys).toHaveLength(2);
    expect(detailsKeys.every((t) => t === '<details key={h.at}>')).toBe(true);
    expect(src).not.toMatch(/<details key=\{i\}>/);
    expect(src).not.toMatch(/reverse\(\)\.slice\(1\)\.map\(\(h, i\) =>/);
  });
});
// R417：4 处「仅挂载时拉取」effect 补 alive 卸载守卫——视图切换 / 弹层快速关闭时
// 迟到的响应不再 setState 到已卸载组件（对齐 R382 / CleanupPage / Studio 的房规）。
describe('挂载期拉取的卸载守卫（R417）', () => {
  const R417_SITES: Record<string, string> = {
    './components/AssetOverview.tsx': 'api.getSpaceHistory()',
    './components/Settings.tsx': 'getHwProfile()',
    './components/toolbelt/toolwall.tsx': 'api.toolbeltManifests()',
    './components/Toolbelt.tsx': 'api.toolbeltCatalog()',
  };
  function effectBlock(src: string, marker: string): string {
    const at = src.indexOf(marker);
    expect(at).toBeGreaterThan(-1);
    const start = src.lastIndexOf('useEffect(', at);
    const end = src.indexOf('}, []);', at);
    expect(start).toBeGreaterThan(-1);
    expect(end).toBeGreaterThan(start);
    return src.slice(start, end);
  }
  it('四处只读拉取（含 toolwall 两处）、一处弹层档位：脱挂后响应不落地', () => {
    for (const [path, marker] of Object.entries(R417_SITES)) {
      const block = effectBlock(RAW_TSX[path] ?? '', marker);
      expect(block).toContain('let alive = true;');
      expect(block).toContain('return () => { alive = false; };');
      expect(block).toMatch(/if \(alive\)|if \(!alive\)/);
    }
  });
  it('设置页性能档位与工具墙目录：先查 alive 再落地', () => {
    const settings = effectBlock(RAW_TSX['./components/Settings.tsx'] ?? '', 'getHwProfile()');
    expect(settings).toContain('if (alive) setHw(p)');
    expect(settings).toContain('if (alive) setHw(null)');
    const belt = effectBlock(RAW_TSX['./components/Toolbelt.tsx'] ?? '', 'api.toolbeltCatalog()');
    expect(belt).toContain('if (alive) setCatalog(c)');
  });
  it('工具墙清单与探活（含 catch 里嵌套 agentReady）都受 alive 保护', () => {
    const tw = RAW_TSX['./components/toolbelt/toolwall.tsx'] ?? '';
    const manifests = effectBlock(tw, 'api.toolbeltManifests()');
    expect(manifests).toContain('if (!alive) return; setManifests(list)');
    const listTools = effectBlock(tw, 'api.listTools()');
    expect(listTools).toContain('if (!alive) return;');
    expect(listTools).toContain("if (alive) setAgentIssue(ready ? 'failed' : 'missing')");
  });
});
// R418：保留天数输入框的钳制回显——输入上界与 sanitize 钳制是同一个常量；
// 提交走「草稿 + 失焦/Enter」而不是每敲一个字符就 Number() 写回。
describe('保留天数输入框的钳制回显（R418）', () => {
  const RAW_MODEL = import.meta.glob('./cleanup/model.ts', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
  const RAW_CB = import.meta.glob('./components/cleanup/CleanupBody.tsx', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
  it('上限常量导出，sanitize 钳制与输入 max 用同一个数', () => {
    const model = RAW_MODEL['./cleanup/model.ts'] ?? '';
    expect(model).toContain('export const MAX_RETENTION_DAYS = 3650;');
    expect(model).toContain('if (raw > MAX_RETENTION_DAYS) return MAX_RETENTION_DAYS;');
    expect(model).not.toMatch(/raw > 3650(?!\b)/);
    const cb = RAW_CB['./components/cleanup/CleanupBody.tsx'] ?? '';
    expect(cb).toContain('max={MAX_RETENTION_DAYS}');
  });
  it('提交走草稿 + 失焦/Enter，清空回退默认天数而非 0', () => {
    const cb = RAW_CB['./components/cleanup/CleanupBody.tsx'] ?? '';
    const at = cb.indexOf('cleanup.keepRecent');
    const end = cb.indexOf('cleanup.daysUnit', at);
    const region = cb.slice(at, end);
    expect(region).toContain('value={daysDraft ?? String(days ?? 0)}');
    expect(region).toContain('onBlur');
    expect(region).toContain("if (e.key === 'Enter') e.currentTarget.blur();");
    expect(region).toContain('raw === \'\' ? fallback');
    expect(region).toContain('sanitizeRetentionDays(parsed)');
    expect(region).not.toContain('setDays(scope.id, Number(');
    expect(region).toContain('setDaysDraft(null)');
  });
});
// R419：跳过链接（skip-link → #main-content）的落点必须看得见焦点——
// main 的 :focus-visible 若把描边抹掉，键盘用户回车跳过去就丢了位置。
describe('跳过链接落点的焦点可见（R419）', () => {
  const RAW_SHELL = import.meta.glob('./styles/app-shell.css', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
  it('main:focus-visible 给可见描边而不是 outline: none', () => {
    const css = RAW_SHELL['./styles/app-shell.css'] ?? '';
    const at = css.indexOf('main:focus-visible');
    expect(at).toBeGreaterThan(-1);
    const end = css.indexOf('}', at);
    const block = css.slice(at, end);
    expect(block).not.toContain('outline: none');
    expect(block).toContain('outline: 2px solid var(--accent-strong)');
    expect(block).toContain('outline-offset: -2px');
  });
  it('落点锚（#main-content / skip-link）仍存在，否则焦点替身是空转', () => {
    const css = RAW_SHELL['./styles/app-shell.css'] ?? '';
    expect(css).toContain('.skip-link:focus-visible');
    const app = RAW_TSX['./App.tsx'] ?? '';
    expect(app).toContain('href="#main-content"');
    expect(app).toContain('id="main-content"');
    expect(app).toContain('tabIndex={-1}');
  });
});

// R421：压测 / 体检报告是 5–300s 的长 call，而 BenchPanel 挂在 Toolbelt 的 view 条件
// 渲染下——切走 tab 直接卸载。此前 poll() 仍每 2s 打一次后端 sensor_trend，run() 的
// await 返回后还往已卸载组件落地。这里锁住「卸载即停轮询、落地先查 alive」的形状。
describe('长任务的卸载守卫（R421）', () => {
  const bench = RAW_TSX['./components/toolbelt/bench.tsx'] ?? '';
  it('压测面板有 aliveRef：setup 置真、cleanup 置假并掐掉实时温度轮询', () => {
    expect(bench).toContain('const aliveRef = useRef(true);');
    expect(bench).toContain('aliveRef.current = true;');
    expect(bench).toContain('aliveRef.current = false;');
    expect(bench).toContain('window.clearInterval(liveTimer.current);');
  });
  it('实时温度轮询：发请求前与响应落地前都查 alive', () => {
    expect(bench).toContain('if (!aliveRef.current) return; // R421：卸载后不再打后端');
    expect(bench).toContain('if (!aliveRef.current) return; // R421：响应回来时已卸载，不落地');
    expect(bench).toContain('if (aliveRef.current) setLiveTemp(');
  });
  it('长 call 前后的 setState 都受 alive 保护（busy / 结果 / 报告）', () => {
    const lines = bench.split('\n');
    const busy = lines.filter((l) => l.includes('setBusy(null)'));
    expect(busy.length).toBeGreaterThan(0);
    for (const line of busy) expect(line).toContain('aliveRef.current');
    const results = lines.filter((l) => l.includes('setResults((prev) => ({ ...prev, [key]:'));
    expect(results.length).toBe(2);
    for (const line of results) expect(line).toContain('aliveRef.current');
    for (const line of lines.filter((l) => l.includes("setReport(r.is_error") || l.includes("setReport(t('toolbelt.bench.report.failed'"))) {
      expect(line).toContain('aliveRef.current');
    }
    expect(bench).toContain('if (!aliveRef.current) return; // R421：卸载后实时温度与曲线不再落地');
  });
});
// R424：内联弹窗（工具箱详情/导出/回收/启动确认、跑分三确认、插件市场写操作、Steam 详情/创意工坊）
// 打开时焦点进场（第一个可聚焦元素）、关闭时归还 opener——键盘用户打开弹窗的瞬间焦点就该在弹窗里。
// dialogFocus.ts 不是 .tsx，RAW_TSX 摸不到它，单独 glob。
const RAW_DF = import.meta.glob('./dialogFocus.ts', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

describe('弹窗焦点进场与归还（R424）', () => {
  it('dialogFocus.ts 提供 useDialogFocus：进场取最上层弹窗第一个可聚焦元素、关闭归还 opener', () => {
    const df = RAW_DF['./dialogFocus.ts'] ?? '';
    expect(df).toContain('export function useDialogFocus(');
    expect(df).toContain('?? list[0] ??');
    expect(df).toContain('prev.current?.focus?.();');
    expect(df).toContain('document.querySelectorAll<HTMLElement>(MODAL_SEL)');
  });
  it('工具箱 ToolDetailModal：详情/导出/回收三层各自挂进场归还', () => {
    const tw = RAW_TSX['./components/toolbelt/toolwall.tsx'] ?? '';
    expect(tw).toContain('useDialogFocus(true)');
    expect(tw).toContain('useDialogFocus(exportAsk !== null)');
    expect(tw).toContain('useDialogFocus(recycleAsk !== null)');
  });
  it('工具箱 CatalogWall：启动确认 / AI 工具详情两层各自挂进场归还', () => {
    const tw = RAW_TSX['./components/toolbelt/toolwall.tsx'] ?? '';
    expect(tw).toContain('useDialogFocus(confirmLaunch !== null)');
    expect(tw).toContain('useDialogFocus(agentDetail !== null)');
  });
  it('跑分三个确认窗挂进场归还（压测 / 磁盘 / 甜甜圈）', () => {
    const b = RAW_TSX['./components/toolbelt/bench.tsx'] ?? '';
    expect(b).toContain('useDialogFocus(confirmStress)');
    expect(b).toContain('useDialogFocus(confirmDiskRun)');
    expect(b).toContain('useDialogFocus(confirmDonut)');
  });
  it('插件市场写操作确认挂进场归还', () => {
    const mk = RAW_TSX['./components/toolbelt/market.tsx'] ?? '';
    expect(mk).toContain('useDialogFocus(confirmAsk !== null)');
  });
  it('Steam 详情 / 创意工坊弹窗（条件挂载）打开即进场', () => {
    expect(RAW_TSX['./components/SteamInspectorModal.tsx'] ?? '').toContain('useDialogFocus(true)');
    expect(RAW_TSX['./components/SteamWorkshopModal.tsx'] ?? '').toContain('useDialogFocus(true)');
  });
});

// R425：挂载式弹窗（预览窗 / 提案窗 / 安装窗）此前只有 close 后归还（useReturnFocus），
// 打开瞬间焦点还留在触发元素上——换成 useDialogFocus 后进场 + 归还一体。
describe('挂载式弹窗的进场焦点（R425）', () => {
  it('预览窗（DryRunPreviewDialog）打开即进场，关掉回到清理按钮', () => {
    const c = RAW_TSX['./components/cleanup/DryRunPreviewDialog.tsx'] ?? '';
    expect(c).toContain('useDialogFocus(true)');
    expect(c).not.toContain('useReturnFocus');
  });
  it('AI 提案窗（CleanupProposalDialog，常驻挂载）提案一出现即进场，关闭归还', () => {
    const c = RAW_TSX['./components/CleanupProposalDialog.tsx'] ?? '';
    expect(c).toContain('useDialogFocus(proposal !== null)');
    expect(c).not.toContain('useReturnFocus');
  });
  it('工具坊安装窗（InstallScaffoldModal）打开即进场，装完/关闭归还', () => {
    const c = RAW_TSX['./components/Studio.tsx'] ?? '';
    expect(c).toContain('useDialogFocus(true)');
    expect(c).not.toContain('useReturnFocus');
  });
  it('已有初始焦点的挂载式弹窗保留 useReturnFocus（口径不变）', () => {
    expect(RAW_TSX['./components/ConfirmDialog.tsx'] ?? '').toContain('useReturnFocus(true)');
    expect(RAW_TSX['./components/ContextMenu.tsx'] ?? '').toContain('useReturnFocus(state !== null, { lockScroll: false })');
    expect(RAW_TSX['./components/HwPanels.tsx'] ?? '').toContain('useReturnFocus(true)');
    expect(RAW_TSX['./components/Settings.tsx'] ?? '').toContain('useReturnFocus(true)');
  });
});

// R426：进场焦点不能落在头部关闭按钮上——打开弹窗按一下 Enter/空格就把弹窗关掉是破坏性默认。
// useDialogFocus 应跳过 .modal-head / .steam-modal-head 里的关闭钮，优先头部之外的可聚焦元素。
describe('弹窗初始焦点避开关闭按钮（R426）', () => {
  it('useDialogFocus 用头部选择器过滤关闭钮，保留头部外第一个可聚焦元素', () => {
    const df = RAW_DF['./dialogFocus.ts'] ?? '';
    expect(df).toContain('.modal-head, .steam-modal-head');
    expect(df).toContain('head ? list.filter((el) => !head.contains(el)) : list');
    expect(df).toContain('list.filter((el) => !head.contains(el))');
  });
});


// R427：视觉提示不等于读屏名——select 必须有 aria-label / aria-labelledby / title，或包在 <label> 里。
describe('下拉框的可访问名（R427）', () => {
  it('提醒设置的两个下拉各有可访问名', () => {
    const rs = RAW_TSX['./components/ReminderSettings.tsx'] ?? '';
    expect(rs).toContain("aria-label={t('system.reminder.autoListPick')}");
    expect(rs).toContain('<span id="reminder-threshold-hint"');
    expect(rs).toContain('aria-labelledby="reminder-threshold-hint"');
  });

  it('全仓没有裸的 select（无名且不在 label 内）', () => {
    const offenders: string[] = [];
    for (const [key, raw] of Object.entries(RAW_TSX)) {
      const lines = String(raw).split(String.fromCharCode(10));
      for (let i = 0; i < lines.length; i++) { {
        if (!/<select(\s|>|$)/.test(lines[i])) continue;
        let body = '';
        for (let j = i; j < Math.min(lines.length, i + 12); j++) {
          body += lines[j] + String.fromCharCode(10);
          if (/[^=<>]>/.test(lines[j]) || /^\s*>\s*$/.test(lines[j])) break;
        }
        if (/aria-label=|aria-labelledby=|title=/.test(body)) continue;
        const prev = lines.slice(Math.max(0, i - 12), i).join(String.fromCharCode(10));
        if (/<label[\s>]/.test(prev)) continue;
        offenders.push(key + ':' + (i + 1));
      } }
    }
    expect(offenders, 'select 缺可访问名（补 aria-label / aria-labelledby / title，或包进 label）').toEqual([]);
  });
});

// R428：role=dialog / role=alertdialog 必须有可访问名（aria-label 或 aria-labelledby）——
// 读屏进入弹窗先念名字；缺名的弹窗像「没有标题的页面」。
// 用与 R427 select 扫描器同款的「逐行找开标签→抽到闭合」逻辑，避免属性跨行漏检。
describe('弹窗的可访问名（R428）', () => {
  it('Steam 创意工坊弹窗有可访问名', () => {
    const m = RAW_TSX['./components/SteamWorkshopModal.tsx'] ?? '';
    expect(m).toContain('aria-labelledby="steam-workshop-title"');
    expect(m).toContain('id="steam-workshop-title"');
  });

  it('全仓所有 role=dialog / alertdialog 都有可访问名', () => {
    const offenders: string[] = [];
    for (const [key, raw0] of Object.entries(RAW_TSX)) {
      const lines = String(raw0).split(String.fromCharCode(10));
      for (let i = 0; i < lines.length; i++) {
        if (!/role="?(dialog|alertdialog)"?/.test(lines[i])) continue;
        // 只扫 JSX 元素行：role= 左侧必须已有 `<`（注释里的 role=dialog 说明文字不算）
        if (lines[i].indexOf('<') < 0 || lines[i].indexOf('<') > lines[i].indexOf('role=')) continue;
        let body = '';
        for (let j = i; j < Math.min(lines.length, i + 14); j++) {
          body += lines[j] + String.fromCharCode(10);
          if (/[^=<>]>/.test(lines[j]) || /^\s*>\s*$/.test(lines[j])) break;
        }
        if (/aria-label=|aria-labelledby=/.test(body)) continue;
        offenders.push(key + ':' + (i + 1));
      }
    }
    expect(offenders, 'role=dialog 缺可访问名（补 aria-label 或 aria-labelledby）').toEqual([]);
  });
});


// R429：弹窗遮罩是全屏 fixed，但挂在各 view 的滚动容器里——滚轮落在遮罩上时背景页跟着滚。
// 两个进场钩子都必须挂 lockAncestorScroll：锁住弹窗的可滚动祖先、按引用计数恢复。
describe('弹窗打开锁背景滚动（R429）', () => {
  it('dialogFocus.ts 提供 lockAncestorScroll：锁可滚动祖先、引用计数恢复', () => {
    const df = RAW_DF['./dialogFocus.ts'] ?? '';
    expect(df).toContain('export function lockAncestorScroll(');
    expect(df).toContain("el.style.overflow = 'hidden';");
    expect(df).toContain('entry.count += 1;');
    expect(df).toContain('if (entry.count > 0) continue;');
    expect(df).toContain('data-scroll-locked');
  });

  it('useReturnFocus 与 useDialogFocus 打开期间都锁、关闭时都解锁', () => {
    const df = RAW_DF['./dialogFocus.ts'] ?? '';
    const rf = df.slice(df.indexOf('export function useReturnFocus'), df.indexOf('export function useDialogFocus'));
    const dg = df.slice(df.indexOf('export function useDialogFocus'), df.indexOf('// ── R429'));
    const segs = [rf, dg];
    expect(segs.length).toBe(2);
    for (const seg of segs) {
      expect(seg).toContain('lockAncestorScroll(');
      expect(seg).toContain('unlockScroll();');
    }
  });
});
describe('弹窗滚动的可配置锁（R430）', () => {
  const RAW_CM = import.meta.glob('./components/ContextMenu.tsx', { query: '?raw', import: 'default', eager: true });
  const ctx = String(RAW_CM['./components/ContextMenu.tsx'] ?? '');
  const df = String(RAW_DF['./dialogFocus.ts'] ?? '');

  it('useReturnFocus 与 useDialogFocus 支持 lockScroll 开关（默认开）', () => {
    // R430：模态弹窗默认锁背景滚动；瞬态浮层（右键菜单）可关掉。
    expect(String(df)).toContain('export type DialogFocusOptions = { lockScroll?: boolean };');
    expect(String(df)).toContain('const lockScroll = options.lockScroll !== false;');
    expect(String(df)).toContain('lockScroll ? lockAncestorScroll(topDialog(), viewport()) : () => {};');
    expect(String(df)).toContain('lockScroll ? lockAncestorScroll(top, viewport()) : () => {};');
    expect(String(df)).toContain('}, [open, lockScroll]);');
  });

  it('右键菜单是瞬态浮层，明确关掉背景滚动锁（滚动关闭菜单口径不变）', () => {
    expect(String(ctx)).toContain('useReturnFocus(state !== null, { lockScroll: false });');
    expect(String(ctx)).toContain("window.addEventListener('scroll', handleScroll, true);");
  });
});

// ── 表格表头的 scope 绑定（R432）────────────────────────
// 全仓唯一数据表（toolwall 的 CLI 参数表）此前 th 没有 scope——读屏把列头当
// 游离文本、不与数据格绑定。规则：任何 <th> 必须带 scope= 属性，新增表格
// 忘了写 scope 直接红。
describe('表格表头的 scope 绑定（R432）', () => {
  it('全仓 <th> 都必须声明 scope', () => {
    const offenders: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      const bad = [...src.matchAll(/<th(?=[\s>])(?![^>]*\bscope=)[^>]*>/g)];
      if (bad.length) offenders.push(String(path) + '：' + bad.length + ' 个 <th> 无 scope');
    }
    expect(offenders).toEqual([]);
  });
  it('toolwall 参数表四列都带 scope="col"', () => {
    const src = RAW_TSX['./components/toolbelt/toolwall.tsx'] as string;
    expect((src.match(/<th scope="col">/g) ?? []).length).toBe(4);
  });
});

// ── 搜索框的可访问名（R433）────────────────────────────
// 图标不是可访问名：label 里只放 <Search/>、input 只写 placeholder 时，
// 读屏读不出这个框是搜什么的（placeholder 不是可靠的可访问名来源）。
// 规则：placeholder 输入框必须有 aria-label / aria-labelledby，
// 或所在 label 里带真实文字（t() 或文字节点）作隐式标签。
describe('搜索框的可访问名（R433）', () => {
  it('placeholder 输入框都有可访问名（aria-label 或 label 文字）', () => {
    const offenders: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      const lines = String(src).split('\n');
      for (let i = 0; i < lines.length; i++) {
        if (!lines[i].includes('<input')) continue;
        const idx = lines[i].indexOf('<input');
        let depth = 0;
        let tag = '';
        let done = false;
        for (let j = i; j < lines.length && !done; j++) {
          const line = lines[j];
          for (let k = j === i ? idx : 0; k < line.length; k++) {
            const c = line[k];
            if (c === '{') depth++;
            else if (c === '}') depth = Math.max(0, depth - 1);
            if (c === '>' && depth === 0 && (j > i || k > idx)) {
              done = true;
              tag += '>';
              break;
            }
            tag += c;
          }
        }
        if (!/placeholder=/.test(tag) || /aria-label=|aria-labelledby=/.test(tag)) continue;
        let labelText = false;
        for (let q = i - 1; q >= Math.max(0, i - 12); q--) {
          if (lines[q].includes('</label>')) break;
          if (lines[q].includes('<label')) {
            let region = '';
            for (let m = q; m <= i - 1; m++) region += lines[m] + '\n';
            const text = region.replace(/<[^>]*>/g, ' ').replace(/\/\*[\s\S]*?\*\//g, '');
            labelText = /t\(|[\u4e00-\u9fff]|[A-Za-z]/.test(text);
            break;
          }
        }
        if (!labelText) offenders.push(String(path) + ':' + (i + 1));
      }
    }
    expect(offenders).toEqual([]);
  });
  it('三个图标搜索框都有显式 aria-label', () => {
    expect(String(RAW_TSX['./components/history/HistoryPage.tsx'])).toContain("aria-label={t('history.searchAria')}");
    expect(String(RAW_TSX['./components/StartupPanel.tsx'])).toContain("aria-label={t('system.startup.searchAria')}");
    expect(String(RAW_TSX['./components/SystemTools.tsx'])).toContain("aria-label={t('system.driver.searchAria')}");
  });
});

const RAW_PV = import.meta.glob('./advisor/provider.ts', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

describe('AI 配置保存的空白归一化（R437）', () => {
  const settings = RAW_TSX['./components/Settings.tsx'] ?? '';

  it('保存与试连两处都先归一化，再把 canonical 值写回输入框', () => {
    expect((settings.match(/const n = normalizeSettings\(/g) || []).length).toBe(2);
    expect((settings.match(/setBaseUrl\(n\.baseUrl\)/g) || []).length).toBe(2);
    expect((settings.match(/setModel\(n\.model\)/g) || []).length).toBe(2);
    expect((settings.match(/setApiKey\(n\.apiKey\)/g) || []).length).toBe(2);
    expect(settings).not.toContain('saveSettings({ provider, model, apiKey, baseUrl');
  });

  it('provider 层读与写都过同一个 normalizeSettings 收口', () => {
    const src = RAW_PV['./advisor/provider.ts'] ?? '';
    expect(src).toContain('export function normalizeSettings(');
    expect(src).toContain('return normalizeSettings(parsed);');
    expect(src).toContain('JSON.stringify(normalizeSettings(s))');
  });
});

describe('表格的标题（R438）', () => {
  it('唯一表格的 <caption> 在表头之前、读屏能念出标题', () => {
    const tw = RAW_TSX['./components/toolbelt/toolwall.tsx'] ?? '';
    const tIdx = tw.indexOf('<table className="tdm-params">');
    const cIdx = tw.indexOf('<caption className="sr-only">');
    const thIdx = tw.indexOf('<thead>');
    expect(tIdx).toBeGreaterThanOrEqual(0);
    expect(cIdx).toBeGreaterThan(tIdx);
    expect(thIdx).toBeGreaterThan(cIdx);
    expect(tw.slice(cIdx, cIdx + 130)).toContain('toolbelt.detail.paramsCaption');
  });
  it('.sr-only 工具类存在于全局样式且视觉隐藏', () => {
    const src = RAW_CSS['./styles/tokens.css'] ?? '';
    const block = src.slice(src.indexOf('.sr-only'), src.indexOf('.sr-only') + 300);
    expect(block).toContain('position: absolute');
    expect(block).toContain('clip: rect(0 0 0 0)');
    expect(block).toContain('white-space: nowrap');
    expect(block).toContain('border: 0');
    expect(block).toContain('overflow: hidden');
  });
});

describe('搜索框 Esc 清空（R439）', () => {
  it('有文字时 Esc 清空并吞掉事件（不冒泡去关弹窗）', () => {
    const clear = vi.fn(); const pd = vi.fn(); const sp = vi.fn();
    clearSearchOnEscape(clear)({ key: 'Escape', preventDefault: pd, stopPropagation: sp, currentTarget: { value: 'abc' } });
    expect(clear).toHaveBeenCalledTimes(1);
    expect(pd).toHaveBeenCalledTimes(1);
    expect(sp).toHaveBeenCalledTimes(1);
  });
  it('空输入时放行（Esc 走弹窗栈）', () => {
    const clear = vi.fn(); const sp = vi.fn();
    clearSearchOnEscape(clear)({ key: 'Escape', stopPropagation: sp, currentTarget: { value: '' } });
    expect(clear).not.toHaveBeenCalled();
    expect(sp).not.toHaveBeenCalled();
  });
  it('其它键不动', () => {
    const clear = vi.fn();
    clearSearchOnEscape(clear)({ key: 'Enter', currentTarget: { value: 'x' } });
    expect(clear).not.toHaveBeenCalled();
  });
  it('六个搜索框都接上 clearSearchOnEscape', () => {
    const sites: [string, string][] = [
      ['./components/history/HistoryPage.tsx', 'clearSearchOnEscape(() => setQuery('],
      ['./components/StartupPanel.tsx', 'clearSearchOnEscape(() => setQuery('],
      ['./components/SystemTools.tsx', 'clearSearchOnEscape(() => setDrvQuery('],
      ['./components/toolbelt/toolwall.tsx', 'clearSearchOnEscape(() => setQuery('],
      ['./components/FileView.tsx', 'clearSearchOnEscape(() => onQueryChange('],
      ['./components/SteamInspector.tsx', 'clearSearchOnEscape(() => setSearchQuery('],
    ];
    for (const [path, needle] of sites) {
      const src = RAW_TSX[path] ?? '';
      expect(src).toContain(needle);
    }
  });
});

describe('搜索框 Esc 清空的输入法组合守卫（R458）', () => {
  it('组合中按 Esc 不清空、也不吞事件（交给输入法取消组字）', () => {
    const clear = vi.fn(); const pd = vi.fn(); const sp = vi.fn();
    const ev = { key: 'Escape', isComposing: true, preventDefault: pd, stopPropagation: sp, currentTarget: { value: 'wo' } };
    clearSearchOnEscape(clear)(ev);
    expect(clear).not.toHaveBeenCalled();
    expect(pd).not.toHaveBeenCalled();
    expect(sp).not.toHaveBeenCalled();
  });
  it('原生事件只剩 isComposing / keyCode 229 时也算组合期', () => {
    for (const nativeEvent of [{ isComposing: true }, { keyCode: 229 }]) {
      const clear = vi.fn(); const pd = vi.fn();
      clearSearchOnEscape(clear)({ key: 'Escape', nativeEvent, preventDefault: pd, currentTarget: { value: 'nihao' } });
      expect(clear).not.toHaveBeenCalled();
      expect(pd).not.toHaveBeenCalled();
    }
  });
  it('组合期之外仍照常清空（没有把 R439 的行为改没）', () => {
    const clear = vi.fn();
    clearSearchOnEscape(clear)({ key: 'Escape', isComposing: false, nativeEvent: { keyCode: 27 }, currentTarget: { value: 'abc' } });
    expect(clear).toHaveBeenCalledTimes(1);
  });
  it('a11y.ts 的三路组合判定都在（isComposing / nativeEvent.isComposing / keyCode 229）', () => {
    const src = RAW_A11Y['./a11y.ts'] ?? '';
    expect(src).toContain('if (e.isComposing || e.nativeEvent?.isComposing || e.nativeEvent?.keyCode === 229) return;');
    expect(src).toContain('isComposing?: boolean;');
    expect(src).toContain('nativeEvent?: { isComposing?: boolean; keyCode?: number };');
  });
});

describe('截断文本的悬停提示（R440）', () => {
  it('磁盘型号 / 主板型号的 hw-name 都有完整值的 title', () => {
    const hw = RAW_TSX['./components/HwPanels.tsx'] ?? '';
    // 三处 hw-name（内存盘 / 磁盘 / 主板）全部带 title，且磁盘分支与 title 同源变量
    expect(hw.split('<span className="hw-name" title=').length - 1).toBe(3);
    expect(hw).toContain('const hwName = letters.length > 0');
    expect(hw).toContain('<span className="hw-name" title={hwName}>{hwName}</span>');
  });
  it('winapp2 规则名与周报磁盘名都有 title', () => {
    const st = RAW_TSX['./components/Studio.tsx'] ?? '';
    expect(st).toContain('<span className="winapp2-item-name" title={item.name}>{item.name}</span>');
    const wh = RAW_TSX['./components/history/WeeklyHealthCard.tsx'] ?? '';
    expect(wh).toContain('<span className="wh-disks-name" title={');
  });
});

describe('台账刷新按钮的进行中反馈（R441）', () => {
  it('HistoryPage 刷新防重入、按钮禁用并换成转圈', () => {
    const page = RAW_TSX['./components/history/HistoryPage.tsx'] ?? '';
    const start = page.indexOf('const refresh = useCallback(');
    const region = start >= 0 ? page.slice(start, page.indexOf('}, [t, limit]);', start)) : '';
    // R441：重入闸门；R455：闸门升级成「进行中 + 一次补跑」（上限在途放宽时补拉）
    expect(region).toContain('if (!gateRef.current.tryStart()) return;');
    expect(region).toContain('setRefreshing(true);');
    expect(region).toContain('.finally(() => {');
    expect(region).toContain('if (!gateRef.current.finish()) {');
    expect(region).toContain('setRefreshing(false);');
    expect(region).toContain('refreshRef.current?.();');
    expect(page).toContain('const [refreshing, setRefreshing] = useState(false);');
    expect(page).toContain('const gateRef = useRef(createRefreshGate());');
    expect(page).toContain('refreshRef.current = refresh;');
    expect(page).not.toContain('refreshingRef');
    // 按钮：禁用 + aria-busy + 图标随 refreshing 切换（不再是恒 RotateCcw）
    expect(page).toContain('disabled={refreshing}');
    expect(page).toContain('aria-busy={refreshing || undefined}');
    expect(page).toContain('{refreshing ? <Loader2 size={12} className="spin" /> : <RotateCcw size={12} />}');
    expect(page).not.toContain("<RotateCcw size={12} /> {t('history.refresh')}");
  });

  it('UndoPanel 重试按钮同样防重入并给进行中反馈', () => {
    const panel = RAW_TSX['./components/UndoPanel.tsx'] ?? '';
    const start = panel.indexOf('const refresh = useCallback(');
    const region = start >= 0 ? panel.slice(start, panel.indexOf('}, []);', start)) : '';
    expect(region).toContain('if (refreshingRef.current) return;');
    expect(region).toContain('setRefreshing(true);');
    expect(region).toContain('.finally(() => {');
    expect(region).toContain('setRefreshing(false);');
    expect(panel).toContain('const [refreshing, setRefreshing] = useState(false);');
    expect(panel).toContain('const refreshingRef = useRef(false);');
    expect(panel).toContain('disabled={refreshing}');
    expect(panel).toContain('aria-busy={refreshing || undefined}');
    expect(panel).toContain('{refreshing ? <Loader2 size={11} className="spin" /> : <RotateCcw size={11} />}');
  });
});

// R443：开关按钮此前只带 aria-checked——aria-checked 在 role=button 上不合法，
// 读屏（NVDA/JAWS）不会念出开/关状态，用户只能靠视觉判断。补 role="switch"。
describe('开关按钮的 role="switch"（R443）', () => {
  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let j = start; j < src.length; j++) {
      const c = src[j];
      if (quote) { if (c === '\\') j++; else if (c === quote) quote = ''; continue; }
      if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return j;
    }
    return -1;
  };
  it('非原生控件带 aria-checked 时必须声明 role="switch"', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<\/?[a-zA-Z][\w.-]*/g)) {
        const start = m.index ?? 0;
        if (src[start + 1] === '/') continue; // 闭合标签
        const end = tagEnd(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        if (!tag.includes('aria-checked=')) continue;
        if (/^<input\b/.test(tag)) continue; // 原生 checkbox 自带 role=checkbox
        if (!tag.includes('role="switch"')) {
          violations.push(path + ':' + src.slice(0, start).split('\n').length);
        }
      }
    }
    expect(violations).toEqual([]);
  });
  it('权限中心与启动项的两个开关都已补上 role="switch"', () => {
    const perm = String(RAW_TSX['./components/PermissionCenter.tsx'] ?? '');
    expect(perm.match(/role="switch"/g)?.length ?? 0).toBeGreaterThanOrEqual(2);
    expect(perm.match(/aria-checked=/g)?.length ?? 0).toBeGreaterThanOrEqual(2);
    // L0 常开开关是禁用态，同样要能被念成「开」
    expect(perm).toContain('role="switch" aria-checked={true}');
    const startup = String(RAW_TSX['./components/StartupPanel.tsx'] ?? '');
    expect(startup).toContain('role="switch"');
    expect(startup.match(/aria-checked=/g)?.length ?? 0).toBe(1);
  });
});

// R444：立即检查 / 启动项对比 / 磁盘预测三个结果区块此前都是普通 div——
// 读屏用户按「立即检查」后听不到任何结果播报（按钮焦点不会自动跳过去）。
// 补 role="status"（隐式 polite 活区），结果一出现就读出来。
describe('检查结果的活区播报（R444）', () => {
  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let j = start; j < src.length; j++) {
      const c = src[j];
      if (quote) { if (c === '\\') j++; else if (c === quote) quote = ''; continue; }
      if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return j;
    }
    return -1;
  };
  it('reminder-result 结果区块必须带 role="status"', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<div\b/g)) {
        const start = m.index ?? 0;
        const end = tagEnd(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        if (!tag.includes('className="reminder-result"')) continue;
        if (!tag.includes('role="status"')) {
          violations.push(path + ':' + src.slice(0, start).split('\n').length);
        }
      }
    }
    expect(violations).toEqual([]);
  });
  it('ReminderSettings 三处结果区块都已接入', () => {
    const rs = String(RAW_TSX['./components/ReminderSettings.tsx'] ?? '');
    expect(rs.match(/className="reminder-result" role="status"/g)?.length ?? 0).toBe(3);
  });
});

// R445：选中/当前状态此前有些只用 className 里的 active 表达（视觉颜色），
// 读屏听不到。会话列表的活动会话、市场页的两个分页按钮就是漏网之鱼——
// 旧守卫只认 `' active'`（带前导空格），`'active'` 写法直接绕过。这里改成
// 按钮类一律必须有 aria 状态，并把漏网的两处补上。
describe('当前项的语义（R445）', () => {
  const tagEnd = (src: string, start: number): number => {
    let brace = 0;
    let quote = '';
    for (let j = start; j < src.length; j++) {
      const c = src[j];
      if (quote) { if (c === '\\') j++; else if (c === quote) quote = ''; continue; }
      if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
      if (c === '{') brace++;
      else if (c === '}') brace--;
      else if (c === '>' && brace <= 0) return j;
    }
    return -1;
  };
  it('className 带 active 的按钮必须声明 aria 状态（含无空格写法）', () => {
    const violations: string[] = [];
    for (const [path, src] of Object.entries(RAW_TSX)) {
      for (const m of src.matchAll(/<button\b/g)) {
        const start = m.index ?? 0;
        const end = tagEnd(src, start);
        if (end < 0) continue;
        const tag = src.slice(start, end + 1);
        if (!tag.includes("'active'")) continue;
        if (/aria-pressed=|aria-selected=|aria-current=|aria-checked=/.test(tag)) continue;
        violations.push(path + ':' + src.slice(0, start).split('\n').length);
      }
    }
    expect(violations).toEqual([]);
  });
  it('会话列表的当前会话对读屏可见', () => {
    const cp = String(RAW_TSX['./components/ChatPanel.tsx'] ?? '');
    expect(cp).toContain('aria-current={sess.id === activeChatId ? \'true\' : undefined}');
  });
  it('市场页两个分页按钮有按下态', () => {
    const mk = String(RAW_TSX['./components/toolbelt/market.tsx'] ?? '');
    expect(mk).toContain("aria-pressed={tab === 'builtin'}");
    expect(mk).toContain("aria-pressed={tab === 'community'}");
  });
});
describe('切换会话回到底部（R453）', () => {
  it('切换会话时重置贴底并滚到新会话的底部', () => {
    const cp = String(RAW_TSX['./components/ChatPanel.tsx'] ?? '');
    const start = cp.indexOf('R453：切换会话时回到该会话的底部');
    expect(start).toBeGreaterThan(-1);
    const end = cp.indexOf('}, [activeChatId]);', start);
    expect(end).toBeGreaterThan(start);
    const region = cp.slice(start, end);
    expect(region).toContain('stickRef.current = true;');
    expect(region).toContain('scrollTo({ top: scrollerRef.current.scrollHeight');
    expect(region).toContain('behavior: scrollBehavior() });');
  });
});

describe('搜索框的清空按钮（R461）', () => {
  const RAW_COMMON = import.meta.glob('./i18n/namespaces/common.ts', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
  // R461：四个搜索框（巡检记录 / 启动项 / 驱动 / 文件）都有有词才出现的清空按钮，
  // 与工具墙、Steam 检查器里既有的两个清空按钮对齐——鼠标/触屏不必记 Esc。
  const SITES = [
    './components/history/HistoryPage.tsx',
    './components/StartupPanel.tsx',
    './components/SystemTools.tsx',
    './components/FileView.tsx',
  ];
  it('四个搜索框各有一个条件渲染、带可访问名的清空按钮', () => {
    for (const path of SITES) {
      const src = String(RAW_TSX[path] ?? '');
      expect(src, path).toContain('common.searchClear');
      expect(src, path).toContain('<X size={12} />');
      expect(src, path).toContain("!== '' && (<button");
      const times = src.split('search-clear').length - 1;
      expect(times, path).toBe(1);
    }
  });
  it('清空按钮的样式是紧凑类，且键盘焦点有描边', () => {
    const css = String(RAW_CSS['./styles/layout.css'] ?? '');
    expect(css).toContain('button.search-clear {');
    expect(css).toContain('button.search-clear:focus-visible { outline: 2px solid var(--accent-strong); outline-offset: 1px; }');
    expect(css).toContain('flex-shrink: 0;');
  });
  it('清空按钮的可访问名中英成对', () => {
    const common = String(RAW_COMMON['./i18n/namespaces/common.ts'] ?? '');
    expect(common).toContain('清空搜索');
    expect(common).toContain('Clear search');
  });
});
