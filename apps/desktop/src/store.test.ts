import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Node } from './types';

// C3 改动：selectPath 增带 node 引用（TreeView/Treemap 点击直传），store 其余
// 动作只走 api（recyclePaths/executeAiPlan）。mock api 以便在 node 环境测
// recycle 后的剪枝清选，不触碰真实 Tauri IPC。
vi.mock('./api', () => ({
  api: {
    recyclePaths: vi.fn(async () => undefined),
    executeAiPlan: vi.fn(async () => undefined),
  },
}));

import { useStore } from './store';

function mk(name: string, path: string, size: number, file_count: number, children: Node[] = []): Node {
  return { name, path, is_dir: true, size, file_count, children, top_extensions: [] };
}

const ROOT: Node = mk('C:', 'C:\\', 900, 90, [
  mk('Users', 'C:\\Users', 600, 60, [
    mk('alice', 'C:\\Users\\alice', 500, 50, [
      mk('cache', 'C:\\Users\\alice\\cache', 100, 10),
    ]),
  ]),
  mk('Windows', 'C:\\Windows', 300, 30),
]);

describe('selectPath (C3 node reference)', () => {
  beforeEach(() => {
    useStore.setState({
      root: null,
      scanCache: {},
      selectedPath: null,
      selectedNode: null,
      chat: { node: null, scaffoldId: null, turns: [], busy: false },
    });
  });

  it('stores the selected node reference alongside the path (O(1) for FileDetailPanel)', () => {
    const n = ROOT.children![0]; // C:\Users
    useStore.getState().selectPath(n.path, n);
    const s = useStore.getState();
    expect(s.selectedPath).toBe('C:\\Users');
    expect(s.selectedNode).toBe(n); // 同一引用，非克隆
  });

  it('path-only selection stays supported and leaves selectedNode null', () => {
    useStore.getState().selectPath('C:\\Users\\alice\\cache');
    const s = useStore.getState();
    expect(s.selectedPath).toBe('C:\\Users\\alice\\cache');
    expect(s.selectedNode).toBeNull();
  });

  it('null selection clears both path and node', () => {
    const n = ROOT.children![1]; // C:\Windows
    useStore.getState().selectPath(n.path, n);
    useStore.getState().selectPath(null);
    const s = useStore.getState();
    expect(s.selectedPath).toBeNull();
    expect(s.selectedNode).toBeNull();
  });
});

describe('recycle prune clears selection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useStore.setState({
      root: ROOT,
      scanCache: { 'C:': ROOT },
      selectedPath: null,
      selectedNode: null,
      chat: { node: null, scaffoldId: null, turns: [], busy: false },
    });
  });

  it('clears selectedPath and selectedNode after a successful recycle', async () => {
    const n = ROOT.children![0]; // C:\Users
    useStore.getState().selectPath(n.path, n);
    const res = await useStore.getState().recyclePaths([{ path: 'C:\\Users' }], '手动回收');
    expect(res.done).toBe(1);
    const s = useStore.getState();
    expect(s.selectedPath).toBeNull();
    expect(s.selectedNode).toBeNull(); // C3：剪枝后引用必须随 path 一起清，防 FileDetailPanel 读到已删节点
    expect(s.root!.children!.map((c) => c.name)).toEqual(['Windows']);
  });
});