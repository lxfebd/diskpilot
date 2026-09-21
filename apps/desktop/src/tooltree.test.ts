import { describe, it, expect } from 'vitest';
import { pruneMany, treeMayContain } from './tooltree';
import type { Node } from './types';

function mk(name: string, path: string, size: number, file_count: number, children: Node[] = []): Node {
  return { name, path, is_dir: true, size, file_count, children, top_extensions: [] };
}

const TREE: Node = mk('C:', 'C:\\', 900, 90, [
  mk('Users', 'C:\\Users', 600, 60, [
    mk('alice', 'C:\\Users\\alice', 500, 50, [
      mk('cache', 'C:\\Users\\alice\\cache', 100, 10),
      mk('docs', 'C:\\Users\\alice\\docs', 50, 5),
    ]),
    mk('bob', 'C:\\Users\\bob', 100, 10),
  ]),
  mk('Windows', 'C:\\Windows', 300, 30),
]);

describe('pruneMany', () => {
  it('removes multiple targets in one pass and adjusts ancestor sizes', () => {
    const out = pruneMany(TREE, ['C:\\Users\\alice\\cache', 'C:\\Windows']);
    expect(out.size).toBe(500); // 900 - 100 - 300
    expect(out.file_count).toBe(50);
    const users = out.children![0];
    expect(users.size).toBe(500); // 600 - 100
    expect(users.file_count).toBe(50);
    const alice = users.children![0];
    expect(alice.size).toBe(400); // 500 - 100
    expect(alice.children.map((c) => c.name)).toEqual(['docs']);
    expect(out.children!.map((c) => c.name)).toEqual(['Users']);
  });

  it('ancestor target covers descendant targets without double counting', () => {
    const out = pruneMany(TREE, ['C:\\Users', 'C:\\Users\\bob']);
    expect(out.size).toBe(300);
    expect(out.file_count).toBe(30);
    expect(out.children!.map((c) => c.name)).toEqual(['Windows']);
  });

  it('is case-insensitive, ignores trailing slashes; no-match returns the same node', () => {
    const out = pruneMany(TREE, ['c:\\USERS\\alice\\docs\\']);
    expect(out.size).toBe(850); // 900 - 50
    const alice = out.children![0].children![0];
    expect(alice.children!.map((c) => c.name)).toEqual(['cache']);
    expect(pruneMany(TREE, ['C:\\nope'])).toBe(TREE);
    expect(pruneMany(TREE, [])).toBe(TREE);
  });
});

describe('treeMayContain', () => {
  it('matches drive roots and nested paths case-insensitively', () => {
    expect(treeMayContain('C:', 'C:\\Users')).toBe(true);
    expect(treeMayContain('C:\\USERS', 'c:\\users\\alice')).toBe(true);
    expect(treeMayContain('C:', 'C:')).toBe(true);
  });

  it('does not match other drives or mere name prefixes', () => {
    expect(treeMayContain('C:', 'D:\\x')).toBe(false);
    // 前缀必须按整段边界匹配：C:\US 不能误吞 C:\Users
    expect(treeMayContain('C:\\US', 'C:\\Users')).toBe(false);
  });

  it('normalizes trailing backslash on the tree key (drive root C:\\ vs child)', () => {
    // 盘符扫描的根节点 path 带尾反斜杠（后端 "C:\\"），回收剪枝判定
    // 必须命中子树，否则刚回收的目录会留在可见树里。
    expect(treeMayContain('C:\\', 'C:\\Users')).toBe(true);
    expect(treeMayContain('c:\\', 'c:\\users\\alice')).toBe(true);
    expect(treeMayContain('C:', 'C:\\Users')).toBe(true);
    expect(treeMayContain('C:\\Users\\', 'C:\\Users\\alice')).toBe(true);
    expect(treeMayContain('C:\\', 'D:\\Users')).toBe(false);
  });
});
