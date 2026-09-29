// 清理页会话状态 + 执行链：把原来 CleanupModal 里的「保留期 → 大小探测 →
// 勾选 → dry-run 预览 → 预备 → 真删」全搬到这里，跨组件共享（CleanupBody /
// CleanupPage / Studio 入口共用）。
//
// 铁律（AGENTS）①先出清单 → 预备 → 真删，任何路径都不许跳过预览窗；
// ②默认回收站，字节数标注「（预估）」；③失败必须看得见——全部失败走 err、
// 部分失败在成功文案后追加「N 项失败」，绝不把失败报成成功。
import { create } from 'zustand';
import { api } from './api';
import type { ExecuteScopeOpts } from './api';
import { formatBytes } from './format';
import { t } from './i18n';
import {
  DRY_RUN_SAMPLE_CAP,
  aggregateScopeSizes,
  collectWxids,
  computeCoverage,
  computeTotalSelected,
  defaultDaysFor,
  detectVariants,
  mergeScopeDays,
  persistScopeDays,
  type CoverageBreakdown,
  type DryRunPreview,
  type ScopeSize,
  type SelectedTotal,
} from './cleanup/model';
import { collectScaffoldCards } from './cleanup/matches';
import type { CondaEnv, Node, Scaffold } from './types';
import { useStore } from './store';

export interface CleanupSession {
  /** 建会话时的磁盘树引用；换盘/重扫后由 CleanupPage 的 effect 重建。 */
  rootRef: Node | null;
  scaffold: Scaffold;
  matches: Node[];
  /** scaffold.id === 'conda'：conda 走环境选择器 + 包缓存 scope，其余走 scope 组。 */
  isConda: boolean;
  defaultDays: Record<string, number>;
  /** 用户改过的保留期（persist 到 localStorage，跨会话保留）。 */
  daysByScope: Record<string, number>;
  wxids: string[];
  selectedWxids: Set<string>;
  detectedVariants: Set<string>;
  visibleScopes: Scaffold['scopes'];
  selectedScopes: Set<string>;
  condaEnvs: CondaEnv[] | null;
  selectedEnvs: Set<string>;
  scopeSizes: ScopeSize[] | null;
  /** 只有「大小探测在飞」才亮这个；conda 环境列表加载走独立的 condaLoading。 */
  scopeLoading: boolean;
  condaLoading: boolean;
  /** preflight 只读结果：required_stopped_processes 里还在跑的进程（提示先退出）。 */
  blockedProcesses: string[] | null;
  running: boolean;
  previewing: boolean;
  preview: DryRunPreview | null;
  /** 预备态：第一次点「清理」后置位，5s 不操作自动回退。 */
  armed: boolean;
  msg: string | null;
  err: string | null;
}

interface CleanupStoreState {
  session: CleanupSession | null;
  select: (scaffoldId?: string, opts?: { force?: boolean }) => void;
  open: (scaffoldId: string, matches: Node[]) => void;
  close: () => void;
  setDays: (scopeId: string, days: number) => void;
  toggleWxid: (wxid: string) => void;
  toggleEnv: (name: string) => void;
  toggleScope: (id: string) => void;
  selectScopes: (ids: string[]) => void;
  clearScopes: (ids: string[]) => void;
  execute: () => Promise<void>;
  runDryRun: () => Promise<void>;
  runRealDelete: () => Promise<void>;
  cancelPreview: () => void;
  refreshSizes: () => void;
}

// ── 模块级定时器（会话只有一份，组件卸载不影响在飞的任务）────────────
let sizeTimer: number | null = null;
let sizeSeq = 0;
let armTimer: number | null = null;

function clearSizeTimer() {
  if (sizeTimer !== null) {
    window.clearTimeout(sizeTimer);
    sizeTimer = null;
  }
}

function clearArmTimer() {
  if (armTimer !== null) {
    window.clearTimeout(armTimer);
    armTimer = null;
  }
}

function clearAllTimers() {
  clearSizeTimer();
  clearArmTimer();
}

function getCur(): CleanupSession | null {
  return useCleanupStore.getState().session;
}

/** 只给 session 打补丁（不改外层），供 async 回调里同步最新状态用。 */
function patchInSession(fn: (s: CleanupSession) => Partial<CleanupSession>): void {
  const cur = getCur();
  if (!cur) return;
  useCleanupStore.setState({ session: { ...cur, ...fn(cur) } });
}

/** 保留期 / 账号筛选变了 → 300ms 防抖后重测大小（避免逐字输入打爆后端）。 */
function scheduleSizeFetch() {
  const cur = getCur();
  if (!cur || cur.matches.length === 0 || (cur.scaffold.scopes ?? []).length === 0) return;
  clearSizeTimer();
  const seq = ++sizeSeq;
  sizeTimer = window.setTimeout(() => {
    sizeTimer = null;
    if (seq !== sizeSeq) return;
    const s = getCur();
    if (!s) return;
    const paths = s.matches.map((m) => m.path);
    const wxidFilter =
      s.wxids.length > 0 && s.selectedWxids.size < s.wxids.length
        ? [...s.selectedWxids]
        : undefined;
    patchInSession(() => ({ scopeLoading: true }));
    api
      .scopeSizesBatch(s.scaffold.id, paths, s.daysByScope, wxidFilter)
      .then((batch) => {
        patchInSession(() => ({
          scopeSizes: aggregateScopeSizes(batch.map((b) => b.sizes)),
          scopeLoading: false,
        }));
      })
      .catch((e) => {
        patchInSession(() => ({
          scopeLoading: false,
          err: t('cleanup.errScopeSizes', { msg: String(e) }),
        }));
      });
  }, 300);
}

/** 手动重测（真删成功后刷新余量、conda 清完环境后）。 */
function fetchSizesNow(): void {
  const cur = getCur();
  if (!cur || cur.matches.length === 0) return;
  clearSizeTimer();
  const seq = ++sizeSeq;
  const s = cur;
  const paths = s.matches.map((m) => m.path);
  const wxidFilter =
    s.wxids.length > 0 && s.selectedWxids.size < s.wxids.length
      ? [...s.selectedWxids]
      : undefined;
  patchInSession(() => ({ scopeLoading: true }));
  api
    .scopeSizesBatch(s.scaffold.id, paths, s.daysByScope, wxidFilter)
    .then((batch) => {
      if (seq !== sizeSeq) return;
      patchInSession(() => ({
        scopeSizes: aggregateScopeSizes(batch.map((b) => b.sizes)),
        scopeLoading: false,
      }));
    })
    .catch((e) => {
      if (seq !== sizeSeq) return;
      patchInSession(() => ({
        scopeLoading: false,
        err: t('cleanup.errScopeSizes', { msg: String(e) }),
      }));
    });
}

/** conda 环境列表：默认勾选 default_checked（stale 90d 推荐）。 */
function loadCondaEnvs(): void {
  const cur = getCur();
  if (!cur || !cur.isConda || cur.matches.length === 0) return;
  const path = cur.matches[0].path;
  patchInSession(() => ({ condaLoading: true }));
  api
    .listCondaEnvs(path)
    .then((envs) => {
      patchInSession(() => ({
        condaEnvs: envs,
        condaLoading: false,
        selectedEnvs: new Set(envs.filter((e) => e.default_checked).map((e) => e.name)),
      }));
    })
    .catch((e) => {
      patchInSession(() => ({
        condaLoading: false,
        err: t('cleanup.errCondaEnvs', { msg: String(e) }),
      }));
    });
}

/** P2b preflight（只读）：查 required_stopped_processes 里还在跑的进程，回填 session。
 *  失败静默——预检只是提示，绝不阻塞清理页。 */
function runPreflight(scaffoldId: string): void {
  api
    .cleanupPreflight(scaffoldId)
    .then((r) => {
      const cur = getCur();
      if (!cur || cur.scaffold.id !== scaffoldId) return; // 会话已切走，丢弃
      patchInSession(() => ({ blockedProcesses: r.running_processes }));
    })
    .catch(() => {});
}

export function scopeBytes(s: CleanupSession, id: string): number {
  return s.scopeSizes?.find((r) => r.scope_id === id)?.bytes ?? 0;
}

export function computeSessionTotal(s: CleanupSession): SelectedTotal {
  return computeTotalSelected({
    isConda: s.isConda,
    selectedScopes: s.selectedScopes,
    scopeSizes: s.scopeSizes,
    condaEnvs: s.condaEnvs,
    selectedEnvs: s.selectedEnvs,
  });
}

export function computeSessionCoverage(s: CleanupSession): CoverageBreakdown | null {
  return computeCoverage(s.isConda, s.matches, s.scopeSizes);
}

function buildSession(
  root: Node | null,
  scaffold: Scaffold,
  matches: Node[],
): CleanupSession {
  const defaultDays = defaultDaysFor(scaffold.scopes);
  const variants = detectVariants(matches);
  const wxids = collectWxids(matches);
  return {
    rootRef: root,
    scaffold,
    matches,
    isConda: scaffold.id === 'conda',
    defaultDays,
    daysByScope: mergeScopeDays(scaffold.id, defaultDays),
    wxids,
    // 默认全选账号：只影响保留期/大小探测的口径，勾选本身才是真删的范围。
    selectedWxids: new Set(wxids),
    detectedVariants: variants,
    visibleScopes: (scaffold.scopes ?? []).filter(
      (sc) => !sc.variant || variants.has(sc.variant),
    ),
    // P2b 推荐预选：scope 自带 recommended_selected 才预勾。只是建议，
    // 用户仍可逐项取消；执行范围永远以勾选为准（铁律不在这里改）。
    selectedScopes: new Set(
      (scaffold.scopes ?? [])
        .filter((sc) => sc.recommended_selected === true)
        .map((sc) => sc.id),
    ),
    blockedProcesses: null,
    condaEnvs: null,
    selectedEnvs: new Set<string>(),
    scopeSizes: null,
    // 只有真的会去探测才亮「扫描中」——matches/scopes 为空时 scheduleSizeFetch
    // 直接返回，初值若为 true 就永远转圈了。
    scopeLoading: matches.length > 0 && (scaffold.scopes ?? []).length > 0,
    condaLoading: false,
    running: false,
    previewing: false,
    preview: null,
    armed: false,
    msg: null,
    err: null,
  };
}

export const useCleanupStore = create<CleanupStoreState>((set) => ({
  session: null,

  // 选择要清理的脚本：scaffoldId 为空时自动挑「第一个检测到的」（cards 已按
  // 命中优先 + 体量降序排好）。正在执行 / 预览时忽略点击，避免把在飞的删除盖掉。
  select: (scaffoldId, opts) => {
    const cur = getCur();
    if (cur && (cur.running || cur.previewing)) return;
    const root = useStore.getState().root;
    const scaffolds = useStore.getState().scaffolds;
    if (scaffolds.length === 0) return;

    const cards = collectScaffoldCards(root, scaffolds);
    const byId = new Map(cards.map((c) => [c.scaffold.id, c]));
    const target = scaffoldId
      ? scaffolds.find((s) => s.id === scaffoldId) ?? cards[0]?.scaffold
      : cards[0]?.scaffold;
    if (!target) return;

    if (!opts?.force && cur && cur.scaffold.id === target.id && cur.rootRef === root) return;

    clearAllTimers();
    sizeSeq += 1;
    const session = buildSession(root, target, byId.get(target.id)?.matches ?? []);
    useCleanupStore.setState({ session });
    if (session.isConda) loadCondaEnvs();
    scheduleSizeFetch();
    runPreflight(target.id);
  },

  // Studio「配置清理…」直接带检测到的目录进来，跳过内部 DFS。
  open: (scaffoldId, matches) => {
    const cur = getCur();
    if (cur && (cur.running || cur.previewing)) return;
    const root = useStore.getState().root;
    const scaffolds = useStore.getState().scaffolds;
    const scaffold =
      scaffolds.find((s) => s.id === scaffoldId) ?? scaffolds.find((s) => s.id === 'conda');
    if (!scaffold) return;
    clearAllTimers();
    sizeSeq += 1;
    const session = buildSession(root, scaffold, matches);
    useCleanupStore.setState({ session });
    if (session.isConda) loadCondaEnvs();
    scheduleSizeFetch();
    runPreflight(scaffold.id);
  },

  close: () => {
    clearAllTimers();
    sizeSeq += 1;
    set({ session: null });
  },

  setDays: (scopeId, days) => {
    const cur = getCur();
    if (!cur) return;
    const v = Number.isFinite(days) && days >= 0 ? Math.floor(days) : 0;
    const next = { ...cur.daysByScope, [scopeId]: v };
    persistScopeDays(cur.scaffold.id, next, cur.defaultDays);
    patchInSession(() => ({ daysByScope: next, msg: null }));
    scheduleSizeFetch();
  },

  toggleWxid: (wxid) => {
    const cur = getCur();
    if (!cur) return;
    const next = new Set(cur.selectedWxids);
    if (next.has(wxid)) next.delete(wxid);
    else next.add(wxid);
    patchInSession(() => ({ selectedWxids: next, msg: null }));
    scheduleSizeFetch();
  },

  toggleEnv: (name) => {
    const cur = getCur();
    if (!cur) return;
    const next = new Set(cur.selectedEnvs);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    patchInSession(() => ({ selectedEnvs: next, msg: null }));
  },

  toggleScope: (id) => {
    const cur = getCur();
    if (!cur) return;
    const next = new Set(cur.selectedScopes);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    patchInSession(() => ({ selectedScopes: next, msg: null }));
  },

  // 分组「全选」：并入（union），只挑 bytes>0 的（勾上空 scope 没有意义）。
  selectScopes: (ids) => {
    const cur = getCur();
    if (!cur) return;
    const next = new Set(cur.selectedScopes);
    for (const id of ids) {
      if (scopeBytes(cur, id) > 0) next.add(id);
    }
    patchInSession(() => ({ selectedScopes: next, msg: null }));
  },

  // 分组「全不选」：只摘本组的，别组保留。
  clearScopes: (ids) => {
    const cur = getCur();
    if (!cur) return;
    const next = new Set(cur.selectedScopes);
    for (const id of ids) next.delete(id);
    patchInSession(() => ({ selectedScopes: next, msg: null }));
  },

  refreshSizes: () => {
    fetchSizesNow();
  },

  // 三段语义：点 1 = 预备（5s 自动回退），点 2 = 第一次跑 dry-run 出预览窗，
  // 预览窗内再点一次 = 预备，再点 = 真删。任何一步都不许跳过预览。
  execute: async () => {
    const cur = getCur();
    if (!cur || cur.running) return;
    if (!cur.armed) {
      patchInSession(() => ({ armed: true, msg: null }));
      clearArmTimer();
      armTimer = window.setTimeout(() => {
        armTimer = null;
        patchInSession(() => ({ armed: false }));
      }, 5000);
      return;
    }
    clearArmTimer();
    patchInSession(() => ({ armed: false }));
    if (cur.preview === null) {
      await useCleanupStore.getState().runDryRun();
      return;
    }
    await useCleanupStore.getState().runRealDelete();
  },

  // 第一步：只探测不落盘。dry-run 失败仅 console.warn（单 scope 失败不该让整批
  // 崩掉），但「一个都没探到」必须报 err（errPreviewEmpty），不弹空窗。
  runDryRun: async () => {
    const s0 = getCur();
    if (!s0) return;
    patchInSession(() => ({ previewing: true, msg: null, err: null }));
    try {
      const s = getCur();
      if (!s) return;
      const samplePaths: string[] = [];
      let totalBytes = 0;
      let totalFiles = 0;
      let scopeIds: string[] = [];

      if (s.isConda) {
        if (s.matches.length === 0) {
          patchInSession(() => ({ err: t('cleanup.errNoCondaRoot') }));
          return;
        }
        scopeIds = [];
        if (s.selectedEnvs.size > 0) scopeIds.push('envs-stale');
        // conda 包缓存 scope：tarballs（pkgs/cache）+ unused-packages（pkgs/*，30 天）
        for (const id of ['tarballs', 'unused-packages']) {
          if (s.selectedScopes.has(id) && scopeBytes(s, id) > 0) scopeIds.push(id);
        }
        if (scopeIds.length === 0) {
          patchInSession(() => ({ err: t('cleanup.errNoCondaItems') }));
          return;
        }
        const tasks: Promise<{ source: string }[]>[] = [];
        for (const scopeId of scopeIds) {
          if (scopeId === 'envs-stale') {
            tasks.push(
              api
                .executeScope(s.scaffold.id, scopeId, s.matches[0].path, true, {
                  envFilter: [...s.selectedEnvs],
                })
                .catch((e) => {
                  console.warn(`[diskpilot] dry-run ${s.scaffold.id}/${scopeId} failed:`, e);
                  return [] as { source: string }[];
                }),
            );
          } else {
            const days = s.daysByScope[scopeId];
            for (const m of s.matches) {
              tasks.push(
                api
                  .executeScope(s.scaffold.id, scopeId, m.path, true, { olderThanDays: days })
                  .catch((e) => {
                    console.warn(
                      `[diskpilot] dry-run ${s.scaffold.id}/${scopeId} on ${m.path} failed:`,
                      e,
                    );
                    return [] as { source: string }[];
                  }),
              );
            }
          }
        }
        const lists = await Promise.all(tasks);
        for (const list of lists) {
          for (const e of list) {
            totalFiles += 1;
            if (samplePaths.length < DRY_RUN_SAMPLE_CAP) samplePaths.push(e.source);
          }
        }
        totalBytes = computeSessionTotal(s).bytes;
      } else {
        scopeIds = [...s.selectedScopes].filter((id) => scopeBytes(s, id) > 0);
        if (scopeIds.length === 0) {
          patchInSession(() => ({ err: t('cleanup.errNoScopes') }));
          return;
        }
        totalBytes = scopeIds.reduce((sum, id) => sum + scopeBytes(s, id), 0);
        const wxidFilter =
          s.wxids.length > 0 && s.selectedWxids.size < s.wxids.length
            ? [...s.selectedWxids]
            : undefined;
        const tasks: Promise<{ source: string }[]>[] = [];
        for (const scopeId of scopeIds) {
          const days = s.daysByScope[scopeId];
          for (const m of s.matches) {
            tasks.push(
              api
                .executeScope(s.scaffold.id, scopeId, m.path, true, {
                  olderThanDays: days,
                  wxidFilter,
                })
                .catch((e) => {
                  console.warn(
                    `[diskpilot] dry-run ${s.scaffold.id}/${scopeId} on ${m.path} failed:`,
                    e,
                  );
                  return [] as { source: string }[];
                }),
            );
          }
        }
        const lists = await Promise.all(tasks);
        for (const list of lists) {
          for (const e of list) {
            totalFiles += 1;
            if (samplePaths.length < DRY_RUN_SAMPLE_CAP) samplePaths.push(e.source);
          }
        }
      }

      if (totalFiles === 0) {
        patchInSession(() => ({ err: t('cleanup.errPreviewEmpty') }));
        return;
      }
      patchInSession(() => ({
        preview: {
          scopeIds,
          totalBytes,
          totalFiles,
          samplePaths,
          truncated: totalFiles > samplePaths.length,
        },
      }));
    } catch (e) {
      patchInSession(() => ({ err: t('cleanup.errPreview', { msg: String(e) }) }));
    } finally {
      patchInSession(() => ({ previewing: false }));
    }
  },

  // 第二步：真删。失败逐项收集，结束时按「全败 / 部分败 / 全成」三种口径出文案。
  runRealDelete: async () => {
    const s0 = getCur();
    if (!s0 || s0.preview === null) return;
    patchInSession(() => ({ running: true, msg: null, err: null }));
    try {
      const s = getCur();
      const preview = s0.preview;
      if (!s || !preview) return;
      let totalEntries = 0;
      const failures: string[] = [];
      const runReal = (scopeId: string, path: string, opts: ExecuteScopeOpts) =>
        api
          .executeScope(s.scaffold.id, scopeId, path, false, { ...opts, confirmed: true })
          .then(
            (entries) => {
              totalEntries += entries.length;
            },
            (e) => {
              // 诊断串（scope @ 路径: 后端原文），全 ASCII 分隔符便于拼进文案表。
              failures.push(`${scopeId} @ ${path}: ${String(e)}`);
              console.warn(
                `[diskpilot] executeScope ${s.scaffold.id}/${scopeId} on ${path} failed:`,
                e,
              );
            },
          );

      if (s.isConda) {
        const envFilterArg = [...s.selectedEnvs];
        const tasks: Promise<unknown>[] = [];
        for (const scopeId of preview.scopeIds) {
          if (scopeId === 'envs-stale') {
            tasks.push(runReal(scopeId, s.matches[0].path, { envFilter: envFilterArg }));
          } else {
            const days = s.daysByScope[scopeId];
            for (const m of s.matches) {
              tasks.push(runReal(scopeId, m.path, { olderThanDays: days }));
            }
          }
        }
        await Promise.all(tasks);
        const refreshed = await api.listCondaEnvs(s.matches[0].path).catch(() => [] as CondaEnv[]);
        patchInSession(() => ({
          condaEnvs: refreshed,
          selectedEnvs: new Set(refreshed.filter((e) => e.default_checked).map((e) => e.name)),
        }));
        if (preview.scopeIds.some((id) => id !== 'envs-stale')) fetchSizesNow();
      } else {
        const wxidFilter =
          s.wxids.length > 0 && s.selectedWxids.size < s.wxids.length
            ? [...s.selectedWxids]
            : undefined;
        const tasks: Promise<unknown>[] = [];
        for (const scopeId of preview.scopeIds) {
          const days = s.daysByScope[scopeId];
          for (const m of s.matches) {
            tasks.push(runReal(scopeId, m.path, { olderThanDays: days, wxidFilter }));
          }
        }
        await Promise.all(tasks);
        fetchSizesNow();
        patchInSession(() => ({ selectedScopes: new Set() }));
      }

      // 只有真清掉东西才累加「已回收」；字节数标「（预估）」——UndoEntry 不带
      // 每项实际字节，实测口径待后端补字段。全败时不记账（失败不能算成功）。
      const allFailed = failures.length > 0 && totalEntries === 0;
      if (!allFailed) useStore.getState().addReclaimed(preview.totalBytes);
      if (allFailed) {
        patchInSession(() => ({
          err: t('cleanup.allFailed', {
            n: failures.length,
            msg: failures[failures.length - 1],
          }),
          preview: null,
        }));
      } else {
        patchInSession(() => ({
          msg:
            t('cleanup.done', { n: totalEntries, size: formatBytes(preview.totalBytes) }) +
            (failures.length > 0
              ? t('cleanup.partialFail', {
                  n: failures.length,
                  msg: failures[failures.length - 1],
                })
              : ''),
          preview: null,
        }));
      }
    } catch (e) {
      patchInSession(() => ({ err: t('cleanup.errCleanup', { msg: String(e) }) }));
      throw e;
    } finally {
      patchInSession(() => ({ running: false }));
    }
  },

  cancelPreview: () => {
    clearArmTimer();
    patchInSession(() => ({ preview: null, armed: false }));
  },
}));
