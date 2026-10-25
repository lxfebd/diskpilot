import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { Node } from './types';

// C3 改动：selectPath 增带 node 引用（TreeView/Treemap 点击直传），store 其余
// 动作只走 api（recyclePaths/executeAiPlan）。mock api 以便在 node 环境测
// recycle 后的剪枝清选，不触碰真实 Tauri IPC。
vi.mock('./api', () => ({
  api: {
    // 真后端 recyclePaths 返回 UndoEntry[]（每个成功条目一个，含实测 bytes_freed）。
    recyclePaths: vi.fn(async () => []),
    executeAiPlan: vi.fn(async () => []),
  },
}));

import { useStore, toastDuration, toastKey, TOAST_MAX_VISIBLE, type ChatTurn } from './store';
import { api } from './api';

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
  // R226：剪枝后 scanCache 同步落盘 IDB——刷新后 takeDrive 命中持久化树
  // 不再复活已删目录。用与下方「扫描树持久化」describe 相同的内存 fake IDB。
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
    vi.clearAllMocks();
    data.clear();
    vi.stubGlobal('indexedDB', fakeFactory);
    // R256/R258：aiRecyclePaths 有 cleanup.execute 门、recyclePaths 有
    // file.recycle 门（均 L2 默认关）——测试里预置开启，否则提前 return，
    // 剪枝与 bumpScanSeq 都不会发生。
    const store = new Map<string, string>();
    vi.stubGlobal('window', {
      localStorage: {
        getItem: (k: string) => store.get(k) ?? null,
        setItem: (k: string, v: string) => { store.set(k, v); },
        removeItem: (k: string) => { store.delete(k); },
      },
    });
    window.localStorage.setItem(
      'diskpilot.perms.v1',
      JSON.stringify({ enabled: { 'cleanup.execute': true, 'file.recycle': true } }),
    );
    // 回滚回收路径返回真实条目（成功剪枝的前提）。
    (api.recyclePaths as unknown as ReturnType<typeof vi.fn>).mockResolvedValue([
      { source: 'C:\\Users', action: 'recycle' as const, reason: '手动回收', timestamp: '' },
    ]);
    useStore.setState({
      root: ROOT,
      scanCache: { 'C:': ROOT },
      selectedPath: null,
      selectedNode: null,
      chat: { node: null, scaffoldId: null, turns: [], busy: false },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
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

  it('R226：剪枝后的 scanCache 同步落盘 IDB（刷新不复活已删目录）', async () => {
    await useStore.getState().recyclePaths([{ path: 'C:\\Users' }], '手动回收');
    await new Promise((r) => setTimeout(r, 20)); // persist 是 fire-and-forget 异步链
    const saved = data.get('C:') as Node;
    expect(saved).toBeTruthy();
    // 持久化树里 C:\Users 已被剪掉——刷新后 takeDrive 不再复活它。
    const names = (saved.children ?? []).map((c) => c.name);
    expect(names).toEqual(['Windows']);
  });

  // R256：剪枝统一 bump scanSeq——三条清理写路径（右键回收 / AI 提案 /
  // 清理页）都让依赖 scanSeq 的读者（脚本卡「上次清理」徽标、总览 loadClean）
  // 收到台账已变信号。此前只有 pruneRecycled bump，右键/AI 路径静默。
  it('R256：recyclePaths 成功剪枝后 scanSeq 递增（台账读者收到信号）', async () => {
    const before = useStore.getState().scanSeq;
    await useStore.getState().recyclePaths([{ path: 'C:\\Users' }], '手动回收');
    expect(useStore.getState().scanSeq).toBe(before + 1);
  });

  it('R256：aiRecyclePaths 成功剪枝后 scanSeq 递增（AI 提案同信号）', async () => {
    (api.executeAiPlan as unknown as ReturnType<typeof vi.fn>).mockResolvedValue([
      { source: 'C:\\Users', action: 'recycle' as const, reason: 'AI 清理', timestamp: '' },
    ]);
    const before = useStore.getState().scanSeq;
    await useStore.getState().aiRecyclePaths([{ path: 'C:\\Users' }]);
    expect(useStore.getState().scanSeq).toBe(before + 1);
  });

  it('R256：pruneRecycled 仍递增（清理页路径，回归）', () => {
    const before = useStore.getState().scanSeq;
    useStore.getState().pruneRecycled(['C:\\Users']);
    expect(useStore.getState().scanSeq).toBe(before + 1);
  });

  // R258：recyclePaths 权限门——file.recycle 是 L2 高危默认关。权限中心
  // 关掉后右键「移入回收站」必须拒绝（此前直接 recycle_paths 漏过权限表）。
  it('R258：file.recycle 关闭 → recyclePaths 拒绝，不调 api，不剪树不记账', async () => {
    window.localStorage.setItem(
      'diskpilot.perms.v1',
      JSON.stringify({ enabled: { 'cleanup.execute': true, 'file.recycle': false } }),
    );
    const beforeSeq = useStore.getState().scanSeq;
    const res = await useStore.getState().recyclePaths([{ path: 'C:\\Users' }], '手动回收');
    expect(res).toEqual({ done: 0, failed: 1, bytes: 0 });
    expect(api.recyclePaths).not.toHaveBeenCalled();
    expect(useStore.getState().scanSeq).toBe(beforeSeq); // 剪枝未发生
    expect(useStore.getState().root!.children!.map((c) => c.name)).toEqual(['Users', 'Windows']);
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

// R342：toast 停留时长按「要读的量 / 要不要动手」分级——错误与动作提示不能再一律 5 秒。
describe('toastDuration（停留时长分级，R342）', () => {
  it('默认按 kind 分级：ok 5s / warn 7s / err 9s', () => {
    expect(toastDuration('ok', '已复制路径', false)).toBe(5000);
    expect(toastDuration('warn', '磁盘快满了', false)).toBe(7000);
    expect(toastDuration('err', '复制失败', false)).toBe(9000);
  });

  it('长文案加时：>60 字 +2s，>140 字再 +2s', () => {
    const mid = 'x'.repeat(61);
    expect(toastDuration('err', mid, false)).toBe(11_000);
    expect(toastDuration('err', 'x'.repeat(141), false)).toBe(13_000);
    expect(toastDuration('ok', 'x'.repeat(60), false)).toBe(5000); // 正好 60 不加
  });

  it('带动作按钮 +3s——按钮得留出可点的时间', () => {
    expect(toastDuration('ok', '散热持续偏高', true)).toBe(8000);
    expect(toastDuration('err', 'x'.repeat(141), true)).toBe(16_000); // 封顶
  });

  it('再长也不超过 16 秒封顶（不挡视线）', () => {
    expect(toastDuration('err', 'x'.repeat(2000), true)).toBe(16_000);
  });
});
// R364：相同 toast 只留一条——反复触发的同一提醒不再在栈里堆一摞，
// 原地替换最晚那条并重记停留时长（旧定时器 dismiss 扑空无害）。
describe('toast 去重（R364）', () => {
  beforeEach(() => {
    useStore.setState({ toasts: [] });
  });

  it('toastKey 按 文本+级别+动作标签 构成标识', () => {
    expect(toastKey('x', 'ok')).toBe('ok\u0001x\u0001');
    expect(toastKey('x', 'err', '去处理')).toBe('err\u0001x\u0001去处理');
    expect(toastKey('x', 'err')).not.toBe(toastKey('x', 'err', '去处理'));
    expect(toastKey('x', 'ok')).not.toBe(toastKey('x', 'warn'));
  });

  it('相同文本+级别的 toast 只留一条（原地替换）', () => {
    useStore.getState().toast('又失败了', 'err');
    useStore.getState().toast('又失败了', 'err');
    expect(useStore.getState().toasts).toHaveLength(1);
    expect(useStore.getState().toasts[0].text).toBe('又失败了');
    expect(useStore.getState().toasts[0].kind).toBe('err');
  });

  it('不同文本 / 级别 / 动作按钮的 toast 各自成条', () => {
    useStore.getState().toast('磁盘快满了', 'warn');
    useStore.getState().toast('磁盘快满了', 'err');
    useStore.getState().toast('磁盘快满了', 'err', { label: '去调散热', run: () => {} });
    expect(useStore.getState().toasts).toHaveLength(3);
  });

  it('替换后位置不变（顺序与最早那条一致）', () => {
    useStore.getState().toast('第一条', 'ok');
    useStore.getState().toast('重复', 'warn');
    useStore.getState().toast('重复', 'warn');
    const ts = useStore.getState().toasts;
    expect(ts).toHaveLength(2);
    expect(ts[0].text).toBe('第一条');
    expect(ts[1].text).toBe('重复');
  });
});

// R459：去重只挡住了「同一句话反复喊」。批量安装/清理逐项失败、多盘体检各自报警时，
// 一堆互不相同的提示仍会同时在场：.toast-stack 没有滚动也不封顶，攒到十几条就把
// 窗口下半截糊满、底下按钮全点不到。同屏只留最近 TOAST_MAX_VISIBLE 条，最早的让位
//  （它们本来就快到期；被丢掉的 id 稍后 dismiss 会扑空，无副作用）。
describe('toast 同屏上限（R459）', () => {
  beforeEach(() => {
    useStore.setState({ toasts: [] });
  });

  it('超过上限时只留最近几条，最早的让位', () => {
    for (const s of ['a', 'b', 'c', 'd', 'e', 'f']) useStore.getState().toast(s, 'err');
    const ts = useStore.getState().toasts;
    expect(ts).toHaveLength(TOAST_MAX_VISIBLE);
    expect(ts.map((t) => t.text)).toEqual(['c', 'd', 'e', 'f']);
  });

  it('满员后重复触发的同一条原地替换，不挤掉别人', () => {
    for (const s of ['a', 'b', 'c', 'd']) useStore.getState().toast(s, 'ok');
    useStore.getState().toast('b', 'ok');
    const ts = useStore.getState().toasts;
    expect(ts.map((t) => t.text)).toEqual(['a', 'b', 'c', 'd']);
  });

  it('上限就是 4 条（同屏再多也读不过来）', () => {
    expect(TOAST_MAX_VISIBLE).toBe(4);
  });
});

// R442：切换会话曾把「AI 流式在飞」的当前 turns 直接丢掉——既不归档也没有
// 落盘点，回复到达时 patchChatTurn 在新会话里找不到 turnId 静默丢弃，
// 用户整段对话蒸发。这里按 newChat / archiveActiveChat 的口径先净化归档。
describe('switchChat（切会话）在 AI 在飞时的归档（R442）', () => {
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
      activeChatId: 'chat_a',
      chatSessions: [
        { id: 'chat_b', title: '旧会话', ts: 1, turns: [{ id: 'u0', role: 'user', text: '你好' }] },
      ],
      chat: { node: null, scaffoldId: null, turns: [], busy: false },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('AI 生成中切走：在飞 turns 先净化归档，回切仍可见「待续跑」', () => {
    const { switchChat } = useStore.getState() as unknown as {
      switchChat: (id: string) => void;
    };
    useStore.setState({
      chat: {
        node: null,
        scaffoldId: null,
        busy: true,
        turns: [
          { id: 'u1', role: 'user', text: '扫一下 J:\\' },
          { id: 'a1', role: 'assistant', text: '已扫完 J:\\ · 1.70 TB · 3,174,982 个文件。AI 正在生成整体解析…', pending: true, overviewPath: 'J:\\' },
        ],
      },
    });
    switchChat('chat_b');
    // 在飞回合归档进 chatSessions：pending 已净化、保留 overviewPath 供补跑
    const raw = localStorage.getItem('diskpilot.chatSessions') ?? '[]';
    const sessions = JSON.parse(raw) as Array<{
      id: string;
      turns: Array<{ id: string; pending?: boolean; overviewPath?: string; needsResume?: boolean }>;
    }>;
    const a1 = sessions.find((x) => x.id === 'chat_a')?.turns.find((t) => t.id === 'a1');
    expect(a1?.pending).toBe(false);
    expect(a1?.needsResume).toBe(true);
    expect(a1?.overviewPath).toBe('J:\\');
    // 切到目标会话，在飞 busy 不复用（旧请求只属于旧会话）
    expect(useStore.getState().activeChatId).toBe('chat_b');
    expect(useStore.getState().chat.turns.map((tm) => tm.id)).toEqual(['u0']);
    expect(useStore.getState().chat.busy).toBe(false);
  });

  it('非在飞切换：不写归档，chatSessions 原样', () => {
    const { switchChat } = useStore.getState() as unknown as {
      switchChat: (id: string) => void;
    };
    switchChat('chat_b');
    expect(localStorage.getItem('diskpilot.chatSessions')).toBeNull();
    expect(useStore.getState().chatSessions.map((x) => x.id)).toEqual(['chat_b']);
    expect(useStore.getState().chat.turns.map((tm) => tm.id)).toEqual(['u0']);
  });

  it('点自己正在看的会话：空操作，实时 turns 不被旧存档覆盖', () => {
    const { switchChat } = useStore.getState() as unknown as {
      switchChat: (id: string) => void;
    };
    useStore.setState({
      chat: {
        node: null,
        scaffoldId: null,
        busy: false,
        turns: [{ id: 'live', role: 'user', text: '新回合' }],
      },
    });
    switchChat('chat_a');
    expect(useStore.getState().chat.turns.map((tm) => tm.id)).toEqual(['live']);
    expect(localStorage.getItem('diskpilot.chatSessions')).toBeNull();
  });
});

// R451：清空对话（resetChat）曾把活动会话从列表移除、清空 turns，但既不把
// activeChatId 归 null、也不同步持久化——残留 id 会让之后的归档（关窗
// archiveActiveChat / 新建 newChat / 在飞切走 switchChat）用旧 id 把刚落盘的
// 会话写回列表：用户刚清空的对话，关个窗又在历史里复活。
describe('resetChat（清空对话）不复活已清掉的会话（R451）', () => {
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
      activeChatId: 'chat_a',
      chatSessions: [
        { id: 'chat_a', title: '要清空的会话', ts: 1, turns: [{ id: 'u0', role: 'user', text: '我在清空前说的话' }] },
      ],
      chat: { node: null, scaffoldId: null, turns: [{ id: 'u0', role: 'user', text: '我在清空前说的话' }], busy: false },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('清空后 activeChatId 归 null，持久化键一并移除', () => {
    useStore.getState().resetChat();
    expect(useStore.getState().activeChatId).toBeNull();
    expect(localStorage.getItem('diskpilot.activeChatId')).toBeNull();
    expect(useStore.getState().chatSessions).toEqual([]);
  });

  it('清空后产生新回合并关窗归档：旧会话不复活，落入的是新 id', () => {
    useStore.getState().resetChat();
    useStore.setState({
      chat: { node: null, scaffoldId: null, turns: [{ id: 'n1', role: 'user', text: '清空后的新问题' }], busy: false },
    });
    const { archiveActiveChat } = useStore.getState() as unknown as {
      archiveActiveChat: () => void;
    };
    archiveActiveChat();
    const raw = localStorage.getItem('diskpilot.chatSessions') ?? '[]';
    const sessions = JSON.parse(raw) as Array<{ id: string; turns: Array<{ id: string }> }>;
    expect(sessions.find((x) => x.id === 'chat_a')).toBeUndefined();
    expect(sessions.some((x) => x.id !== 'chat_a' && x.turns.some((t) => t.id === 'n1'))).toBe(true);
  });
});

// R452：写盘时把会话列表截到 20，但内存不截——一次会话里新建超过 20 个会话，
// 侧边栏会显示第 21+ 条，重启后它们静默消失（两层皮）。上限统一到
// MAX_CHAT_SESSIONS 常量：新建 / 归档 / 在飞切换都先截再落盘。
describe('会话列表上限（R452）', () => {
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
      activeChatId: null,
      chatSessions: [],
      chat: { node: null, scaffoldId: null, turns: [], busy: false },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('新建 21 个会话：内存与写盘都停在 20，最旧的被丢', () => {
    for (let i = 0; i < 21; i += 1) {
      // 每个新会话用全新 id 归档（真实用法里 activeChatId 是未归档的在飞会话，
      // 不与列表中的历史条目重合）；这里把 activeChatId 归 null 模拟同样语义。
      useStore.setState({ activeChatId: null });
      useStore.setState({
        chat: { node: null, scaffoldId: null, turns: [{ id: 't' + i, role: 'user', text: 'q' + i }], busy: false },
      });
      useStore.getState().newChat();
    }
    const mem = useStore.getState().chatSessions;
    expect(mem.length).toBe(20);
    expect(mem[0].title).toBe('q20');
    expect(mem.some((x) => x.title === 'q0')).toBe(false);
    const rawSaved = localStorage.getItem('diskpilot.chatSessions') ?? '[]';
    const saved = JSON.parse(rawSaved) as Array<{ title: string }>;
    expect(saved.length).toBe(20);
    expect(saved.map((x) => x.title)).toEqual(mem.map((x) => x.title));
  });

  it('归档与在飞切换同样受上限约束：新归档进首位，最旧被挤掉', () => {
    const existing: Array<{ id: string; title: string; ts: number; turns: Array<{ id: string; role: 'user'; text: string }> }> = [];
    for (let i = 0; i < 20; i += 1) {
      existing.push({ id: 'c' + (19 - i), title: 'c' + (19 - i), ts: i, turns: [{ id: 'x' + i, role: 'user', text: 'c' + i }] });
    }
    useStore.setState({
      chatSessions: existing,
      chat: { node: null, scaffoldId: null, turns: [{ id: 'n1', role: 'user', text: '归档的新对话' }], busy: false },
    });
    useStore.getState().archiveActiveChat();
    const mem = useStore.getState().chatSessions;
    expect(mem.length).toBe(20);
    expect(mem[0].title).toBe('归档的新对话');
    expect(mem[0].id.startsWith('chat_')).toBe(true);
    expect(mem.some((x) => x.id === 'c0')).toBe(false);
    const rawSaved = localStorage.getItem('diskpilot.chatSessions') ?? '[]';
    const saved = JSON.parse(rawSaved) as Array<{ id: string }>;
    expect(saved.length).toBe(20);
    expect(saved.some((x) => x.id === 'c0')).toBe(false);
  });
});
describe('toast 倒计时可暂停（R467）', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('到期自动关闭', () => {
    useStore.getState().toast('普通提示', 'ok');
    vi.advanceTimersByTime(toastDuration('ok', '普通提示', false) + 1);
    expect(useStore.getState().toasts).toHaveLength(0);
  });

  it('暂停后到期不关，续上后按剩余时长走完才关', () => {
    useStore.getState().toast('带动作的提示', 'warn', { label: '去调散热', run: () => {} });
    const id = useStore.getState().toasts[0].id;
    const dur = toastDuration('warn', '带动作的提示', true);
    vi.advanceTimersByTime(1000);
    useStore.getState().pauseToast(id);
    vi.advanceTimersByTime(dur * 5);
    expect(useStore.getState().toasts).toHaveLength(1);
    useStore.getState().resumeToast(id);
    vi.advanceTimersByTime(dur - 1000 - 1);
    expect(useStore.getState().toasts).toHaveLength(1);
    vi.advanceTimersByTime(2);
    expect(useStore.getState().toasts).toHaveLength(0);
  });

  it('重复暂停 / 未登记 id 都静默，不抛', () => {
    useStore.getState().toast('x', 'ok');
    const id = useStore.getState().toasts[0].id;
    useStore.getState().pauseToast(id);
    useStore.getState().pauseToast(id);
    useStore.getState().resumeToast(id);
    useStore.getState().resumeToast(id);
    useStore.getState().pauseToast('ghost');
    useStore.getState().resumeToast('ghost');
    vi.advanceTimersByTime(toastDuration('ok', 'x', false) + 1);
    expect(useStore.getState().toasts).toHaveLength(0);
  });

  it('手动关闭收掉定时器，不再触发 dismiss', () => {
    useStore.getState().toast('y', 'ok');
    const id = useStore.getState().toasts[0].id;
    vi.advanceTimersByTime(100);
    useStore.getState().dismissToast(id);
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(100000);
    expect(useStore.getState().toasts).toHaveLength(0);
  });
});
