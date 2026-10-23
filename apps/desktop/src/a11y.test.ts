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
