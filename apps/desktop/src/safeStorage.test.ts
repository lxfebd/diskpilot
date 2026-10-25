import { describe, it, expect, vi, afterEach } from 'vitest';
import { safeGetItem, safeSetItem, safeRemoveItem } from './safeStorage';

function stubStorage(impl: Partial<Storage>): void {
  vi.stubGlobal('localStorage', impl);
}

afterEach(() => vi.unstubAllGlobals());

describe('safeStorage（R395）', () => {
  it('存储正常时读出、写入、删除都直通', () => {
    const store = new Map<string, string>();
    stubStorage({
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => { store.set(k, v); },
      removeItem: (k: string) => { store.delete(k); },
    } as unknown as Storage);
    expect(safeGetItem('a')).toBeNull();
    safeSetItem('a', '1');
    expect(safeGetItem('a')).toBe('1');
    safeRemoveItem('a');
    expect(safeGetItem('a')).toBeNull();
  });

  it('读失败回 null，写/删失败静默吞掉，不抛', () => {
    stubStorage({
      getItem: () => { throw new Error('no storage'); },
      setItem: () => { throw new Error('quota exceeded'); },
      removeItem: () => { throw new Error('denied'); },
    } as unknown as Storage);
    expect(safeGetItem('a')).toBeNull();
    expect(() => safeSetItem('a', '1')).not.toThrow();
    expect(() => safeRemoveItem('a')).not.toThrow();
  });

  it('迁移守卫：启动/渲染路径不再有裸 localStorage 调用（R395）', () => {
    const RAW = import.meta.glob('./**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
    const banned = ['./App.tsx', './components/LeftPanel.tsx', './components/Settings.tsx', './components/toolbelt/toolwall.tsx', './advisor/provider.ts'];
    const violations: string[] = [];
    for (const rel of banned) {
      const src = RAW[rel];
      if (!src) continue;
      src.split('\n').forEach((line, i) => {
        if (/\blocalStorage\.(getItem|setItem|removeItem)\s*\(/.test(line)) violations.push(rel + ':' + (i + 1));
      });
    }
    expect(violations).toEqual([]);
  });
});
