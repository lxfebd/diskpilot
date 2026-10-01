import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
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

describe('扫描树持久化（重启不丢）', () => {
  const storage = new Map<string, string>();

  beforeEach(() => {
    storage.clear();
    vi.clearAllMocks();
    // 提供内存版 localStorage，验证落盘/恢复逻辑；缺失时 load/persist 均应静默。
    const fake: Storage = {
      getItem: (k) => storage.get(k) ?? null,
      setItem: (k, v) => { storage.set(k, String(v)); },
      removeItem: (k) => { storage.delete(k); },
      clear: () => storage.clear(),
      key: (i) => [...storage.keys()][i] ?? null,
      get length() { return storage.size; },
    };
    vi.stubGlobal('localStorage', fake);
    useStore.setState({
      root: null,
      scanCache: {},
      selectedPath: null,
      selectedNode: null,
      chat: { node: null, scaffoldId: null, turns: [], busy: false },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('cacheDrives 落盘最近使用的盘树（重启后恢复的数据源）', () => {
    const D_ROOT = mk('D:', 'D:\\', 700, 70);
    useStore.getState().cacheDrives({ 'C:': ROOT, 'D:': D_ROOT });
    const raw = storage.get('diskpilot.scanCache');
    expect(raw).toBeTruthy();
    const parsed = JSON.parse(raw!);
    // 最近使用序：D: 后写入在前，C: 次之（cacheDrives 逐个 touch）
    expect(Object.keys(parsed)).toEqual(['D:', 'C:']);
    expect(parsed['D:'].name).toBe('D:');
    expect(parsed['C:'].name).toBe('C:');
  });

  it('超过预算（4MB）不落盘，静默跳过', () => {
    const huge: Node = { name: 'E:', path: 'E:\\', is_dir: true, size: 0, file_count: 0, top_extensions: [], children: [] };
    // 构造一个序列化后超预算的树：children 里放深递归会爆栈，改用超大字符串节点名。
    const big: Node = {
      name: 'x'.repeat(5 * 1024 * 1024),
      path: 'E:\\x', is_dir: false, size: 1, file_count: 0, top_extensions: [], children: [],
    };
    useStore.getState().cacheDrives({ 'E:': { ...huge, children: [big] } });
    expect(storage.get('diskpilot.scanCache')).toBeUndefined();
  });

  it('localStorage 不可用（node 无全局）时读写均静默，不抛错', () => {
    vi.unstubAllGlobals(); // 移除 fake → localStorage 引用即抛 ReferenceError
    expect(() => useStore.getState().cacheDrives({ 'C:': ROOT })).not.toThrow();
    // 恢复路径同样静默返回空
    expect(useStore.getState().scanCache['C:']).toBe(ROOT); // 内存缓存不受影响
  });
});