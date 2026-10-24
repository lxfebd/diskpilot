import { describe, it, expect, vi } from 'vitest';
import { activateOnKey, cycleIndex, isActivationKey } from './a11y';

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
