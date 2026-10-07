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

import { useStore, type ChatTurn } from './store';

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

describe('扫描树持久化（IndexedDB）', () => {
  const storage = new Map<string, string>();
  // 极简 fake IDB：put/delete/getAllKeys/getAll 对齐 store.ts 的用法，
  // request/transaction 回调用 microtask 触发（与真实 IDB 一致的异步语义）。
  const data = new Map<string, unknown>();
  const microReq = (result: unknown) => {
    const req: { result: unknown; onsuccess: ((e: unknown) => void) | null; onerror: unknown } = {
      result, onsuccess: null, onerror: null,
    };
    queueMicrotask(() => req.onsuccess?.({ target: { result } }));
    return req;
  };
  const os = {
    put: (v: unknown, k: string) => { data.set(k, structuredClone(v)); },
    delete: (k: string) => { data.delete(k); },
    getAllKeys: () => microReq([...data.keys()]),
    getAll: () => microReq([...data.values()]),
  };
  const fakeDb = {
    createObjectStore: () => ({}),
    close: () => {},
    transaction: () => {
      const tx: { objectStore: () => typeof os; oncomplete: (() => void) | null; onerror: unknown; onabort: unknown } = {
        objectStore: () => os, oncomplete: null, onerror: null, onabort: null,
      };
      queueMicrotask(() => tx.oncomplete?.());
      return tx;
    },
  };
  const fakeFactory = {
    open: () => {
      const req: { result: typeof fakeDb; onupgradeneeded: (() => void) | null; onsuccess: ((e: unknown) => void) | null; onerror: unknown; onblocked: unknown } = {
        result: fakeDb, onupgradeneeded: null, onsuccess: null, onerror: null, onblocked: null,
      };
      queueMicrotask(() => {
        req.onupgradeneeded?.();
        req.onsuccess?.({ target: { result: fakeDb } });
      });
      return req;
    },
  };

  beforeEach(() => {
    storage.clear();
    data.clear();
    vi.clearAllMocks();
    // 提供内存版 localStorage + fake IDB，验证落盘/恢复逻辑；缺失时 load/persist 均应静默。
    const fake: Storage = {
      getItem: (k) => storage.get(k) ?? null,
      setItem: (k, v) => { storage.set(k, String(v)); },
      removeItem: (k) => { storage.delete(k); },
      clear: () => storage.clear(),
      key: (i) => [...storage.keys()][i] ?? null,
      get length() { return storage.size; },
    };
    vi.stubGlobal('localStorage', fake);
    vi.stubGlobal('indexedDB', fakeFactory);
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

  it('cacheDrives 把全部缓存盘整树写进 IndexedDB（重启后恢复的数据源）', async () => {
    const D_ROOT = mk('D:', 'D:\\', 700, 70);
    useStore.getState().cacheDrives({ 'C:': ROOT, 'D:': D_ROOT });
    await new Promise((r) => setTimeout(r, 20)); // persist 是 fire-and-forget 异步链
    expect(data.get('C:')).toBeTruthy();
    expect(data.get('D:')).toBeTruthy();
    expect((data.get('D:') as Node).name).toBe('D:');
  });

  it('内存淘汰的盘同步从 IDB 清除', async () => {
    // 塞满 7 盘（MAX_CACHED_DRIVES=6），最早写入的 'C0:' 被逐出。
    const entries: Record<string, Node> = {};
    for (let i = 0; i < 7; i++) {
      entries[`C${i}:`] = mk(`C${i}:`, `C${i}:\\`, 100, 10);
    }
    useStore.getState().cacheDrives(entries);
    await new Promise((r) => setTimeout(r, 20));
    expect(data.has('C0:')).toBe(false); // 被逐出 → IDB 同步删除
    expect(data.has('C6:')).toBe(true);
  });

  it('localStorage 不可用（node 无全局）时读写均静默，不抛错', () => {
    vi.unstubAllGlobals(); // 移除 fake → localStorage/indexedDB 引用即抛 ReferenceError
    expect(() => useStore.getState().cacheDrives({ 'C:': ROOT })).not.toThrow();
    // 恢复路径同样静默返回空
    expect(useStore.getState().scanCache['C:']).toBe(ROOT); // 内存缓存不受影响
  });
});

describe('sanitizeTurns（幽灵 pending 净化）', () => {
  it('总览解析残留 → needsResume（待续跑），不再打中断文案', async () => {
    const { sanitizeTurns } = await import('./store');
    const out = sanitizeTurns([
      { id: 'a', role: 'assistant' as const, text: '已扫完 G:\\ · 374 GB', pending: true, overviewPath: 'G:\\' },
      { id: 'b', role: 'user' as const, text: '把鸣潮删了' },
      { id: 'c', role: 'assistant' as const, text: '已经生成清单', pending: false },
    ]);
    // 总览残留：置 needsResume + 保留 overviewPath，供恢复后自动补跑
    expect(out[0].pending).toBe(false);
    expect(out[0].needsResume).toBe(true);
    expect(out[0].overviewPath).toBe('G:\\');
    expect(out[0].text).not.toContain('中断');
    // 非 pending 回合原样保留
    expect(out[1]).toEqual({ id: 'b', role: 'user', text: '把鸣潮删了' });
    expect(out[2]).toEqual({ id: 'c', role: 'assistant', text: '已经生成清单', pending: false });
  });

  it('非总览 pending（自由对话中断）→ 打中断文案', async () => {
    const { sanitizeTurns } = await import('./store');
    const out = sanitizeTurns([
      { id: 'x', role: 'assistant' as const, text: '让我想想', pending: true },
    ]);
    expect(out[0].pending).toBe(false);
    expect(out[0].text).toContain('中断');
    expect(out[0].needsResume).toBeUndefined();
  });

  it('空 turns / 无 pending 时原样返回', async () => {
    const { sanitizeTurns } = await import('./store');
    expect(sanitizeTurns([])).toEqual([]);
    const turns = [{ id: 'x', role: 'user' as const, text: 'hi' }];
    expect(sanitizeTurns(turns)).toEqual(turns);
  });

  it('旧版中断残留（无 overviewPath + 已含中断文案）→ 迁移成待续跑', async () => {
    const { sanitizeTurns } = await import('./store');
    // 旧版（修复前）归档把 pending 原样落盘，恢复时打的「中断」文案；
    // 新版识别这类残留，从文案还原路径并置 needsResume，下次打开自动补跑
    const out = sanitizeTurns([
      { id: 'old1', role: 'assistant' as const, text: '已扫完 G:\\ · 374 GB · 535,206 个文件。AI 正在生成整体解析…\n\n（上次的分析被中断，未完成）', pending: false },
    ]);
    expect(out[0].needsResume).toBe(true);
    expect(out[0].overviewPath).toBe('G:\\');
  });

  it('非盘根中断文案不迁移（避免误识别自由对话）', async () => {
    const { sanitizeTurns } = await import('./store');
    const out = sanitizeTurns([
      { id: 'x', role: 'assistant' as const, text: '让我想想…\n\n（上次的分析被中断，未完成）', pending: false },
    ]);
    expect(out[0].needsResume).toBeUndefined();
    expect(out[0].overviewPath).toBeUndefined();
  });
});

describe('archiveActiveChat（关窗归档）在 AI 在飞时的断点', () => {
  beforeEach(() => {
    const storage = new Map<string, string>();
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
      activeChatId: 'chat_test',
      chatSessions: [],
      chat: { node: null, scaffoldId: null, turns: [], busy: false },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('AI 生成中关窗：pending 回合归档前被净化成 needsResume，恢复后自动补跑', async () => {
    const { archiveActiveChat } = useStore.getState() as unknown as {
      archiveActiveChat: () => void;
    };
    const { sanitizeTurns } = await import('./store');
    // 模拟真实时序：扫描完成 → pushTurn(pending, overviewPath) → 请求还在飞 → beforeunload
    useStore.setState({
      chat: {
        node: null,
        scaffoldId: null,
        busy: true,
        turns: [
          { id: 'u1', role: 'user' as const, text: '扫一下 J:\\' },
          { id: 'a1', role: 'assistant' as const, text: '已扫完 J:\\ · 1.70 TB · 3,174,982 个文件。AI 正在生成整体解析…', pending: true, overviewPath: 'J:\\' },
        ],
      },
    });
    archiveActiveChat();
    // 归档先净化：pending 回合落盘时已是 needsResume 标记，不再原样 pending:true
    const raw = localStorage.getItem('diskpilot.chatSessions') ?? '[]';
    const sessions = JSON.parse(raw) as Array<{ turns: Array<{ id: string; pending?: boolean; text?: string; overviewPath?: string; needsResume?: boolean }> }>;
    const a1 = sessions[0]?.turns.find((x) => x.id === 'a1');
    expect(a1?.pending).toBe(false); // 已净化，不残留 pending
    expect(a1?.needsResume).toBe(true); // 待续跑标记
    expect(a1?.overviewPath).toBe('J:\\'); // 保留路径供补跑
    expect(a1?.text).not.toContain('中断'); // 不再打中断文案
    // 恢复路径对已净化数据幂等：sanitize 不改变 needsResume 回合
    const out = sanitizeTurns((sessions[0]?.turns ?? []) as unknown as ChatTurn[]);
    const a1r = out.find((x) => x.id === 'a1');
    expect(a1r?.needsResume).toBe(true);
  });

  it('AI 已完成才关窗：pending:false 回合归档，恢复后无中断误报', () => {
    const { archiveActiveChat } = useStore.getState() as unknown as {
      archiveActiveChat: () => void;
    };
    useStore.setState({
      chat: {
        node: null,
        scaffoldId: null,
        busy: false,
        turns: [
          { id: 'u1', role: 'user' as const, text: '扫一下 J:\\' },
          { id: 'a1', role: 'assistant' as const, text: '【整体】J 盘结构清晰…', pending: false },
        ],
      },
    });
    archiveActiveChat();
    const raw = (localStorage.getItem('diskpilot.chatSessions') ?? '[]');
    const sessions = JSON.parse(raw) as Array<{ turns: Array<{ id: string; pending?: boolean }> }>;
    const a1 = sessions[0]?.turns.find((x) => x.id === 'a1');
    // pending:false 原样落盘 → 恢复不会标记中断
    expect(a1?.pending).toBe(false);
    expect(a1 && 'text' in a1 ? (a1 as { text?: string }).text ?? '' : '').not.toContain('中断');
  });
});