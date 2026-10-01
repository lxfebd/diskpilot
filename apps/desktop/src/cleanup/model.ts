// 清理域纯逻辑：scope 大小聚合、保留天数持久化、覆盖率/合计计算、变体探测、
// 勾选模式。组件（CleanupBody / CleanupProposalDialog）与 store 共用同一份口径，
// 这里不碰渲染，方便单测直接钉住行为。
//
// 删除语义（先出清单 → 预备 → 真删、失败可见）由 useCleanupStore 的执行链实现；
// 本文件只放无副作用的计算原语（localStorage 读写除外，且全程 try/catch 兜底）。
import { t } from '../i18n';
import type { CondaEnv, Node, Scope } from '../types';

/** 单个 scope 的大小探测结果。`bytes`/`file_count` 受保留期过滤（=真会清的量），
 *  `total_*` 忽略保留期——界面上用后者让用户看到「共 12 GB · 其中 0 GB 超过
 *  保留期」，而不是把「全部在保留期内」误报成「空」。 */
export interface ScopeSize {
  scope_id: string;
  /** Bytes that match scope glob AND are older than the requested retention. */
  bytes: number;
  file_count: number;
  total_bytes: number;
  total_files: number;
}

export const ZERO_SCOPE_SIZE: ScopeSize = {
  scope_id: '',
  bytes: 0,
  file_count: 0,
  total_bytes: 0,
  total_files: 0,
};

/** dry-run 预览的路径清单（第一步：预备态之后弹出，用户确认才真删）。 */
export interface DryRunPreview {
  scopeIds: string[];
  totalBytes: number;
  totalFiles: number;
  /** First N paths that would be deleted. Capped to keep the dialog usable. */
  samplePaths: string[];
  /** True when more paths exist than samplePaths shows. */
  truncated: boolean;
  /** 预览时点各 scope 的应用口径快照：真删必须复用同一份 days/wxid/env，
   *  不能取 runRealDelete 时的最新值——预览窗停留期间用户改设置会以新口径误删。 */
  daysSnapshot: Record<string, number>;
  wxidSnapshot: string[] | null;
  envSnapshot: string[];
  /** 预览时点的命中目录路径数组快照：换盘/重扫会重建 session（matches 打散），
   *  真删绝不能沿「当前 matches」的真删时刻快照跑——应按预览时的路径删。 */
  matchesPaths: string[];
  /** 预览时点是不是 conda 会话（conda 分支真删引 matches[0] 路径，需同步快照）。 */
  isCondaSnapshot: boolean;
}

export const DRY_RUN_SAMPLE_CAP = 80;

const SCOPE_DAYS_STORAGE_KEY = 'diskpilot.scopeDays';

function readScopeDaysAll(): Record<string, Record<string, number>> {
  try {
    const raw = localStorage.getItem(SCOPE_DAYS_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    return {};
  }
}

/** scope.prompt.kind === 'days' 的默认天数表（TOML 里给的值）。 */
export function defaultDaysFor(scopes: Scope[] | undefined): Record<string, number> {
  const out: Record<string, number> = {};
  for (const scope of scopes ?? []) {
    if (scope.prompt?.kind === 'days') out[scope.id] = scope.prompt.default;
  }
  return out;
}

/** 用户改过的天数（与默认不同的才存；全默认则删掉该脚本的条目）。 */
export function scopeDayOverrides(
  days: Record<string, number>,
  defaults: Record<string, number>,
): Record<string, number> {
  const out: Record<string, number> = {};
  for (const [k, v] of Object.entries(days)) {
    if (defaults[k] !== v) out[k] = v;
  }
  return out;
}

/** 默认天数 + 持久化覆盖：跨会话记住用户为每个脚本设的保留期。 */
export function mergeScopeDays(
  scaffoldId: string,
  defaults: Record<string, number>,
): Record<string, number> {
  const persisted = readScopeDaysAll()[scaffoldId] ?? {};
  return { ...defaults, ...persisted };
}

export function persistScopeDays(
  scaffoldId: string,
  days: Record<string, number>,
  defaults: Record<string, number>,
): void {
  const overrides = scopeDayOverrides(days, defaults);
  try {
    const all = readScopeDaysAll();
    if (Object.keys(overrides).length === 0) delete all[scaffoldId];
    else all[scaffoldId] = overrides;
    localStorage.setItem(SCOPE_DAYS_STORAGE_KEY, JSON.stringify(all));
  } catch {
    /* quota / private mode — 保留期记不住不影响本次清理 */
  }
}

/** 微信多账号：扫描命中节点下所有 wxid_* 子目录（升序），供账号筛选 chips 用。 */
export function collectWxids(matches: Node[]): string[] {
  const out = new Set<string>();
  for (const m of matches) {
    for (const c of m.children ?? []) {
      if (c.is_dir && c.name.startsWith('wxid_')) out.add(c.name);
    }
  }
  return [...out].sort();
}

/** 微信目录变体探测：命中的路径决定只显示 4.x / 3.x 的 scope（互斥）。 */
export function detectVariants(matches: Node[]): Set<string> {
  const out = new Set<string>();
  for (const m of matches) {
    const p = m.path.replace(/\\/g, '/').toLowerCase();
    if (p.includes('xwechat_files') || p.includes('tencent/xwechat')) out.add('4.x');
    if (p.includes('wechat files') || p.includes('tencent/wechat')) out.add('3.x');
  }
  return out;
}

/** 多个命中目录的 scope 大小表按 scope_id 累加。 */
export function aggregateScopeSizes(rowsList: ScopeSize[][]): ScopeSize[] {
  const merged = new Map<string, ScopeSize>();
  for (const rows of rowsList) {
    for (const r of rows) {
      const prev = merged.get(r.scope_id);
      if (prev) {
        prev.bytes += r.bytes;
        prev.file_count += r.file_count;
        prev.total_bytes += r.total_bytes;
        prev.total_files += r.total_files;
      } else {
        merged.set(r.scope_id, {
          scope_id: r.scope_id,
          bytes: r.bytes,
          file_count: r.file_count,
          total_bytes: r.total_bytes,
          total_files: r.total_files,
        });
      }
    }
  }
  return [...merged.values()];
}

/** 取单个 scope 的大小行；未测到 / 没有该 scope 时返回 null（区别于「0 字节」）。 */
export function sizeOfScope(sizes: ScopeSize[] | null, id: string): ScopeSize | null {
  if (!sizes) return null;
  return sizes.find((r) => r.scope_id === id) ?? null;
}

export function formatLastActive(ts: number | null): string {
  if (ts === null) return t('cleanup.lastActiveNever');
  const now = Math.floor(Date.now() / 1000);
  const diffSecs = Math.max(0, now - ts);
  const days = Math.floor(diffSecs / 86400);
  if (days < 1) return t('cleanup.lastActiveToday');
  if (days < 30) return t('cleanup.lastActiveDays', { n: days });
  if (days < 365) return t('cleanup.lastActiveMonths', { n: Math.floor(days / 30) });
  return t('cleanup.lastActiveYears', { n: Math.floor(days / 365) });
}

export interface SelectedTotal {
  count: number;
  bytes: number;
}

/** 底部合计口径：conda 算「勾的环境 + 勾的包缓存 scope」，其他算「勾的 scope」。
 *  字节数为 0 的项不计入，避免「勾了 3 项 · 0 B」。 */
export function computeTotalSelected(input: {
  isConda: boolean;
  selectedScopes: ReadonlySet<string>;
  scopeSizes: ScopeSize[] | null;
  condaEnvs?: CondaEnv[] | null;
  selectedEnvs?: ReadonlySet<string>;
}): SelectedTotal {
  const { isConda, selectedScopes, scopeSizes } = input;
  const bytesOf = (id: string) => sizeOfScope(scopeSizes, id)?.bytes ?? 0;

  if (isConda) {
    let count = 0;
    let bytes = 0;
    for (const e of input.condaEnvs ?? []) {
      if ((input.selectedEnvs ?? new Set()).has(e.name)) {
        count += 1;
        bytes += e.size_bytes;
      }
    }
    // conda 包缓存 scope（tarballs / unused-packages）与 env 分开勾选，但同计合计
    for (const id of ['tarballs', 'unused-packages']) {
      const b = bytesOf(id);
      if (b > 0 && selectedScopes.has(id)) {
        count += 1;
        bytes += b;
      }
    }
    return { count, bytes };
  }

  let count = 0;
  let bytes = 0;
  for (const id of selectedScopes) {
    const b = bytesOf(id);
    if (b > 0) {
      count += 1;
      bytes += b;
    }
  }
  return { count, bytes };
}

/** dry-run 命中目录 → 预估字节：把命中的 env 目录路径对回 listCondaEnvs 元数据
 *  （size_bytes），按**实际命中**累加——不能把勾选但未过期的 env 也全算进去
 *  （envs-stale 只清「未使用」的，勾了 10 个只删 3 个时按 10 个算会虚高）。
 *  tarballs/unused-packages 走 scopeSizes 缓存（与普通脚本分支同源）。
 *  返回 { bytes, count }：字节与 dry-run 真实命中对齐，预览口径才与普通分支一致。 */
export function condaDryBytes(
  hitPaths: string[],
  envs: CondaEnv[] | null,
  scopeSizes: ScopeSize[] | null,
  selectedScopes: ReadonlySet<string>,
): { bytes: number; count: number } {
  const envBytes = (p: string) => {
    const norm = p.replace(/[\\/]+$/, '').toLowerCase();
    const e = (envs ?? []).find((x) => x.path.replace(/[\\/]+$/, '').toLowerCase() === norm);
    return e?.size_bytes ?? 0;
  };
  let bytes = 0;
  let count = 0;
  for (const p of hitPaths) {
    const b = envBytes(p);
    if (b > 0) {
      bytes += b;
      count += 1;
    }
  }
  for (const id of ['tarballs', 'unused-packages']) {
    const b = sizeOfScope(scopeSizes, id)?.bytes ?? 0;
    if (b > 0 && selectedScopes.has(id)) {
      bytes += b;
      count += 1;
    }
  }
  return { bytes, count };
}

export interface CoverageBreakdown {
  folderTotal: number;
  inScope: number;
  outsideScope: number;
}

/** 覆盖说明：解释「文件夹 13 GB 但 scope 只占 4 GB」——差额是红线区（聊天记录 /
 *  收藏 / 账号 / 加密物料），任何 scope 都不许命中。conda 没有磁盘根，不适用。 */
export function computeCoverage(
  isConda: boolean,
  matches: Node[],
  scopeSizes: ScopeSize[] | null,
): CoverageBreakdown | null {
  if (isConda) return null;
  const folderTotal = matches.reduce((s, m) => s + m.size, 0);
  if (folderTotal === 0) return null;
  const inScope = (scopeSizes ?? []).reduce((s, r) => s + r.total_bytes, 0);
  return { folderTotal, inScope, outsideScope: Math.max(0, folderTotal - inScope) };
}

// ── 勾选模式（AI 提案工具条 / scope 分组头共用）─────────────────────

export type SelectionMode = 'all' | 'safe' | 'none';

/**
 * 勾选模式的纯计算：`none` 恒返回空集；`all` 返回全部合格项的键；
 * `safe` 在合格项上再叠一道 isSafe 过滤。
 *
 * 返回值是「本次要勾上的键」——调用方自己决定并入现选（全选类按钮）还是
 * 按组摘除（分组「全不选」），因为两处语义不同：提案工具条对 safe 是
 * 增量并入（不动用户已勾的谨慎项），scope 分组是全组替换。
 */
export function applySelectionMode<T>(
  mode: SelectionMode,
  items: readonly T[],
  isEligible: (item: T) => boolean,
  keyOf: (item: T) => string,
  isSafe: (item: T) => boolean = () => true,
): Set<string> {
  if (mode === 'none') return new Set<string>();
  const out = new Set<string>();
  for (const item of items) {
    if (!isEligible(item)) continue;
    if (mode === 'safe' && !isSafe(item)) continue;
    out.add(keyOf(item));
  }
  return out;
}
