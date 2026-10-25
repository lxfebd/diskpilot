import { describe, it, expect } from 'vitest';

// R436：localStorage 字面键的「读了就有人写」对偶守卫。
// 规则：非测试源码里凡 getItem('<字面键>')，全仓必须有同键 setItem 或 removeItem；
// 例外（由外部进程/其它页面写入，需注释说明）放进 ALLOW_GET_ONLY。
const RAW_TS = import.meta.glob('./**/*.ts', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const RAW_TSX = import.meta.glob('./**/*.tsx', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const SRC = { ...RAW_TS, ...RAW_TSX };

const ALLOW_GET_ONLY: string[] = [];

const keyRe = new RegExp("(?:getItem|setItem|removeItem)\\(\\s*(['\"`])((?:\\\\.|(?!\\1).)*)\\1", 'g');

describe('localStorage 读写键对偶（R436）', () => {
  it('凡 getItem 的字面键，全仓必须有同键写入方', () => {
    const reads = new Set<string>();
    const writes = new Set<string>();
    for (const f of Object.keys(SRC)) {
      if (/\.test\./.test(f)) continue;
      const s = SRC[f];
      keyRe.lastIndex = 0;
      let m: RegExpExecArray | null;
      while ((m = keyRe.exec(s))) {
        const op = /getItem/.test(m[0]) ? 'r' : 'w';
        (op === 'r' ? reads : writes).add(m[2]);
      }
    }
    const orphans = [...reads].filter((k) => !writes.has(k) && !ALLOW_GET_ONLY.includes(k));
    expect(orphans).toEqual([]);
  });

  it('Studio 页不再保留无法触达的隐藏分支（hideStudio / studio.hidden 已移除）', () => {
    const st = SRC['./components/Studio.tsx'] ?? '';
    expect(st).not.toContain('hideStudio');
    expect(st).not.toContain('studio.hidden');
  });
});

