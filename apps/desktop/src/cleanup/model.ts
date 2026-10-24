// 清理域纯逻辑：scope 大小聚合、保留天数持久化、覆盖率/合计计算、变体探测、
// 勾选模式。组件（CleanupBody / CleanupProposalDialog）与 store 共用同一份口径，
// 这里不碰渲染，方便单测直接钉住行为。
//
// 删除语义（先出清单 → 预备 → 真删、失败可见）由 useCleanupStore 的执行链实现；
// 本文件只放无副作用的计算原语（localStorage 读写除外，且全程 try/catch 兜底）。
import { t } from '../i18n';
import { pathDriveLetter } from '../diskHealth';
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

/** dry-run 预览的路径清单（第一步：预备态之后弹出，用户确认才真删）。 */
export interface DryRunPreview {
  scopeIds: string[];
  totalBytes: number;
  totalFiles: number;
  /** First N paths that would be deleted. Capped to keep the dialog usable. */
  samplePaths: string[];
  /** R131：与 samplePaths 同序的「路径 + 实际释放字节」条目（供预览窗
   *  行尾展示单文件大小；bytes_freed 缺失为 null，展示省略）。 */
  sampleEntries: { source: string; bytes_freed?: number | null }[];
  /** True when more paths exist than samplePaths shows. */
  truncated: boolean;
  /** 这批路径分布在哪些盘上（按文件数降序）。整批口径，不受 samplePaths 截断影响——
   *  预览窗只列前 80 条，用户却需要知道「这次会动到 C 和 F 两块盘」。 */
  driveCounts: DriveCount[];
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

/** R130：dry-run 预览路径采样——全量命中按「实际释放字节」降序后取前 cap 条。
 *  大文件优先展示（用户一眼看到占最多空间的项）；bytes_freed 缺失（后端
 *  未统计）的沉底且保持相对顺序。纯函数：不改入参。返回路径数组（保持
 *  与 DryRunPreview.samplePaths 同构）。 */
export function samplePathsByBytes(
  entries: readonly { source: string; bytes_freed?: number | null }[],
  cap: number = DRY_RUN_SAMPLE_CAP,
): string[] {
  return sampleEntriesByBytes(entries, cap).map((e) => e.source);
}

/** R131：与 samplePathsByBytes 同排序规则的「条目」版本——预览窗行尾要
 *  显示单文件大小，路径数组不够。字节降序、缺失沉底保相对序、不改入参。 */
export function sampleEntriesByBytes(
  entries: readonly { source: string; bytes_freed?: number | null }[],
  cap: number = DRY_RUN_SAMPLE_CAP,
): { source: string; bytes_freed?: number | null }[] {
  return [...entries]
    .sort((a, b) => {
      const ab = a.bytes_freed ?? null;
      const bb = b.bytes_freed ?? null;
      if (ab == null && bb == null) return 0;
      if (ab == null) return 1;
      if (bb == null) return -1;
      return bb - ab;
    })
    .slice(0, cap);
}

/** 一次清理会动到哪几块物理盘：盘符 + 该盘上的文件数。 */
export interface DriveCount {
  letter: string;
  files: number;
}

/** 把一条路径计入它所在的盘；非盘符路径（UNC / 相对）不计——没有本地物理盘。 */
export function tallyDrive(counts: Map<string, number>, path: string): void {
  const letter = pathDriveLetter(path);
  if (letter) counts.set(letter, (counts.get(letter) ?? 0) + 1);
}

/** 计数表 → 展示顺序：文件数多的盘在前，同数按字母升序。 */
export function driveBreakdown(counts: Map<string, number>): DriveCount[] {
  return [...counts.entries()]
    .filter(([letter]) => /^[A-Z]$/.test(letter))
    .map(([letter, files]) => ({ letter, files }))
    .sort((a, b) => b.files - a.files || a.letter.localeCompare(b.letter));
}

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

/** R251：保留天数输入 sanitize——负值 → 0（立即全清语义），超过 3650 天
 *  （10 年）→ 3650。拒绝脏值：超长保留期会污染持久化覆盖表（每次会话
 *  默认被带偏），且「保留近 3650 天」算出的可清范围几乎为空（误导没得清）。
 *  纯函数，输入非有限数按 0。 */
export function sanitizeRetentionDays(days: number): number {
  const raw = Number.isFinite(days) ? Math.floor(days) : 0;
  if (raw < 0) return 0;
  if (raw > 3650) return 3650;
  return raw;
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
    // R210：engine-caches（unity/unreal/godot）与 teams（1.x/2.x）的 scope
    // 全部带 variant——这些变体不识别就全部过滤掉，勾选行一条不渲染
    // （与 R206 隐私分组同根因：有 scope 却落不进任何渲染分支）。
    if (p.includes('/unity/') || p.includes('/unityhub')) out.add('unity');
    if (p.includes('unrealengine')) out.add('unreal');
    if (p.includes('/godot')) out.add('godot');
    if (p.includes('msteams_8wekyb3d8bbwe')) out.add('2.x');
    else if (p.includes('microsoft/teams')) out.add('1.x');
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

/** R174：可清理占比（0–1）——scope 行的三个数字（总/可清/保留）要读一遍才能
 *  判断「这项能清掉几成」；百分比条让这个比例一眼可见。
 *  纯函数：total 非正 → null（没有分母就不给比例，不编造）；结果钳到 [0,1]
 *  （可清 > 总 的脏数据不画出超长条）。 */
export function cleanRatio(bytes: number, totalBytes: number): number | null {
  if (!Number.isFinite(bytes) || !Number.isFinite(totalBytes) || totalBytes <= 0) return null;
  const r = bytes / totalBytes;
  if (!Number.isFinite(r)) return null;
  return Math.min(1, Math.max(0, r));
}

/** R156：按目录聚合「上次清理时刻」——台账里每条 undo 的 source 落在哪个
 *  扫描命中目录下，就更新该目录的最新清理时间（秒）。
 *  只按目录前缀匹配（scope glob 的展开只有后端能做，前端不猜）——所以语义是
 *  「这个目录上次被清是什么时候」，对用户仍是有用的决策参照。
 *  纯函数：大小写不敏感、路径分隔符统一；返回 Record<目录路径, 最新 ts 秒>。 */
export function lastCleanByPath(
  entries: readonly { timestamp: string; source: string }[],
  matchPaths: readonly string[],
): Record<string, number> {
  const norm = (p: string) => p.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
  const roots = matchPaths.map((p) => ({ raw: p, n: norm(p) }));
  const out: Record<string, number> = {};
  for (const e of entries) {
    const ts = Math.floor(Date.parse(e.timestamp) / 1000);
    if (!Number.isFinite(ts) || ts <= 0) continue;
    const src = norm(e.source);
    // 取最长匹配的根（嵌套目录时更具体的那个才是它的归属）。
    let best: { raw: string; n: string } | null = null;
    for (const r of roots) {
      if (src === r.n || src.startsWith(r.n + '\\')) {
        if (!best || r.n.length > best.n.length) best = r;
      }
    }
    if (!best) continue;
    if (out[best.raw] === undefined || ts > out[best.raw]) out[best.raw] = ts;
  }
  return out;
}

/** R157：命中目录的覆盖统计——同一 scaffold 常有多个命中目录（多账号 /
 *  多个缓存位置），只报「最新一次清理」会掩盖「只清过其中一个」。
 *  纯函数：cleaned = 台账里被清过的目录数，total = 命中目录总数，
 *  latestTs = 最新清理时刻（无 → null）。 */
export function cleanedHitStats(
  byPath: Record<string, number> | null,
  matchPaths: readonly string[],
): { cleaned: number; total: number; latestTs: number | null } {
  const total = matchPaths.length;
  if (!byPath) return { cleaned: 0, total, latestTs: null };
  let cleaned = 0;
  let latestTs: number | null = null;
  for (const p of matchPaths) {
    const ts = byPath[p];
    if (ts === undefined) continue;
    cleaned += 1;
    if (latestTs === null || ts > latestTs) latestTs = ts;
  }
  return { cleaned, total, latestTs };
}

/** R244：单个目录的「上次清理时刻」——台账里匹配该目录（或其下文件）的
 *  最新时间戳（秒）；无匹配返回 null。总览预览弹窗/脚本卡共用。 */
export function latestCleanTs(
  entries: readonly { timestamp: string; source: string }[],
  matchPath: string,
): number | null {
  const by = lastCleanByPath(entries, [matchPath]);
  const v = Object.values(by)[0];
  return v ?? null;
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

/**
 * R177：清理分类组排序——同组 scope 按「可清大小」降序；大小相同维持原目录序
 * （依赖引擎稳定排序），sizeOf 取不到视为 0。让 cache/backup/pkgs 等原本按目录
 * 顺序展示的组与 media 组口径对齐（用户按体量挑大的先看），且不改入参。
 */
export function sortScopesByBytes<T>(
  scopes: readonly T[],
  sizeOf: (item: T) => number,
): T[] {
  return [...scopes].sort((a, b) => sizeOf(b) - sizeOf(a));
}
