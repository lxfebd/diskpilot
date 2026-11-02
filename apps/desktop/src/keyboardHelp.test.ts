import { describe, expect, it } from 'vitest';

// R494 守卫：? 键快捷键帮助面板。与 a11y.test.ts 同款——静态扫描源码，
// 钉住「唤出键 + 输入框/IME 豁免 + 弹窗语义 + i18n 中英成对」这四处接线。
const RAW_TSX = import.meta.glob('./**/*.tsx', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

describe('键盘快捷键帮助面板（R494）', () => {
  it('App 全局监听 ? 键唤出/关闭，且避开输入框、可编辑区与输入法组合期', () => {
    const app = RAW_TSX['./App.tsx'];
    expect(app).toContain(`e.key !== '?'`);
    expect(app).toContain('isComposing');
    expect(app).toContain(`el.tagName === 'INPUT'`);
    expect(app).toContain('isContentEditable');
    expect(app).toContain('setShowHelp');
    expect(app).toContain('<KeyboardHelp');
  });

  it('帮助面板接入 Esc 栈与焦点管理，dialog 语义齐全', () => {
    const s = RAW_TSX['./components/KeyboardHelp.tsx'];
    expect(s).toContain('useEscapeKey(onClose, open)');
    expect(s).toContain('useDialogFocus(open)');
    expect(s).toContain('role="dialog"');
    expect(s).toContain('aria-modal="true"');
    expect(s).toContain('aria-label={t(\'shell.help.title\')}');
  });

  it('i18n 快捷键文案中英成对、键值非空', () => {
    const ns = import.meta.glob('./i18n/namespaces/shell.ts', {
      query: '?raw',
      import: 'default',
      eager: true,
    }) as Record<string, string>;
    const src = ns['./i18n/namespaces/shell.ts'];
    const keys = [...new Set([...src.matchAll(/'(shell\.help\.[^']+)':/g)].map((m) => m[1]))];
    expect(keys.length).toBeGreaterThanOrEqual(20);
    // 中英各一份：去重后的键数必须是偶数（zh 一组 + en 一组）
    expect(keys.length % 2).toBe(0);
    // 每个键中英各一份，且值非空
    const vals = src.match(/'shell\.help\.[^']+': '[^']+'/g) ?? [];
    expect(vals.length).toBe(keys.length * 2);
  });
});
