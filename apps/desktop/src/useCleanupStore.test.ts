// R178 单测：清理执行的「可取消 + 实时进度」。
// 后端 execute_scope 没有取消令牌，取消是纯前端「批次边界不再提交」——已交后端的
// 批次不撤回（回收站可逆）。这里用可控的 executeScope mock 验证：
//   ① 取消只在批次边界生效，已交批次照常完成、进度保留；
//   ② 进度 done/total/freedBytes 随批累加；
//   ③ 四种收尾文案：done / cancelled / cancelledNone / allFailed。
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { api } from './api';
import { useCleanupStore, type CleanupSession } from './useCleanupStore';
import { useStore } from './store';
import { setLang } from './i18n';
import type { DryRunPreview } from './cleanup/model';

vi.mock('./api', () => ({
  api: {
    executeScope: vi.fn(),
    scopeSizesBatch: vi.fn(async () => []),
    listUndo: vi.fn(async () => []),
    listCondaEnvs: vi.fn(async () => []),
  },
}));

const executeScope = api.executeScope as unknown as ReturnType<typeof vi.fn>;

// 6 个 scope × 1 命中路径 = 6 个任务；CONCURRENCY=4 → 两批（4 + 2）。
function makePreview(): DryRunPreview {
  return {
    scopeIds: ['s1', 's2', 's3', 's4', 's5', 's6'],
    totalBytes: 0,
    totalFiles: 0,
    samplePaths: [],
    sampleEntries: [],
    truncated: false,
    driveCounts: [],
    daysSnapshot: {},
    wxidSnapshot: null,
    envSnapshot: [],
    matchesPaths: ['C:\\Users\\me\\x'],
    isCondaSnapshot: false,
  };
}

function makeSession(preview: DryRunPreview): CleanupSession {
  return {
    rootRef: null,
    scaffold: {
      id: 'wechat',
      name: '微信',
      risk: 'low',
      disclaimer: '',
      detect: [],
      match: {},
      scopes: [],
    },
    matches: [],
    isConda: false,
    defaultDays: {},
    daysByScope: {},
    wxids: [],
    selectedWxids: new Set<string>(),
    detectedVariants: new Set<string>(),
    visibleScopes: [],
    selectedScopes: new Set<string>(),
    blockedProcesses: null,
    condaEnvs: null,
    selectedEnvs: new Set<string>(),
    scopeSizes: null,
    lastCleanByScope: null,
    scopeLoading: false,
    condaLoading: false,
    running: false,
    previewing: false,
    preview,
    armed: false,
    cancelRequested: false,
    progress: { done: 0, total: 0, freedBytes: 0 },
    msg: null,
    err: null,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  // 固定中文，让收尾文案可断言（默认语言在 node 环境会落到 en）。
  setLang('zh');
  useCleanupStore.setState({ session: null });
  useStore.setState({ reclaimedBytes: 0 });
});

describe('runRealDelete 实时进度与取消', () => {
  it('不取消：全部跑完，进度到 total，文案走 done', async () => {
    executeScope.mockImplementation(async () => [{ source: 'a', bytes_freed: 1000 }]);
    useCleanupStore.setState({ session: makeSession(makePreview()) });

    await useCleanupStore.getState().runRealDelete();
    const s = useCleanupStore.getState().session!;

    expect(s.running).toBe(false);
    expect(s.progress.done).toBe(6);
    expect(s.progress.total).toBe(6);
    expect(s.progress.freedBytes).toBe(6000);
    expect(s.msg).toContain('已清理');
    expect(s.err).toBeNull();
    // 字节按后端实测累计入账，不是预览预估。
    expect(useStore.getState().reclaimedBytes).toBe(6000);
  });

  it('R222：成功后把成功条目的 source 路径同步剪出 root/scanCache（跨页体量同步）', async () => {
    // 预先放一棵小树：C:\Users\me\x 下有一个 delete-me（要被剪）。
    type T = { name: string; path: string; is_dir: boolean; size: number; file_count: number; scaffold_id: string | null; top_extensions: string[]; children: T[] };
    const mkNode = (name: string, path: string, size: number, children: T[] = []): T => ({
      name, path, is_dir: true, size, file_count: 1, scaffold_id: null, top_extensions: [], children,
    });
    const leaf: T = { name: 'delete-me', path: 'C:\\Users\\me\\x\\delete-me', is_dir: false, size: 500, file_count: 1, scaffold_id: null, top_extensions: [], children: [] };
    const tree = mkNode('C:', 'C:\\', 2000, [mkNode('Users', 'C:\\Users', 1500, [mkNode('me', 'C:\\Users\\me', 1000, [leaf])])]);
    // Node 是结构化类型：直接以 setState 部分对象传入（大小/子节点字段齐全即合法）。
    useStore.setState({ root: tree, scanCache: { 'C:\\': tree } } as unknown as Parameters<typeof useStore.setState>[0]);
    // executeScope 返回 leaf.source——成功条目路径进入剪枝清单。
    executeScope.mockImplementation(async () => [{ source: 'C:\\Users\\me\\x\\delete-me', bytes_freed: 500 }]);
    useCleanupStore.setState({ session: makeSession(makePreview()) });

    await useCleanupStore.getState().runRealDelete();

    const st = useStore.getState();
    // leaf 已被剪：root / scanCache 里都不再有 delete-me 路径。
    expect(JSON.stringify(st.root)).not.toContain('delete-me');
    expect(JSON.stringify(st.scanCache)).not.toContain('delete-me');
    // 「已回收」按实测入账。
    expect(st.reclaimedBytes).toBe(3000); // 6 任务 × 500
  });

  it('取消：已交批次完成、后续批次不再提交，进度停在已清处', async () => {
    const pending: ((v: { source: string; bytes_freed: number }[]) => void)[] = [];
    executeScope.mockImplementation(() => new Promise((res) => pending.push(res)));
    useCleanupStore.setState({ session: makeSession(makePreview()) });

    // 同步跑到第一批的 await：此刻 pending 恰为第一批的 4 个任务。
    const run = useCleanupStore.getState().runRealDelete();
    pending.forEach((res) => res([{ source: 'a', bytes_freed: 1000 }]));
    // 在批次边界请求取消——第二批（2 个）不会被提交。
    useCleanupStore.getState().cancelCleanup();
    await run;

    const s = useCleanupStore.getState().session!;
    expect(s.running).toBe(false);
    expect(s.progress.done).toBe(4);
    expect(s.progress.total).toBe(6);
    expect(s.progress.freedBytes).toBe(4000);
    expect(s.msg).toContain('已取消');
    expect(s.msg).toContain('4');
    expect(s.err).toBeNull();
    expect(useStore.getState().reclaimedBytes).toBe(4000);
    // 取消后预览窗关闭。
    expect(s.preview).toBeNull();
  });

  it('取消且尚未清掉任何文件：走 cancelledNone，不记账', async () => {
    const pending: ((v: { source: string; bytes_freed: number }[]) => void)[] = [];
    executeScope.mockImplementation(() => new Promise((res) => pending.push(res)));
    useCleanupStore.setState({ session: makeSession(makePreview()) });

    const run = useCleanupStore.getState().runRealDelete();
    pending.forEach((res) => res([])); // 第一批返回空条目：还没清掉任何文件
    useCleanupStore.getState().cancelCleanup();
    await run;

    const s = useCleanupStore.getState().session!;
    expect(s.progress.done).toBe(4);
    expect(s.progress.total).toBe(6);
    expect(s.msg).toContain('还没清掉任何文件');
    expect(s.err).toBeNull();
    expect(useStore.getState().reclaimedBytes).toBe(0);
  });

  it('全部失败：走 allFailed 错误文案，不记账', async () => {
    executeScope.mockImplementation(async () => {
      throw new Error('boom');
    });
    useCleanupStore.setState({ session: makeSession(makePreview()) });

    await useCleanupStore.getState().runRealDelete();
    const s = useCleanupStore.getState().session!;

    expect(s.running).toBe(false);
    expect(s.progress.done).toBe(6);
    expect(s.progress.total).toBe(6);
    expect(s.err).toContain('全部出错');
    expect(s.msg).toBeNull();
    // 全败不能算成功，绝不累加「已回收」。
    expect(useStore.getState().reclaimedBytes).toBe(0);
  });

  it('cancelCleanup 只置位 cancelRequested，不动进度 / running（UI 靠它显示「取消中」）', () => {
    useCleanupStore.setState({ session: makeSession(makePreview()) });
    useCleanupStore.getState().cancelCleanup();
    const s = useCleanupStore.getState().session!;
    expect(s.cancelRequested).toBe(true);
    // 取消请求本身不清进度、不改 running——真正的收尾由 runRealDelete 在批次边界做。
    expect(s.progress).toEqual({ done: 0, total: 0, freedBytes: 0 });
    expect(s.running).toBe(false);
  });
});
