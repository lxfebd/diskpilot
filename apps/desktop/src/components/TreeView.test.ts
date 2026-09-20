import { describe, it, expect } from 'vitest';
import { flattenTree, isTruncated } from './TreeView';
import type { Node } from '../types';

function mk(
  name: string,
  path: string,
  size: number,
  file_count: number,
  children: Node[] = [],
  children_truncated?: number | null,
): Node {
  return { name, path, is_dir: true, size, file_count, children, top_extensions: [], children_truncated };
}

const TRUNC: Node = mk('C:', 'C:\\', 1000, 100, [
  mk('big', 'C:\\big', 800, 80, [
    mk('a', 'C:\\big\\a', 10, 1),
    mk('b', 'C:\\big\\b', 10, 1),
  ], 200),
  mk('small', 'C:\\small', 200, 20),
], 50);

// 组件默认把 root 加进展开集合，测试保持一致。
const OPEN_ALL = new Set(['C:\\', 'C:\\big']);

describe('flattenTree', () => {
  it('flattens preorder depth-first like the tree renders', () => {
    const rows = flattenTree(TRUNC, OPEN_ALL, new Map(), new Set());
    expect(rows.map((r) => r.node.path)).toEqual(['C:\\', 'C:\\big', 'C:\\big\\a', 'C:\\big\\b', 'C:\\small']);
  });

  it('uses lazy children over node children once fetched', () => {
    const full = [mk('x', 'C:\\big\\x', 1, 1), mk('y', 'C:\\big\\y', 2, 2), mk('z', 'C:\\big\\z', 3, 3)];
    const lazy = new Map([['C:\\big', full]]);
    const rows = flattenTree(TRUNC, OPEN_ALL, lazy, new Set());
    // 截断树自带的 children（a、b）让位于按需取回的完整列表
    expect(rows.map((r) => r.node.path)).toEqual([
      'C:\\',
      'C:\\big',
      'C:\\big\\x',
      'C:\\big\\y',
      'C:\\big\\z',
      'C:\\small',
    ]);
  });

  it('marks loading rows and stops descending while loading', () => {
    const rows = flattenTree(TRUNC, OPEN_ALL, new Map(), new Set(['C:\\big']));
    const big = rows[1];
    expect(big.loading).toBe(true);
    // 取回中不展开子项，避免双渲染
    expect(rows.map((r) => r.node.path)).toEqual(['C:\\', 'C:\\big', 'C:\\small']);
  });

  it('caps a single directory at 5000 entries to protect the main thread', () => {
    const hugeKids: Node[] = [];
    for (let i = 0; i < 6000; i++) hugeKids.push(mk(`f${i}`, `C:\\huge\\f${i}`, 1, 1));
    const huge = mk('huge', 'C:\\huge', 6000, 6000, hugeKids);
    const rows = flattenTree(huge, new Set(['C:\\', 'C:\\huge']), new Map(), new Set());
    const hugeRow = rows.find((r) => r.node.path === 'C:\\huge')!;
    const kidRows = rows.filter((r) => r.node.path.startsWith('C:\\huge\\'));
    expect(kidRows.length).toBe(5000);
    expect(hugeRow).toBeDefined();
  });
});

describe('isTruncated', () => {
  it('shows the badge only when children are capped and not fully loaded', () => {
    const capped = mk('c', 'C:\\c', 1, 1, [], 200);
    const normal = mk('n', 'C:\\n', 1, 1, [], null);
    expect(isTruncated(capped, false)).toBe(true);
    expect(isTruncated(capped, true)).toBe(false);
    expect(isTruncated(normal, false)).toBe(false);
    expect(isTruncated(normal, true)).toBe(false);
  });
});
