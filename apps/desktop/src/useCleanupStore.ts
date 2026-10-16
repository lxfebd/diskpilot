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
  aggregateScopeSizes,
  collectWxids,
  computeCoverage,
  computeTotalSelected,
  condaDryBytes,
  defaultDaysFor,
  detectVariants,
  driveBreakdown,
  mergeScopeDays,
  persistScopeDays,
  lastCleanByPath,
  sampleEntriesByBytes,
  tallyDrive,
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
  /** R156：各类目录「上次清理时刻」（scopeId → 最新 ts 秒）；null = 还没读到
   *  台账（失败与空都走这个分支，界面不显示而非编造）。 */
  lastCleanByScope: Record<string, number> | null;
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
  /** R178：执行中被用户取消——前端在批次边界拦截后续提交；已交后端的批次不撤回
   * （清进回收站可逆），所以取消是「不再继续」而非「回滚已删」。 */
  cancelRequested: boolean;
  /** R178：真删实时进度：已完成 scope×path 任务数 / 总任务数 / 已释放字节（后端实测）。 */
  progress: { done: number; total: number; freedBytes: number };
  msg: string | null;
  err: string | null;
}

interface CleanupStoreState {
  session: CleanupSession | null;
  select: (scaffoldId?: string, opts?: { force?: boolean }) => void;
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
  /** R178：请求取消正在进行的真删（批次边界生效）。 */
  cancelCleanup: () => void;
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

/** 重测当前会话的每个 scope 大小。debounceMs>0 时防抖（保留期/账号筛选变了重测），否则立即（手动刷新余量/真删后）。 */
function fetchScopeSizes(debounceMs: number): void {
  const cur = getCur();
  if (!cur || cur.matches.length === 0 || (cur.scaffold.scopes ?? []).length === 0) return;
  clearSizeTimer();
  const seq = ++sizeSeq;
  const run = () => {
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
        if (seq !== sizeSeq) return;
        patchInSession(() => ({
          scopeSizes: aggregateScopeSizes(batch.map((b) => b.sizes)),
          scopeLoading: false,
        }));
        // R156：顺带拉一次台账，算出各扫描目录的「上次清理时刻」——让 scope
        // 行在用户勾选前就能看到「这个目录刚清过 / 很久没清」。读失败保持
        // null（界面不显示），不影响大小探测结果。
        void api
          .listUndo(200)
          .then((list) => {
            if (seq !== sizeSeq) return;
            const cur2 = getCur();
            if (!cur2) return;
            patchInSession(() => ({
              lastCleanByScope: lastCleanByPath(list, cur2.matches.map((m) => m.path)),
            }));
          })
          .catch(() => {
            if (seq !== sizeSeq) return;
            patchInSession(() => ({ lastCleanByScope: null }));
          });
      })
      .catch((e) => {
        if (seq !== sizeSeq) return;
        patchInSession(() => ({
          scopeLoading: false,
          err: t('cleanup.errScopeSizes', { msg: String(e) }),
        }));
      });
  };
  if (debounceMs > 0) {
    sizeTimer = window.setTimeout(run, debounceMs);
  } else {
    run();
  }
}

/** 保留期 / 账号筛选变了 → 300ms 防抖后重测大小（避免逐字输入打爆后端）。 */
function scheduleSizeFetch() {
  fetchScopeSizes(300);
}

/** 手动重测（真删成功后刷新余量、conda 清完环境后）。 */
function fetchSizesNow(): void {
  fetchScopeSizes(0);
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

/** R175：全盘可清总量——所有可见 scope 的可清字节之和（未测到按 0）。
 *  行动条据此在「已选 X」旁补「占可清总量 N%」，让用户知道选择覆盖了多少；
 *  总量为 0 时返回 0（调用方不显示比例，避免除零）。纯函数。 */
export function computeAllCleanableBytes(s: CleanupSession): number {
  let sum = 0;
  for (const sc of s.visibleScopes) {
    sum += scopeBytes(s, sc.id);
  }
  return sum;
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
    lastCleanByScope: null,
    // 只有真的会去探测才亮「扫描中」——matches/scopes 为空时 scheduleSizeFetch
    // 直接返回，初值若为 true 就永远转圈了。
    scopeLoading: matches.length > 0 && (scaffold.scopes ?? []).length > 0,
    condaLoading: false,
    running: false,
    previewing: false,
    preview: null,
    armed: false,
    cancelRequested: false,
    progress: { done: 0, total: 0, freedBytes: 0 },
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

  // 第一步：只探测不落盘。单 scope 探测失败不崩整批（console.warn + 失败计数），
  // 但「一个都没探到」时必须区分「真没有文件」（errPreviewEmpty）和
  // 「探测失败」（errPreviewProbeFailed）——读失败 ≠ 空。
  runDryRun: async () => {
    const s0 = getCur();
    if (!s0) return;
    patchInSession(() => ({ previewing: true, msg: null, err: null }));
    try {
      const s = getCur();
      if (!s) return;
      // R130：预览采样路径（全量命中按字节降序取前 cap 条后填充；两分支共用）。
      let samplePaths: string[] = [];
      // R131：与 samplePaths 同序的路径+字节条目（预览窗行尾显示单文件大小）。
      let sampleEntries: { source: string; bytes_freed?: number | null }[] = [];
      // 整批路径按盘计数：预览窗只列前 80 条，但「会动到哪几块盘」必须按全量算。
      const driveTally = new Map<string, number>();
      let totalBytes = 0;
      let totalFiles = 0;
      let scopeIds: string[] = [];
      // 探测失败计数：单 scope 失败不该让整批崩掉，但「全都没探到」必须和
      // 「真没有文件」区分——失败是失败，不是空（读失败 ≠ 空）。
      let probeFailures = 0;

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
        const tasks: Promise<{ ok: boolean; entries: { source: string; bytes_freed?: number | null }[] }>[] = [];
        const hitPaths: string[] = [];
        // R130：全量命中的路径+字节（不在此截断）；预览采样在收尾统一按
        // 字节降序取前 cap 条（samplePathsByBytes），大文件优先展示。
        const allHits: { source: string; bytes_freed?: number | null }[] = [];
        for (const scopeId of scopeIds) {
          if (scopeId === 'envs-stale') {
            tasks.push(
              api
                .executeScope(s.scaffold.id, scopeId, s.matches[0].path, true, {
                  envFilter: [...s.selectedEnvs],
                })
                .then(
                  (entries) => ({ ok: true, entries }),
                  (e) => {
                    console.warn(`[diskpilot] dry-run ${s.scaffold.id}/${scopeId} failed:`, e);
                    return { ok: false, entries: [] as { source: string }[] };
                  },
                ),
            );
          } else {
            const days = s.daysByScope[scopeId];
            for (const m of s.matches) {
              tasks.push(
                api
                  .executeScope(s.scaffold.id, scopeId, m.path, true, { olderThanDays: days })
                  .then(
                    (entries) => ({ ok: true, entries }),
                    (e) => {
                      console.warn(
                        `[diskpilot] dry-run ${s.scaffold.id}/${scopeId} on ${m.path} failed:`,
                        e,
                      );
                      return { ok: false, entries: [] as { source: string }[] };
                    },
                  ),
              );
            }
          }
        }
        const lists = await Promise.all(tasks);
        for (const list of lists) {
          if (!list.ok) probeFailures += 1;
          for (const e of list.entries) {
            totalFiles += 1;
            hitPaths.push(e.source);
            allHits.push({ source: e.source, bytes_freed: e.bytes_freed });
            tallyDrive(driveTally, e.source);
          }
        }
        sampleEntries = sampleEntriesByBytes(allHits);
        samplePaths = sampleEntries.map((e) => e.source);
        // conda 字节走 condaDryBytes：只累加「后端 dry-run 实际命中的 env 目录」
        // 的 size_bytes + 勾选的包缓存 scope（tarballs/unused-packages）——不能
        // 把所有勾选 env 全算进去（envs-stale 只清未使用的，勾了 10 删 3 按 10
        // 算会虚高）；与普通分支「按 scope 实际命中总量」口径一致。
        totalBytes = condaDryBytes(hitPaths, s.condaEnvs, s.scopeSizes, s.selectedScopes).bytes;
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
        const tasks: Promise<{ ok: boolean; entries: { source: string; bytes_freed?: number | null }[] }>[] = [];
        // R130：全量命中的路径+字节；预览采样收尾统一按字节降序取前 cap 条。
        const allHits: { source: string; bytes_freed?: number | null }[] = [];
        for (const scopeId of scopeIds) {
          const days = s.daysByScope[scopeId];
          for (const m of s.matches) {
            tasks.push(
              api
                .executeScope(s.scaffold.id, scopeId, m.path, true, {
                  olderThanDays: days,
                  wxidFilter,
                })
                .then(
                  (entries) => ({ ok: true, entries }),
                  (e) => {
                    console.warn(
                      `[diskpilot] dry-run ${s.scaffold.id}/${scopeId} on ${m.path} failed:`,
                      e,
                    );
                    return { ok: false, entries: [] as { source: string }[] };
                  },
                ),
            );
          }
        }
        const lists = await Promise.all(tasks);
        for (const list of lists) {
          if (!list.ok) probeFailures += 1;
          for (const e of list.entries) {
            totalFiles += 1;
            allHits.push({ source: e.source, bytes_freed: e.bytes_freed });
            tallyDrive(driveTally, e.source);
          }
        }
        sampleEntries = sampleEntriesByBytes(allHits);
        samplePaths = sampleEntries.map((e) => e.source);
      }

      if (totalFiles === 0) {
        // 一个都没探到：若存在探测失败，那是「读失败」而非「真空」。
        patchInSession(() => ({
          err: probeFailures > 0 ? t('cleanup.errPreviewProbeFailed') : t('cleanup.errPreviewEmpty'),
        }));
        return;
      }
      // 快照当前口径：真删复用这份，预览窗停留期间改 days/wxid/env 不会影响。
      const daysSnapshot = { ...s.daysByScope };
      const wxidSnapshot =
        s.wxids.length > 0 && s.selectedWxids.size < s.wxids.length
          ? [...s.selectedWxids]
          : null;
      const envSnapshot = [...s.selectedEnvs];
      patchInSession(() => ({
        preview: {
          scopeIds,
          totalBytes,
          totalFiles,
          samplePaths,
          sampleEntries,
          truncated: totalFiles > samplePaths.length,
          driveCounts: driveBreakdown(driveTally),
          daysSnapshot,
          wxidSnapshot,
          envSnapshot,
          matchesPaths: s.matches.map((m) => m.path),
          isCondaSnapshot: s.isConda,
        },
      }));
    } catch (e) {
      patchInSession(() => ({ err: t('cleanup.errPreview', { msg: String(e) }) }));
    } finally {
      patchInSession(() => ({ previewing: false }));
    }
  },

  // 第二步：真删。失败逐项收集，结束时按「全败 / 部分败 / 全成」三种口径出文案。
  // R178：改一次性 Promise.all 为分批（CONCURRENCY 并发）执行，每批结束后在批次边界
  // 检查 cancelRequested——已交后端的批次不撤回（清进回收站可逆），取消只「不再继续」。
  // 进度（done/total/freedBytes）随批实时写回，供界面显示进度条与已释放字节。
  runRealDelete: async () => {
    const s0 = getCur();
    if (!s0 || s0.preview === null) return;
    patchInSession(() => ({
      running: true,
      cancelRequested: false,
      progress: { done: 0, total: 0, freedBytes: 0 },
      msg: null,
      err: null,
    }));
    try {
      const s = getCur();
      const preview = s0.preview;
      if (!s || !preview) return;
      let totalEntries = 0;
      let freedBytes = 0;
      const failures: string[] = [];
      // 每个任务封装成 () => Promise<{ entries: UndoEntry[] }>，错误在内部吞掉并记进 failures，
      // 不因单任务失败中断整批（与旧 Promise.all 的语义一致，只是改在批边界可中断）。
      const makeTask = (scopeId: string, path: string, opts: ExecuteScopeOpts) => () =>
        api
          .executeScope(s.scaffold.id, scopeId, path, false, { ...opts, confirmed: true })
          .then(
            (entries) => ({ entries }),
            (e) => {
              // 诊断串（scope @ 路径: 后端原文），全 ASCII 分隔符便于拼进文案表。
              failures.push(`${scopeId} @ ${path}: ${String(e)}`);
              console.warn(
                `[diskpilot] executeScope ${s.scaffold.id}/${scopeId} on ${path} failed:`,
                e,
              );
              return { entries: [] as { source: string; bytes_freed?: number | null }[] };
            },
          );

      // 先把所有任务描述收集成数组，再分批跑——这样取消时直接不再提交后续批次。
      const taskList: (() => Promise<{ entries: { source: string; bytes_freed?: number | null }[] }>)[] = [];
      if (preview.isCondaSnapshot) {
        // 用预览时点快照的 env 列表 + matches 路径，不用 runRealDelete 时的最新勾选/换盘。
        const envFilterArg = preview.envSnapshot;
        const paths = preview.matchesPaths.length > 0 ? preview.matchesPaths : [s.matches[0]?.path];
        const rootPath = preview.matchesPaths[0] ?? s.matches[0]?.path;
        if (rootPath) {
          for (const scopeId of preview.scopeIds) {
            if (scopeId === 'envs-stale') {
              taskList.push(makeTask(scopeId, rootPath, { envFilter: envFilterArg }));
            } else {
              const days = preview.daysSnapshot[scopeId];
              for (const mPath of paths) {
                taskList.push(makeTask(scopeId, mPath, { olderThanDays: days }));
              }
            }
          }
        }
      } else {
        // 快照口径：预览时勾选的账号过滤 + 命中路径，预览窗停留期间改勾选/换盘不影响本次真删。
        const wxidFilter = preview.wxidSnapshot ?? undefined;
        const paths = preview.matchesPaths.length > 0 ? preview.matchesPaths : s.matches.map((m) => m.path);
        for (const scopeId of preview.scopeIds) {
          const days = preview.daysSnapshot[scopeId];
          for (const mPath of paths) {
            taskList.push(makeTask(scopeId, mPath, { olderThanDays: days, wxidFilter }));
          }
        }
      }

      // 分批执行 + 批次边界取消检查 + 实时进度。
      const CONCURRENCY = 4;
      let done = 0;
      // R222：成功回收条目的 source 路径收集——收尾一次性剪 root/scanCache，
      // 总览/工作台跨页立即反映真实剩余体量（与 recyclePaths 同一批语义）。
      const recycledPaths: string[] = [];
      patchInSession(() => ({ progress: { done: 0, total: taskList.length, freedBytes: 0 } }));
      for (let i = 0; i < taskList.length; i += CONCURRENCY) {
        if (getCur()?.cancelRequested) break;
        const slice = taskList.slice(i, i + CONCURRENCY);
        const results = await Promise.all(slice.map((fn) => fn()));
        for (const r of results) {
          totalEntries += r.entries.length;
          for (const e of r.entries) {
            freedBytes += e.bytes_freed ?? 0;
            if (e.source) recycledPaths.push(e.source);
          }
          done += 1;
        }
        patchInSession(() => ({ progress: { done, total: taskList.length, freedBytes } }));
      }
      const cancelled = !!getCur()?.cancelRequested;

      // 收尾：conda 刷新环境列表 / 非 conda 重测大小并清勾选。与旧逻辑一致，只是现在在批循环后。
      if (preview.isCondaSnapshot) {
        const rootPath = preview.matchesPaths[0] ?? s.matches[0]?.path;
        if (rootPath) {
          const refreshed = await api.listCondaEnvs(rootPath).catch(() => [] as CondaEnv[]);
          patchInSession(() => ({
            condaEnvs: refreshed,
            selectedEnvs: new Set(refreshed.filter((e) => e.default_checked).map((e) => e.name)),
          }));
          if (preview.scopeIds.some((id) => id !== 'envs-stale')) fetchSizesNow();
        }
      } else {
        fetchSizesNow();
        patchInSession(() => ({ selectedScopes: new Set() }));
      }

      // 只有真清掉东西才累加「已回收」。字节数用后端实测（bytes_freed 累计），
      // 而非预览预估值——后端已返 actual，不再标「预估」。全败时不记账（失败不能算成功）。
      const allFailed = failures.length > 0 && totalEntries === 0;
      // R222：成功回收跨页同步——只剪真删掉的（全败/全取消空路径不剪）。
      if (!allFailed && recycledPaths.length > 0) {
        useStore.getState().pruneRecycled(recycledPaths);
      }
      if (!allFailed) useStore.getState().addReclaimed(freedBytes);
      if (cancelled) {
        // 中途取消：已交批次不可逆（回收站可达），按「清了一部分」如实报。
        patchInSession(() => ({
          msg:
            totalEntries > 0
              ? t('cleanup.cancelled', { n: totalEntries, size: formatBytes(freedBytes) }) +
                (failures.length > 0
                  ? t('cleanup.partialFail', { n: failures.length, msg: failures[failures.length - 1] })
                  : '')
              : t('cleanup.cancelledNone'),
          preview: null,
        }));
      } else if (allFailed) {
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
            t('cleanup.done', { n: totalEntries, size: formatBytes(freedBytes) }) +
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
    patchInSession(() => ({ preview: null, armed: false, cancelRequested: false }));
  },

  // R178：执行中请求取消。批次边界生效——已交后端的批次不撤回（回收站可逆），
  // 只是不再提交后续批次，进度停在当前值。
  cancelCleanup: () => {
    patchInSession(() => ({ cancelRequested: true }));
  },
}));
