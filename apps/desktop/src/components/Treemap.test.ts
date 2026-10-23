import { describe, it, expect } from 'vitest';
import type { Node } from '../types';
import { treeIsTruncated } from './Treemap';

function n(name: string, over: Partial<Node> = {}): Node {
  return { name, path: name, is_dir: false, size: 0, file_count: 0, children: [], top_extensions: [], ...over };
}

describe('空间图截断判定（R312：前 300 项 + 折叠目录都要说出来）', () => {
  it('空树 / 无子项 = 没截断', () => {
    expect(treeIsTruncated(null)).toBe(false);
    expect(treeIsTruncated(undefined)).toBe(false);
    expect(treeIsTruncated(n('C:\\'))).toBe(false);
  });

  it('根的子项超过 300 才算根层截断（正好 300 不算）', () => {
    const exactly = n('C:\\', { children: Array.from({ length: 300 }, (_, i) => n('d' + i)) });
    expect(treeIsTruncated(exactly)).toBe(false);
    const over = n('C:\\', { children: Array.from({ length: 301 }, (_, i) => n('d' + i)) });
    expect(treeIsTruncated(over)).toBe(true);
  });

  it('任意层 children_truncated>0 都算截断（0 / null 不算）', () => {
    expect(treeIsTruncated(n('C:\\', { children_truncated: 3 }))).toBe(true);
    expect(treeIsTruncated(n('C:\\', { children_truncated: 0 }))).toBe(false);
    expect(treeIsTruncated(n('C:\\', { children_truncated: null }))).toBe(false);
  });

  it('深层折叠也能挖出来（不止看根）', () => {
    const deep = n('C:\\', {
      children: [n('a', { children: [n('b', { children: [n('c', { children_truncated: 7 })] })] })],
    });
    expect(treeIsTruncated(deep)).toBe(true);
  });

  it('maxChildren 可调（小图也能放宽/收紧口径）', () => {
    const two = n('C:\\', { children: [n('a'), n('b')] });
    expect(treeIsTruncated(two, 3)).toBe(false);
    expect(treeIsTruncated(two, 1)).toBe(true);
  });
});
