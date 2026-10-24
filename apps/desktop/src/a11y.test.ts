import { describe, it, expect, vi } from 'vitest';
import { activateOnKey, isActivationKey } from './a11y';

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

