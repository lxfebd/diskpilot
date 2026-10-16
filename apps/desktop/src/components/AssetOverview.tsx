import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { HardDrive, Zap, Sparkles, FileSearch, Files, Trash2, Check, Info, RefreshCw, FolderOpen, BarChart3, Package, AlertTriangle, Thermometer, Rocket, ShieldAlert, ShieldCheck, ShieldQuestion, ChevronDown, ChevronUp, File } from 'lucide-react';
import { api, type SpacePoint, type StartupItem, type HwDiskHealth } from '../api';
import { CAT_COLORS } from '../colors';
import { formatBytes, formatBytesTriple, normKey, driveLetter } from '../format';
import { ensureHwDiskQuick } from '../hwCache';
import { pathDriveLetter, riskyDrives, summarizeDiskHealth } from '../diskHealth';
import { useStore, type CleanupProposalItem } from '../store';
import { physicalDrives, physicalUsedBytes, volumeLetterOf, type DriveEntry } from '../drives';
import { DriveStrip } from './DriveStrip';
import { t, useT } from '../i18n';
import { latestCleanTs, formatLastActive } from '../cleanup/model';
import type { Node, Scaffold } from '../types';

// SVG 渐变 id 的唯一序列源：每实例自增，保证跨 remount/多实例不撞。
let trendSeq = 0;
function trendFillSeq(): number {
  trendSeq += 1;
  return trendSeq;
}

interface DriveInfo {
  path: string;
  total_bytes: number;
  used_bytes: number;
  free_bytes: number;
  /** 映射盘（subst）的目标路径，普通盘为空。 */
  mapped_to?: string;
}

type Props = {
  root: Node | null;
  drives: DriveInfo[];
  scaffolds: Scaffold[];
  scanning: boolean;
  onScanDrive: (path: string) => void;
  onScanAll: () => void;
  onRefresh: (path: string) => void;
  /** 跳工作台；R91 后带可选盘路径（趋势行点击定位到该盘，无参调用=仅切页）。 */
  onGoWorkspace: (drivePath?: string) => void;
  onOpenCleanup: (scaffoldId?: string) => void;
  /** 跳到工具页的硬件面板看硬盘健康明细（体检条的硬盘格用）。 */
  onGoHardware?: () => void;
};

function catColor(seed: string): string {
  let h = 0;
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) & 0xffffffff;
  return CAT_COLORS[Math.abs(h) % CAT_COLORS.length];
}

/** 盘使用率百分比（0..100，total 无效返回 0）。 */
export function driveUsagePct(usedBytes: number, totalBytes: number): number {
  if (!(totalBytes > 0)) return 0;
  return (usedBytes / totalBytes) * 100;
}

/** 红盘救援阈值：使用率 >=85% 视为「快满了」（DriveGauge 与救援横幅共用）。 */
export function isDriveCritical(usedBytes: number, totalBytes: number): boolean {
  return driveUsagePct(usedBytes, totalBytes) >= 85;
}

/** 盘容量是否读到了（total 无效 = 没读到，不是「0 字节」）。 */
export function capacityKnown(totalBytes: number): boolean {
  return totalBytes > 0;
}

// 单盘使用率环形图：中心显示百分比；容量没读到时显示「未读到」而不是 0%。
function DriveGauge({ usedBytes, totalBytes }: { usedBytes: number; totalBytes: number }) {
  const t = useT();
  const known = capacityKnown(totalBytes);
  const pct = known ? Math.round(driveUsagePct(usedBytes, totalBytes)) : 0;
  const R = 52;
  const CIRC = 2 * Math.PI * R;
  const color = !known ? 'var(--paper-3)' : isDriveCritical(usedBytes, totalBytes) ? 'var(--risk-danger)' : pct >= 60 ? 'var(--risk-caution)' : 'var(--risk-safe)';
  return (
    <div className="ao-gauge">
      <svg viewBox="0 0 140 140">
        <circle cx="70" cy="70" r={R} fill="none" stroke="var(--paper-3)" strokeWidth="16" />
        <circle
          cx="70"
          cy="70"
          r={R}
          fill="none"
          stroke={color}
          strokeWidth="16"
          strokeLinecap="round"
          strokeDasharray={`${known ? (pct / 100) * CIRC : 0} ${CIRC}`}
          transform="rotate(-90 70 70)"
        />
        <text x="70" y="74" textAnchor="middle" className="ao-gauge-pct">{known ? `${pct}%` : '—'}</text>
        <text x="70" y="92" textAnchor="middle" className="ao-gauge-label">{known ? t('overview.gauge.used') : t('overview.gauge.unknown')}</text>
      </svg>
    </div>
  );
}

// ── 磁盘空间趋势纯逻辑（R6）：抽成可测函数，组件只负责渲染 ──────────

/** 路径规范化比较键：C:\ 与 C:\Users 视为同盘（取盘根两字符）。复用 format.normKey。 */
function spaceNormKey(p: string): string {
  return normKey(p);
}

/** 把 {root -> 快照} 聚合到盘级序列：按时间排序。
 * 传入 drives 时，映射盘（subst）的快照并到它真正所在的卷那一列——
 * 同一块物理盘不该在趋势里出现两条曲线。
 * `rangeDays` 为时间窗口（7 / 30 / 0=全部，R90）：只保留窗口内
 * `at >= now - rangeDays*86400` 的点；默认不传 = 全部保留（后端每盘
 * 上限 180 条已够画，不再硬截断 14 条——R89 每日采样后曲线持续积累）。 */
export function buildSpaceSeries(
  hist: Record<string, SpacePoint[]>,
  drives: readonly DriveEntry[] = [],
  rangeDays = 0,
): { drive: string; points: SpacePoint[] }[] {
  const byDrive = new Map<string, SpacePoint[]>();
  const listed = new Set<string>();
  const nowSecs = Date.now() / 1000;
  const cutoff = rangeDays > 0 ? nowSecs - rangeDays * 86400 : 0;
  for (const d of drives) {
    const l = pathDriveLetter(d.path).toUpperCase();
    if (l) listed.add(l);
  }
  for (const pts of Object.values(hist)) {
    for (const p of pts) {
      if (cutoff > 0 && p.at < cutoff) continue;
      const letter = pathDriveLetter(p.root).toUpperCase();
      if (!letter) {
        // 不是盘符根（罕见）：按原样自成一条，不猜它属于哪块盘。
        const key = spaceNormKey(p.root);
        const arr = byDrive.get(key) ?? [];
        arr.push(p);
        byDrive.set(key, arr);
        continue;
      }
      const info = drives.find((d) => pathDriveLetter(d.path).toUpperCase() === letter);
      // 映射盘并到真实卷；但目标卷不在盘列表里时，映射盘是碰到那份空间的唯一入口，保留自己。
      const real = info ? volumeLetterOf(info).toUpperCase() : letter;
      const drive = (listed.has(real) ? real : letter) + ':\\';
      const arr = byDrive.get(drive) ?? [];
      arr.push(p);
      byDrive.set(drive, arr);
    }
  }
  const out: { drive: string; points: SpacePoint[] }[] = [];
  for (const [drive, pts] of byDrive) {
    pts.sort((a, b) => a.at - b.at);
    out.push({ drive, points: pts });
  }
  return out.sort((a, b) => b.points[b.points.length - 1]?.used_bytes - a.points[a.points.length - 1]?.used_bytes);
}

/** R145：空间趋势的采样充分度——趋势/外推的可信度取决于有多少个采样点：
 *  2 个点算出的「每天涨 X GB」和 30 个点算出的完全不是一回事。纯函数：
 *  ≥5 点 rich（趋势可信）、2–4 点 sparse（仅供参考）、<2 点 none（不出结论）。 */
export function spaceTrendSamples(pts: readonly { at: number }[]): 'rich' | 'sparse' | 'none' {
  const n = pts.filter((p) => Number.isFinite(p.at)).length;
  if (n < 2) return 'none';
  return n >= 5 ? 'rich' : 'sparse';
}

/** R148：耗尽预测的可信度——外推建立在首末两点上，样本少时「约 N 天满」
 *  本身就不可靠。纯函数：样本 none → none（不出预测）；sparse → low（要
 *  标注「样本少」）；rich → high（照常显示）。 */
export function forecastConfidence(samples: 'rich' | 'sparse' | 'none'): 'high' | 'low' | 'none' {
  if (samples === 'none') return 'none';
  return samples === 'sparse' ? 'low' : 'high';
}

/** 近期增量：首尾已用差 + 跨度天数文案（不足 2 条返回「记录不足」）。 */
export function spaceWeekDelta(pts: SpacePoint[]): { delta: number; days: string } {
  if (pts.length < 2) return { delta: 0, days: t('overview.trend.noRecords') };
  const first = pts[0];
  const last = pts[pts.length - 1];
  const delta = last.used_bytes - first.used_bytes;
  const spanDays = (last.at - first.at) / 86400;
  return { delta, days: spanDays < 1 ? t('overview.trend.today') : t('overview.trend.daysSpan', { n: Math.round(spanDays) }) };
}

/** R98 空间耗尽预测：按首尾采样的平均增速线性外推「多少天后用满」。
 *  返回 null = 不该预测：不足 2 点、时间跨度为 0、增速 ≤0（持平或在
 *  释放空间——释放不是「快满了」，硬算会把健康盘唱成满盘）。
 *  已满（剩余 ≤0）返回 0，调用方用「已满」文案区分于天数。 */
export function forecastFullDays(pts: SpacePoint[]): number | null {
  if (pts.length < 2) return null;
  const first = pts[0];
  const last = pts[pts.length - 1];
  const spanSecs = last.at - first.at;
  if (spanSecs <= 0) return null;
  const slope = (last.used_bytes - first.used_bytes) / spanSecs; // 字节/秒
  if (slope <= 0) return null;
  const remaining = last.total_bytes - last.used_bytes;
  if (remaining <= 0) return 0;
  const days = remaining / (slope * 86400);
  return Number.isFinite(days) && days > 0 ? days : null;
}

/** R119 耗尽风险分级（纯函数，可单测）：把 forecastFullDays 的天数映射成
 *  风险等级。null（不预测：不足 2 点/增速 ≤0/释放中）→ ok——释放不是
 *  「快满」，不唱衰；0 或 ≤30 天 → critical（已满/30 天内占满，红色告警）；
 *  ≤90 天 → warn（一个季度内，琥珀提醒）；其余 → ok。*/
export function spaceRisk(fullDays: number | null): 'ok' | 'warn' | 'critical' {
  if (fullDays === null || fullDays > 90) return 'ok';
  if (fullDays <= 30) return 'critical';
  return 'warn';
}

// R90 时间窗口选项：0 = 全部（后端每盘上限 180 条 ≈ 半年日采样）。
const trendRanges: { days: number; label: string }[] = [
  { days: 7, label: 'overview.trend.range7' },
  { days: 30, label: 'overview.trend.range30' },
  { days: 0, label: 'overview.trend.rangeAll' },
];

/** SVG 迷你曲线 path：归一化到相对自身 min/max（值平=水平线）。 */
export function spaceSparkPath(pts: SpacePoint[]): string {
  if (pts.length < 2) return '';
  const min = Math.min(...pts.map((p) => p.used_bytes));
  const max = Math.max(...pts.map((p) => p.used_bytes));
  const span = Math.max(1, max - min);
  const W = 120;
  const H = 34;
  return pts
    .map((p, i) => {
      const x = (i / (pts.length - 1)) * W;
      const y = H - 3 - ((p.used_bytes - min) / span) * (H - 6);
      return `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(' ');
}

// ── 分类占用纯逻辑（R11）：抽成可测函数，组件只负责渲染 ──────────────

/** 按 scaffold 聚合占用：父节点已打标则子节点不再重复累计（同 scaffold
 *  嵌套时外层算一次、内层不再加；不同 scaffold 嵌套各自计入）。 */
export function aggregateByScaffold(root: Node | null): Map<string, { bytes: number; files: number }> {
  const m = new Map<string, { bytes: number; files: number }>();
  if (!root) return m;
  const walk = (n: Node, parentSid: string | null) => {
    const sid = n.scaffold_id ?? null;
    if (sid && sid !== parentSid) {
      const cur = m.get(sid) ?? { bytes: 0, files: 0 };
      cur.bytes += n.size || 0;
      cur.files += n.file_count || 0;
      m.set(sid, cur);
    }
    for (const c of n.children ?? []) walk(c, sid ?? parentSid);
  };
  walk(root, null);
  return m;
}

/** 按 scaffold 聚合「可回收潜力」（选中盘建议清理项加总）。 */
export function reclaimByScaffoldIds(
  items: readonly { scaffoldId: string; bytes: number }[],
): Map<string, number> {
  const m = new Map<string, number>();
  for (const it of items) {
    m.set(it.scaffoldId, (m.get(it.scaffoldId) ?? 0) + it.bytes);
  }
  return m;
}

/** R92：每个 scaffold 的「主目录」路径（同 aggregateByScaffold 口径——父节点已
 *  打标则子节点不算，只收最外层命中；同 scaffold 多命中取体量最大的那个）。
 *  分类占用行点击时用它定位到工作台；没有命中目录的分类返回不了键（点击不动作）。
 *  纯函数可测。 */
export function scaffoldMainPaths(root: Node | null): Map<string, string> {
  const m = new Map<string, { path: string; size: number }>();
  if (!root) return new Map();
  const walk = (n: Node, parentSid: string | null) => {
    const sid = n.scaffold_id ?? null;
    if (sid && sid !== parentSid) {
      const size = n.size || 0;
      const prev = m.get(sid);
      if (prev === undefined || size > prev.size) m.set(sid, { path: n.path, size });
    }
    for (const c of n.children ?? []) walk(c, sid ?? parentSid);
  };
  walk(root, null);
  const out = new Map<string, string>();
  for (const [sid, v] of m) out.set(sid, v.path);
  return out;
}

// ── 今日体检（health-butler P0）：把散点的健康信号汇总成一页横幅 ──
// 全只读：红盘/可回收来自 drives 与建议清单缓存，温度/启动项来自后端一次查询，
// 不为展示新造任何磁盘扫描。纯函数便于单测，组件只负责装配与跳转。

/** 温度档位判定：是否达到「偏高」阈值（健康管家口径，可单测）。 */
export function isTempHigh(tempC: number | null | undefined, warnC = 80): boolean {
  return tempC != null && tempC >= warnC;
}

/** 启动项里「已禁用」条数（禁用算低风险健康项，展示用）。
 * 传 enabled 字段数组（缺省按 true 处理），返回禁用量。 */
export function disabledStartupCount(items: readonly { enabled: boolean }[] | null | undefined): number {
  if (!items) return 0;
  return items.filter((it) => !it.enabled).length;
}

/** 启动项检查三态（R74）：'ok'=读到数据；'unread'=读失败（≠「未检测」）；'none'=真空未检测。 */
export type StartupCheckState = 'ok' | 'unread' | 'none';
export function startupCheckState(
  startup: readonly { enabled: boolean }[] | null,
  startupErr: string | null,
): StartupCheckState {
  if (startup) return 'ok';
  if (startupErr) return 'unread';
  return 'none';
}

// 磁盘空间趋势（R6）：拉取扫描历史，按盘画迷你曲线 + 近期增量定位。
// 纯只读展示：数据来自每次扫描后追加的 space-history.jsonl，绝不触盘 IO。
function SpaceTrendCard({ selPath, drives, onGoWorkspace }: { selPath: string | null; drives: readonly DriveEntry[]; onGoWorkspace: (drivePath: string) => void }) {
  const t = useT();
  const [hist, setHist] = useState<Record<string, SpacePoint[]>>({});
  const [histErr, setHistErr] = useState<string | null>(null);
  useEffect(() => {
    api.getSpaceHistory().then(setHist).catch((e) => {
      // 读失败 ≠ 没扫过：记录原因，别让「还没扫」的空态文案盖住真实错误。
      setHist({});
      setHistErr(String(e instanceof Error ? e.message : e));
    });
  }, []);

  const selfKey = selPath ? spaceNormKey(selPath) : null;

  // 时间窗口（R90）：7 / 30 天 / 全部。默认 30 天——R89 每日采样后曲线持续
  // 积累（后端每盘上限 180 条），全量会把两个月的点压进一条小曲线里看不清。
  const [range, setRange] = useState<number>(30);

  // 渐变 id 需要每实例唯一：用模块级自增计数而非 Math.random——随机既不可复现，
  // 多实例并发渲染时也可能撞 id（曲线引用错位）。
  const fillId = useMemo(() => `trend-fill-${trendFillSeq()}`, []);

  // 聚合到盘：只取同盘快照（C:\ vs C:\Users 视为同一盘；记录本身是扫描根，
  // 去重键取盘根）。映射盘的快照并到真实卷。按时间排序，窗口内全保留。
  const series = useMemo(() => buildSpaceSeries(hist, drives, range), [hist, drives, range]);
  // 窗口过滤后的空 ≠ 没有历史：历史里有点但都落在窗口外时给切换提示，
  // 别用「还没扫过」的引导文案骗人。
  const histEmpty = Object.keys(hist).length === 0;

  if (histErr !== null) {
    return (
      <section className="ao-card">
        <h2 className="ao-card-title"><BarChart3 size={16} /> {t('overview.trend.title')}</h2>
        <div className="ao-empty"><Info size={16} />{t('overview.trend.loadFailed', { msg: histErr })}</div>
      </section>
    );
  }
  if (series.length === 0) {
    return (
      <section className="ao-card">
        <h2 className="ao-card-title"><BarChart3 size={16} /> {t('overview.trend.title')}</h2>
        <div className="ao-empty">
          <Info size={16} />
          {histEmpty ? t('overview.trend.empty') : t('overview.trend.rangeEmpty')}
        </div>
        {/* 历史里有数据但窗口过滤后为空：给个切回全量的出口，别让用户卡死。 */}
        {!histEmpty && (
          <div className="ao-trend-range">
            {trendRanges.map((r) => (
              <button key={r.days} type="button" className="ghost small" onClick={() => setRange(r.days)}>
                {t(r.label)}
              </button>
            ))}
          </div>
        )}
      </section>
    );
  }

  // 本周新增：最早（≥7 天前若够）与最新快照的已用差。不足 7 天取首尾差。
  const weekDelta = (pts: SpacePoint[]) => spaceWeekDelta(pts);

  // SVG 迷你面积曲线：归一化到 0..100（相对自身 min/max），点间直线，
  // 曲线下加同色渐变填充，视觉更立体。
  const sparkAreaPath = (pts: SpacePoint[]) => {
    const line = spaceSparkPath(pts);
    if (!line) return '';
    const H = 34;
    const W = 120;
    const lastX = ((pts.length - 1) / (pts.length - 1)) * W;
    return `${line} L${lastX.toFixed(1)},${H} L0,${H} Z`;
  };

  return (
    <section className="ao-card">
      <h2 className="ao-card-title">
        <BarChart3 size={16} /> {t('overview.trend.title')}
        <span className="ao-card-note">{t('overview.trend.note')}</span>
        {/* R90 时间窗口切换：7 天 / 30 天 / 全部。 */}
        <span className="ao-trend-range">
          {trendRanges.map((r) => (
            <button
              key={r.days}
              type="button"
              className={'ghost small' + (range === r.days ? ' sel' : '')}
              onClick={() => setRange(r.days)}
            >
              {t(r.label)}
            </button>
          ))}
        </span>
      </h2>
      <div className="ao-trend-list">
        {series.map(({ drive, points }) => {
          const { delta, days } = weekDelta(points);
          const sel = selfKey === spaceNormKey(drive);
          const last = points[points.length - 1];
          const usage = last.total_bytes > 0 ? (last.used_bytes / last.total_bytes) * 100 : 0;
          const isShrinking = delta < 0; // 负增量 = 空间释放
          // R98 空间耗尽预测：按首尾增速外推「多少天后用满」；0 = 已满。
          const samples = spaceTrendSamples(points);
          const confidence = forecastConfidence(samples);
          const fullDays = forecastFullDays(points);
          const forecastLabel =
            fullDays === null ? null
            : fullDays === 0 ? t('overview.trend.fullNow')
            // R148：样本少（2–4 点）时外推不可靠，加「?」提示别当准数
            : confidence === 'low'
              ? t('overview.trend.fullInRough', { n: Math.ceil(fullDays) })
              : t('overview.trend.fullIn', { n: Math.ceil(fullDays) });
          return (
            <button
              key={drive}
              className={'ao-trend-row' + (sel ? ' sel' : '')}
              onClick={() => onGoWorkspace(drive)}
              title={t('overview.locateInWorkspace')}
            >
              <span className="ao-trend-drive">
                {drive.replace('\\', '')}
                <small>{t('overview.trend.driveUnit')}</small>
              </span>
              <svg className="ao-trend-svg" viewBox="0 0 120 34" preserveAspectRatio="none">
                <defs>
                  <linearGradient id={fillId} x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="var(--accent-strong)" stopOpacity="0.30" />
                    <stop offset="100%" stopColor="var(--accent-strong)" stopOpacity="0.02" />
                  </linearGradient>
                </defs>
                <path d={sparkAreaPath(points)} fill={`url(#${fillId})`} />
                <path d={spaceSparkPath(points)} fill="none" stroke="var(--accent-strong)" strokeWidth="2" />
              </svg>
              <span className="ao-trend-meta">
                <span className="ao-trend-pct">{usage.toFixed(0)}%</span>
                <span className="ao-trend-used">{formatBytes(last.used_bytes)}</span>
              </span>
              <span className={'ao-trend-delta ' + (isShrinking ? 'down' : 'up')}>
                <b>{delta > 0 ? '+' : ''}{formatBytes(delta)}</b>
                <small>{days}</small>
              </span>
{/* R145：采样点充分度——2 点算出的增速不足以当结论 */}
              {samples !== 'none' && (
                <span
                  className={'ao-trend-samples muted small' + (samples === 'sparse' ? ' sparse' : '')}
                  title={
                    samples === 'sparse'
                      ? t('overview.trend.samplesSparse', { n: points.length })
                      : t('overview.trend.samplesRich', { n: points.length })
                  }
                >
                  {points.length}
                </span>
              )}
              {forecastLabel && (
                <span className={'ao-trend-forecast ' + spaceRisk(fullDays)}>
                  {forecastLabel}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </section>
  );
}

interface CleanItem {
  key: string;
  scaffoldId: string;
  scopeId: string;
  label: string;
  desc: string;
  bytes: number;
  files: number;
  /** scope.prompt.kind==='days' 的默认天数：预览与真实执行共用同一旋钮，
   *  预览显示的文件 = 执行会删的文件（诚实性口径一致）。 */
  days?: number;
  /** R244：命中目录（cleanupSuggestions 返回的 path=hitPath）——预览的
   *  「上次清理」用它匹配台账（dry-run 合成 source 匹配不到真实目录）。 */
  path: string;
}

/** 勾选的建议项里是否含微信缓存（G-高3）：微信多账号必须去清理页按账号清，
 *  总览没有 wxid 选择器，一键清理会覆盖所有账号。纯函数便于回归测试。 */
export function hasCheckedWechat(items: readonly CleanItem[], checked: Record<string, boolean>): boolean {
  return items.some((i) => i.scaffoldId === 'wechat-pc' && !!checked[i.key]);
}

/** R126：建议清理按可回收字节降序排序——最有价值的清理项排最前（微信缓存/
 *  大块头优先），用户一眼看到收益最大项。后端顺序不稳定（map 迭代序），
 *  前端统一口径。导出供单测锁定排序规则。 */
export function sortCleanItemsByBytes(items: readonly CleanItem[]): CleanItem[] {
  return [...items].sort((a, b) => b.bytes - a.bytes);
}

// ── R102 scope 逐文件预览的纯展示逻辑（可单测）─────────────────────

/** 预览路径列表的截断策略：最多显示前 limit 条，其余折成一行
 *  `… 另有 N 条`。limit ≤ 0 视为不截断。 */
export function truncatePaths(paths: readonly string[], limit: number): { shown: readonly string[]; more: number } {
  if (limit <= 0 || paths.length <= limit) return { shown: paths, more: 0 };
  return { shown: paths.slice(0, limit), more: paths.length - limit };
}

/** 展开区的标题行：`{n} 个文件 · {size}`；空清单（读到了但没匹配）返回 null。
 *  文案键由调用方注入（i18n），本函数只组织数字参数。 */
export function previewSummary(
  filesCount: number,
  bytes: number | null,
  tFiles: (opts: { n: number }) => string,
): string | null {
  if (filesCount === 0) return null;
  const size = bytes == null ? '' : ` · ${formatBytes(bytes)}`;
  return `${tFiles({ n: filesCount })}${size}`;
}

/** 判定一条预览结果该显示什么：加载中 / 读失败（诚实：失败≠空）/ 空清单 / 有清单。
 *  顺序语义（R106 抽成纯函数，渲染与单测共用）：in-flight 优先 loading——
 *  即便上一次已留 err 也先转 loading（重试中不显示旧错误）；失败 ≠ 空清单。 */
export type PreviewState = 'loading' | 'error' | 'empty' | 'has';

export function previewState(
  loading: boolean,
  err: string | null | undefined,
  items: readonly unknown[] | undefined,
): PreviewState {
  if (loading) return 'loading';
  if (err) return 'error';
  return (items?.length ?? 0) === 0 ? 'empty' : 'has';
}

/** R102 scope 展开明细：将删文件路径清单（dry-run 预览，与执行同 olderThanDays
 *  口径）。最多展示 12 条，其余折成「另有 N 条」；路径按 basename 截短防长串撑爆。 */
const PEEK_LIMIT = 12;

function peekBase(p: string): string {
  // Windows 路径可能含 \ 或 /（dry-run 返回的匹配路径是系统原生分隔）。
  const i = Math.max(p.lastIndexOf('\\'), p.lastIndexOf('/'));
  return i >= 0 ? p.slice(i + 1) : p;
}

/** R105：预览项合计字节。全部项 bytes 为 null（后端未统计）时返回 null，
 *  部分缺失则按已知求和（展示口径：有实测才算，缺的省略）。 */
export function previewTotalBytes(items: readonly { bytes: number | null }[]): number | null {
  if (items.length === 0) return null;
  let known = 0;
  let anyKnown = false;
  for (const it of items) {
    if (it.bytes != null) {
      known += it.bytes;
      anyKnown = true;
    }
  }
  return anyKnown ? known : null;
}

/** R128：dry-run 预览项按字节降序——大文件优先展示（用户一眼看到哪些匹配
 *  项占用最多）。bytes 为 null（后端未统计）的项沉底且保持相对顺序；不修改
 *  入参数组。导出供单测锁定排序规则。 */
export function sortPreviewItemsByBytes(
  items: readonly { path: string; bytes: number | null }[],
): { path: string; bytes: number | null }[] {
  return [...items].sort((a, b) => {
    if (a.bytes == null && b.bytes == null) return 0;
    if (a.bytes == null) return 1;
    if (b.bytes == null) return -1;
    return b.bytes - a.bytes;
  });
}

function ScopePreview({ items, lastClean }: { items: readonly { path: string; bytes: number | null }[]; lastClean: number | null }) {
  const t = useT();
  // R128：按字节降序排序后再截断——大文件优先展示（PEEK_LIMIT 截断时
  // 留下的是占用最大的项，而不是后端返回顺序里的前几条）。
  const sorted = sortPreviewItemsByBytes(items);
  const files = sorted.map((i) => i.path);
  const { shown, more } = truncatePaths(files, PEEK_LIMIT);
  // 头部合计：同一份 dry-run 返回的实测字节求和（部分缺失 → 按已知合计，
  // 缺失的只影响精度不影响「要删哪些」——执行时以实测为准）。
  const totalBytes = previewTotalBytes(items);
  return (
    <div className="ao-clean-preview-box">
      <div className="ao-clean-preview-head">
        <span>{previewSummary(files.length, totalBytes, (o) => t('overview.clean.peekFiles', o))}</span>
        {/* R244：上次清理徽标——与清理页 scope 行/脚本卡同台账同源；
            台账读失败或无记录时不显示（读失败 ≠ 真没清过，诚实）。 */}
        {lastClean != null && (
          <span className="ao-clean-preview-lastclean">{t('overview.clean.peekLastClean', { age: formatLastActive(lastClean) })}</span>
        )}
        <span className="muted small">{t('overview.clean.peekHint')}</span>
      </div>
      <ul className="ao-clean-preview-list">
        {shown.map((p) => {
          const it = sorted.find((i) => i.path === p);
          return (
            <li key={p} className="ao-clean-preview-file" title={p}>
              <File size={11} />
              <span className="ao-clean-preview-name">{peekBase(p)}</span>
              <span className="ao-clean-preview-dir">{p.slice(0, Math.max(0, p.lastIndexOf('\\')))}</span>
              {it?.bytes != null && <span className="ao-clean-preview-size">{formatBytes(it.bytes)}</span>}
            </li>
          );
        })}
      </ul>
      {more > 0 && (
        <div className="ao-clean-preview-more">{t('overview.clean.peekMore', { n: more })}</div>
      )}
    </div>
  );
}

// ── 今日体检条（health-butler P0）──
// 全只读把散点的健康信号汇总成一条横幅：红盘数、可回收合计、温度、启动项。
// 温度走 fanCurveAdvice().temp_max_c（后端已归一化最高温，一次调用），
// 启动项走 listStartupItems。不为展示新造任何磁盘扫描。
const TEMP_WARN_C = 80;

function HealthCheckupCard({
  drives,
  reclaimTotal,
  reclaimErr,
  scanning,
  onScanAll,
  onGoCleanup,
  onGoHardware,
}: {
  drives: readonly DriveInfo[];
  reclaimTotal: number;
  reclaimErr?: string | null;
  scanning: boolean;
  onScanAll: () => void;
  onGoCleanup: () => void;
  onGoHardware?: () => void;
}) {
  const t = useT();
  // 温度 / 启动项 / 硬盘健康每次进总览拉一次（非关键路径，失败静默降级为「未读到」）。
  // 硬盘健康走快路径：只跑 WMI，家页不启动 CrystalDiskInfo（那是「工具」页该做的事，详情见工具页的硬盘健康面板）。
  const [tempC, setTempC] = useState<number | null>(null);
  const [startup, setStartup] = useState<StartupItem[] | null>(null);
  // startup 读失败单独记：失败 ≠「未检测」（真空语义），≠「无启动项」。
  const [startupErr, setStartupErr] = useState<string | null>(null);
  const [diskHealth, setDiskHealth] = useState<HwDiskHealth | null>(null);
  useEffect(() => {
    let alive = true;
    api.fanCurveAdvice().then((r) => { if (alive && r?.temp_max_c != null) setTempC(r.temp_max_c); }).catch(() => {});
    api.listStartupItems().then((r) => { if (alive) { setStartup(r); setStartupErr(null); } }).catch((e) => { if (alive) { setStartup(null); setStartupErr(String(e instanceof Error ? e.message : e)); } });
    ensureHwDiskQuick().then((h) => { if (alive) setDiskHealth(h); }).catch(() => {});
    return () => { alive = false; };
  }, []);

  // 红盘按物理卷数：映射盘（D: 是 J:\D 的 subst）不是第二块盘，
  // 直接数条目会把一块盘说成「2 块盘空间告急」。容量没读到的盘（total 无效）
  // 不能算「无红盘」——没读到 ≠ 健康，单独计 unknownCount 提示。
  const knownDrives = physicalDrives(drives).filter((d) => capacityKnown(d.total_bytes));
  const redCount = knownDrives.filter((d) => isDriveCritical(d.used_bytes, d.total_bytes)).length;
  const redUnknownCount = physicalDrives(drives).length - knownDrives.length;
  const disabledCount = disabledStartupCount(startup);
  const tempHigh = isTempHigh(tempC, TEMP_WARN_C);

  // 硬盘健康一格：读不到就说读不到，Unknown 不等于坏（第 26 轮的教训）。
  const hwDisks = diskHealth?.disks ?? [];
  const hwSummary = summarizeDiskHealth(hwDisks);
  const riskyDisks = riskyDrives(hwDisks);

  return (
    <section className="ao-checkup">
      <div className="ao-checkup-head">
        <Sparkles size={14} className="ao-checkup-icon" />
        <b>{t('overview.checkup.title')}</b>
        <span className="muted small">{t('overview.checkup.subtitle')}</span>
      </div>
      <div className="ao-checkup-grid">
        {/* 红盘 */}
        <button className={'ao-checkup-item' + (redCount > 0 ? ' danger' : '')} onClick={onScanAll} disabled={scanning} title={t('overview.checkup.goRescue')}>
          <HardDrive size={14} />
          <span>
            {redCount > 0
              ? t('overview.checkup.redDrive', { n: redCount })
              : redUnknownCount > 0
                ? t('overview.checkup.redDriveUnread', { n: redUnknownCount })
                : t('overview.checkup.redDriveNone')}
          </span>
        </button>
        {/* 可回收 */}
        <button className={'ao-checkup-item' + (reclaimTotal > 0 ? ' accent' : '')} onClick={onGoCleanup} title={t('overview.checkup.goClean')}>
          <Zap size={14} />
          <span>
            {reclaimTotal > 0
              ? t('overview.checkup.reclaimable', { size: formatBytes(reclaimTotal) })
              : reclaimErr
                ? t('overview.checkup.reclaimUnread')
                : t('overview.checkup.reclaimableNone')}
          </span>
        </button>
        {/* 温度 */}
        <div className={'ao-checkup-item' + (tempHigh ? ' danger' : '')} title={tempHigh ? t('overview.checkup.tempHigh', { warn: TEMP_WARN_C }) : undefined}>
          <Thermometer size={14} />
          <span>{tempC != null ? t('overview.checkup.temp', { temp: Math.round(tempC) }) : t('overview.checkup.tempNone')}</span>
        </div>
        {/* 启动项 */}
        <div className="ao-checkup-item">
          <Rocket size={14} />
          <span>
            {startupCheckState(startup, startupErr) === 'ok'
              ? t('overview.checkup.startup', { n: startup!.length }) + (disabledCount > 0 ? ` · ${t('overview.checkup.startupDisabled', { n: disabledCount })}` : '')
              : startupCheckState(startup, startupErr) === 'unread'
                ? t('overview.checkup.startupUnread')
                : t('overview.checkup.startupNone')}
          </span>
        </div>
        {/* 硬盘健康：读不到就说读不到；Unknown 只标「未上报」，不给红牌（第 26 轮的教训） */}
        <button
          className={'ao-checkup-item' + (hwSummary.state === 'risky' ? (hwSummary.bad > 0 ? ' danger' : ' caution') : '')}
          onClick={onGoHardware}
          disabled={!onGoHardware}
          title={
            riskyDisks.length > 0
              ? `${t('overview.checkup.goHardware')} · ${riskyDisks.map((d) => (d.letters ? `${d.letters}: ${d.friendly}` : d.friendly)).join(' · ')}`
              : t('overview.checkup.goHardware')
          }
        >
          {hwSummary.state === 'risky' ? <ShieldAlert size={14} /> : hwSummary.total === 0 ? <ShieldQuestion size={14} /> : <ShieldCheck size={14} />}
          <span>
            {hwSummary.state === 'none'
              ? t('overview.checkup.diskNone')
              : hwSummary.state === 'risky'
                ? t('overview.checkup.diskRisky', { n: hwSummary.risky })
                : hwSummary.state === 'all-unknown'
                  ? t('overview.checkup.diskUnknown', { n: hwSummary.unknown })
                  : hwSummary.state === 'partial'
                    ? t('overview.checkup.diskPartial', { good: hwSummary.total - hwSummary.unknown, unknown: hwSummary.unknown })
                    : t('overview.checkup.diskAllGood', { n: hwSummary.total })}
          </span>
        </button>
      </div>
    </section>
  );
}

export function AssetOverview({ root, drives, scaffolds, scanning, onScanDrive, onScanAll, onRefresh, onGoWorkspace, onOpenCleanup, onGoHardware }: Props) {
  const t = useT();
  const scanCache = useStore((s) => s.scanCache);
  const toast = useStore((s) => s.toast);
  const addReclaimed = useStore((s) => s.addReclaimed);
  const setProposal = useStore((s) => s.setProposal);
  // 重复文件扫描状态：idle | busy（扫描中）
  const [dupScan, setDupScan] = useState<'idle' | 'busy'>('idle');

  // 当前选中磁盘
  const [selPath, setSelPath] = useState<string | null>(drives[0]?.path ?? null);
  const selDrive = useMemo(() => drives.find((d) => d.path === selPath) ?? drives[0] ?? null, [drives, selPath]);

  // ── 分类占用（R11 复盘）：根换源为「选中盘」─ 与建议清理/空间大头同一数据维度 ──
  // 原本遍历全局 root（可能是"我的电脑"全部磁盘虚拟根）：切盘后卡片仍显示全盘
  // 数据，与按选中盘的 cleanItems 对不上。现在按选中盘树聚合（顶层只认
  // scanCache 命中，盘没扫过宁可显示「扫描后显示分类」也不拿别的盘数顶缸），
  // 避免「分类占用显示 D 盘、可清理项算的是 C 盘」的错位。
  const selScanned = selDrive ? !!scanCache[normKey(selDrive.path)] : false;
  const catBase = useMemo(() => {
    if (selDrive) return scanCache[normKey(selDrive.path)] ?? null;
    return root;
  }, [selDrive, scanCache, root]);

  // 分类占用聚合（父节点已打标则子节点不再重复累计）
  const byScaffold = useMemo(() => aggregateByScaffold(catBase), [catBase]);

  const scaffoldStats = useMemo(() => {
    const items: { scaffold: Scaffold; bytes: number; files: number }[] = [];
    for (const sc of scaffolds) {
      const hit = byScaffold.get(sc.id);
      if (!hit) continue;
      items.push({ scaffold: sc, bytes: hit.bytes, files: hit.files });
    }
    return items.sort((a, b) => b.bytes - a.bytes);
  }, [scaffolds, byScaffold]);

  const [scOpen, setScOpen] = useState(false);
  const SC_LIMIT = 6;
  const shownScaffolds = scOpen ? scaffoldStats : scaffoldStats.slice(0, SC_LIMIT);

  // R92: 分类 → 主目录路径（同盘树一次 DFS，行点击定位用；盘未扫则空）。
  const scMainPaths = useMemo(() => scaffoldMainPaths(catBase), [catBase]);

  // 建议清理项目：选中盘的 scope 级可回收项
  const [cleanItems, setCleanItems] = useState<CleanItem[]>([]);
  const [checked, setChecked] = useState<Record<string, boolean>>({});
  const [cleaning, setCleaning] = useState(false);
  const [cleanLoading, setCleanLoading] = useState(false);
  const [cleanErr, setCleanErr] = useState<string | null>(null);
  // R102 scope 逐文件预览：展开的 key → { items, err }。items 是 dry-run 预览
  // （与执行同 olderThanDays 口径），每项带实测字节（bytes_freed，后端 dry-run
  // 分支逐文件实测返回）；err 单独记录——读失败 ≠ 空清单（诚实）。
  const [previewOpen, setPreviewOpen] = useState<Record<string, boolean>>({});
  const [previews, setPreviews] = useState<Record<string, { items: { path: string; bytes: number | null }[]; err: string | null; lastClean: number | null }>>({});
  const [previewing, setPreviewing] = useState<Record<string, boolean>>({});

  // 展开一项：dry-run 拿将删文件明细。只读（dryRun=true），绝不落盘。
  const togglePreview = async (it: CleanItem) => {
    if (!selDrive) return;
    const open = !previewOpen[it.key];
    setPreviewOpen((p) => ({ ...p, [it.key]: open }));
    if (!open) return; // 收起：不重拉
    if (previews[it.key]) return; // 已加载过：不重复请求
    setPreviewing((p) => ({ ...p, [it.key]: true }));
    try {
      const entries = await api.executeScope(it.scaffoldId, it.scopeId, selDrive.path, true, {
        olderThanDays: it.days,
      });
      // dry-run 每条 UndoEntry.source = 匹配文件路径，bytes_freed = 实测字节——
      // 预览的每文件大小与执行口径一致（同一份后端返回，无需再次扫描）。
      // R244：顺带拉台账聚合「上次清理」——用建议项命中目录 it.path 匹配
      // ledger source（dry-run 返回的 source 是合成清单路径，匹配不到真实
      // 目录台账；命中目录才是与清理页 scope 行/脚本卡同源的口径）。
      let lastClean: number | null = null;
      try {
        const ledger = await api.listUndo(200);
        lastClean = latestCleanTs(ledger, it.path);
      } catch {
        // 台账读失败：不显示徽标（与清理页同语义，读失败 ≠ 真没清过）。
      }
      setPreviews((p) => ({
        ...p,
        [it.key]: { items: entries.map((e) => ({ path: e.source, bytes: e.bytes_freed ?? null })), err: null, lastClean },
      }));
    } catch (e) {
      setPreviews((p) => ({ ...p, [it.key]: { items: [], err: String(e instanceof Error ? e.message : e), lastClean: null } }));
    } finally {
      setPreviewing((p) => ({ ...p, [it.key]: false }));
    }
  };

  // 两步确认（铁律#6）：第一次点「立即清理」只进入预备态，按钮变为明确的
  // 确认文案；5 秒内再点一次才真正执行。倒计时结束自动解除，防止页面停留
  // 久了之后误点第二下。
  const [armed, setArmed] = useState(false);
  const armTimeout = useRef<number | null>(null);
  useEffect(() => () => { if (armTimeout.current) window.clearTimeout(armTimeout.current); }, []);

  const loadClean = useCallback(async (path: string) => {
    setCleanLoading(true);
    setCleanErr(null);
    try {
      // cachedOnly=true：只读后端缓存/内存树，秒回；从不触发全盘 walk。
      // 应用刚启动还没扫过盘时，这里会拿到空列表并显示「先扫描再算」——
      // 展示建议不值得把整块磁盘遍历一遍（那就是"一开就卡"的元凶）。
      const raw = await api.cleanupSuggestions(path, undefined, true);
      const scaffoldsFor = scaffolds; // 闭包取当前目录清单
      // R126：按可回收字节降序排序——最有价值的清理项排最前（微信缓存/大
      // 块头优先），用户一眼看到收益最大项；后端顺序不稳定（map 迭代序），
      // 前端统一口径。
      const items: CleanItem[] = sortCleanItemsByBytes(
        raw.map((r) => {
          const sc = scaffoldsFor.find((s) => s.id === r.scaffold_id);
          const scope = sc?.scopes.find((s) => s.id === r.scope_id);
          const days = scope?.prompt && scope.prompt.kind === 'days' ? Number(scope.prompt.default) : undefined;
          return {
            key: `${r.scaffold_id}:${r.scope_id}`,
            scaffoldId: r.scaffold_id,
            scopeId: r.scope_id,
            label: r.label,
            desc: r.desc,
            bytes: r.bytes,
            files: r.files,
            days,
            path: r.path,
          };
        }),
      );
      setCleanItems(items);
      setChecked(Object.fromEntries(items.map((i) => [i.key, true])));
    } catch (e) {
      // 后端统计失败不是「真没有」：记错误，让空态显示「没能读取」而不是
      // 「很干净」——读失败时说盘很干净是最误导的收尾。
      setCleanItems([]);
      setCleanErr(String(e instanceof Error ? e.message : e));
    } finally {
      setCleanLoading(false);
    }
  }, [scaffolds]);

  // 触发条件 = 选盘变化 + 扫描完成（scanSeq 递增）：扫描开始前 selDrive 就已
  // 选中，首次 loadClean(cachedOnly) 拿到空列表；扫完不重拉的话红盘会一直
  // 显示「暂无可清理项目」，用户以为扫描没有后续动作。cachedOnly=true 秒回，
  // 多拉一次无成本；selDrive 与 scanSeq 同帧变化时 React 自动批处理只跑一次。
  const scanSeq = useStore((s) => s.scanSeq);
  useEffect(() => {
    if (selDrive) void loadClean(selDrive.path);
  }, [selDrive, scanSeq, loadClean]);

  const selCount = useMemo(() => cleanItems.filter((i) => checked[i.key]).length, [cleanItems, checked]);
  const selBytes = useMemo(() => cleanItems.filter((i) => checked[i.key]).reduce((s, i) => s + i.bytes, 0), [cleanItems, checked]);
  // 分类占用卡标题上的「可回收潜力合计」：全部建议项加总（不依赖勾选状态）。
  const reclaimTotal = useMemo(() => cleanItems.reduce((s, i) => s + i.bytes, 0), [cleanItems]);
  // 可回收潜力按 scaffold 聚合（选中盘建议清理项，与 cleanItems 同源）：
  // 在分类占用条的尺寸旁标注「可回收 X」，一眼看出哪些大块头还能清出空间。
  const reclaimByScaffold = useMemo(() => reclaimByScaffoldIds(cleanItems), [cleanItems]);
  // 微信缓存一旦勾选，从总览一键清理会覆盖本机所有微信账号（清理页才有账号筛选）。
  // G-高3 修复：总览没有 wxid 选择器，误清其他账号的代价不可逆，所以勾选微信项时
  // 一键清理按钮改走「引导去清理页」——真实清理在那里按账号筛选执行。
  const wechatChecked = useMemo(
    () => hasCheckedWechat(cleanItems, checked),
    [cleanItems, checked],
  );

  const doClean = async () => {
    const sel = cleanItems.filter((i) => checked[i.key]);
    if (!sel.length || !selDrive) return;
    // 微信项被勾选：不静默清所有账号，引导去清理页（那里能选账号）。
    if (wechatChecked) {
      toast(t('overview.clean.wechatRedirect'), 'err');
      onOpenCleanup();
      return;
    }
    if (!armed) {
      // 第一步：进入预备态，5 秒内再点一次才真删。
      setArmed(true);
      if (armTimeout.current) window.clearTimeout(armTimeout.current);
      armTimeout.current = window.setTimeout(() => setArmed(false), 5000);
      return;
    }
    // 第二步：确认执行。
    setArmed(false);
    if (armTimeout.current) window.clearTimeout(armTimeout.current);
    setCleaning(true);
    try {
      let bytes = 0;
      let cleaned = 0;
      let failed = 0;
      let firstError = '';
      // R224：成功回收条目的 source 路径收集——执行后一次性剪 root/scanCache，
      // 工作台/分类占用跨页立即反映真实剩余体量（与清理页 R222 同一语义）。
      const recycledPaths: string[] = [];
      for (const it of sel) {
        try {
          // 返回实际操作的条目列表：空数组 = 该 scope 没匹配到文件（可能已清过），
          // 不算清理成功，预估字节也不计入，避免「清了 12 项 · 0 B」的误导。
          // days 与预览同源（it.days，loadClean 时从 scope.prompt 取）——
          // 预览显示的文件就是执行会删的文件。
          const entries = await api.executeScope(it.scaffoldId, it.scopeId, selDrive.path, false, {
            olderThanDays: it.days,
            confirmed: true,
          });
          if (entries.length > 0) {
            bytes += it.bytes;
            cleaned += 1;
            for (const e of entries) if (e.source) recycledPaths.push(e.source);
          }
        } catch (e) {
          failed += 1;
          if (!firstError) firstError = String(e);
          console.warn(`[diskpilot] executeScope ${it.scaffoldId}/${it.scopeId} failed:`, e);
        }
      }
      addReclaimed(bytes);
      // R224：成功回收跨页同步剪树（root + 受影响缓存树）。
      if (recycledPaths.length > 0) useStore.getState().pruneRecycled(recycledPaths);
      if (cleaned > 0) {
        toast(
          t('overview.clean.done', { n: cleaned, size: formatBytes(bytes) })
            + (failed > 0 ? t('overview.clean.someFailed', { n: failed }) : ''),
          failed > 0 ? 'err' : 'ok',
        );
      } else {
        toast(
          failed > 0
            ? t('overview.clean.allFailed', { n: failed, err: firstError })
            : t('overview.clean.nothingMatched'),
          'err',
        );
      }
      await loadClean(selDrive.path);
      onRefresh(selDrive.path);
    } finally {
      setCleaning(false);
    }
  };

  // 页脚「已用」按物理卷求和：映射盘和它的目标卷是同一块盘，数两遍就虚高。
  // M4：缺 used_bytes 的卷按 0 计（不猜），但要在页脚注明有几块盘没计入，
  // 否则「本机已用 X」会把缺失说成「真的只用了这么多」。
  const usedTotal = physicalUsedBytes(drives);
  const usedUnknownCount = physicalDrives(drives).filter((d) => !(d.used_bytes > 0)).length;
  const scannedCount = drives.filter((d) => scanCache[normKey(d.path)]).length;

  // 扫描完成自动刷新可清理项：cachedOnly=true 只读内存树，秒回；
  // 兑现「扫描后直接给可安全清理 Top 榜」，低配用户少点一次。
  const prevScanned = useRef<boolean | null>(null);
  useEffect(() => {
    if (prevScanned.current === false && selScanned && selDrive) {
      void loadClean(selDrive.path);
    }
    prevScanned.current = selScanned;
  }, [selScanned, selDrive, loadClean]);

  // 红盘救援横幅的「查看可清理项」按钮：滚动到建议清理卡片（ao-clean-card）。
  const resqHintRef = useRef<HTMLDivElement | null>(null);
  const scrollToClean = () => {
    resqHintRef.current?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  };

  const cap: [string, string, string] = selDrive
    ? formatBytesTriple(selDrive.total_bytes, selDrive.used_bytes, selDrive.free_bytes)
    : ['-', '-', '-'];

  // 底部空间大头：选中盘（或合并根）的一级子目录按大小排序取前 10
  const topDirs = useMemo(() => {
    const base = selDrive ? (scanCache[normKey(selDrive.path)] ?? root) : root;
    if (!base) return [];
    const children = (base.children ?? []).filter((n) => n.is_dir && (n.size || 0) > 0);
    const sorted = [...children].sort((a, b) => (b.size || 0) - (a.size || 0)).slice(0, 10);
    const total = base.size || sorted.reduce((s, n) => s + (n.size || 0), 0);
    return sorted.map((n) => ({
      name: n.name || n.path,
      files: n.file_count || 0,
      size: n.size || 0,
      pct: total > 0 ? Math.round(((n.size || 0) / total) * 100) : 0,
    }));
  }, [scanCache, selDrive, root]);

  const quickActions = [
    { id: 'ai', icon: Sparkles, label: t('overview.quick.ai'), tip: t('overview.quick.aiTip') },
    { id: 'big', icon: FileSearch, label: t('overview.quick.big'), tip: t('overview.quick.bigTip') },
    { id: 'dup', icon: Files, label: t('overview.quick.dup'), tip: t('overview.quick.dupTip') },
    { id: 'junk', icon: Trash2, label: t('overview.quick.junk'), tip: t('overview.quick.junkTip') },
  ] as const;

  // 重复文件查找：扫描选中盘 → 把每组（保留 1 份，其余可回收）映射成
  // 确认窗条目 → setProposal 弹出 CleanupProposalDialog。清理仍走确认窗的
  // executeAiPlan（user_confirmed）铁律，这里只出清单。
  const onDupScan = async () => {
    const target = selDrive?.path;
    if (!target) return;
    setDupScan('busy');
    try {
      const groups = await api.scanDuplicateFiles(target, 1 * 1024 * 1024); // 只建议 ≥1MB 的重复
      if (!groups || groups.length === 0) {
        toast(t('overview.dup.noneFound'), 'ok');
        return;
      }
      const items: CleanupProposalItem[] = [];
      let total = 0;
      for (const g of groups) {
        // 每组保留 1 份（keep），其余非运行中文件为回收候选。caution 级：
        // 头部哈希可能有极少误判，执行前确认窗还会逐个实测大小。
        for (const f of g.recycle_candidates) {
          items.push({
            path: f,
            reason: t('overview.dup.reason', { n: g.files.length, size: formatBytes(g.size) }),
            size_bytes: g.size,
            risk: 'caution',
            what: t('overview.dup.what'),
            purpose: t('overview.dup.purpose', { keep: g.keep }),
            impact: t('overview.dup.impact'),
          });
          total += g.size;
        }
      }
      if (items.length === 0) {
        toast(t('overview.dup.allRunning'), 'ok');
        return;
      }
      setProposal({
        id: `dup-${Date.now()}`,
        title: t('overview.dup.title', { path: target, size: formatBytes(total) }),
        items,
      });
    } catch (e) {
      console.warn('[diskpilot] scanDuplicateFiles failed:', e);
      toast(t('overview.dup.scanFailed'), 'err');
    } finally {
      setDupScan('idle');
    }
  };

  const onQuick = (id: string) => {
    if (id === 'dup') {
      onDupScan();
      return;
    }
    if (id === 'junk') {
      onOpenCleanup();
      return;
    }
    onGoWorkspace();
  };

  // 建议清理项目：四种状态互斥，避免"读失败/选择磁盘/未发现/可释放"叠在一起
  type CleanState = 'loading' | 'empty' | 'has' | 'error';
  const cleanState: CleanState = cleanLoading ? 'loading' : cleanErr !== null ? 'error' : cleanItems.length > 0 ? 'has' : 'empty';
  const cleanSummaryText =
    cleanState === 'loading' ? t('overview.clean.checking')
      : cleanState === 'has' ? t('overview.clean.estimated', { size: formatBytes(selBytes) })
        : cleanState === 'error' ? t('overview.clean.failed')
          : selScanned ? t('overview.clean.noneOnDrive')
            : t('overview.clean.needScan');

  return (
    <div className="asset-overview">
      {/* ---- 今日体检条：全只读汇总（红盘 / 可回收 / 温度 / 启动项 / 硬盘健康）---- */}
      <HealthCheckupCard
        drives={drives}
        reclaimTotal={reclaimTotal}
        reclaimErr={cleanErr}
        scanning={scanning}
        onScanAll={onScanAll}
        onGoCleanup={onOpenCleanup}
        onGoHardware={onGoHardware}
      />

      {/* ---- 顶部硬盘扫描条：一键扫描全部 + 盘卡片横向滚动 ---- */}
      {drives.length > 0 && (
        <DriveStrip
          drives={drives}
          scanning={scanning}
          onScanAll={onScanAll}
          onScanDrive={onScanDrive}
          onRefresh={onRefresh}
          selPath={selPath}
          onSelect={setSelPath}
        />
      )}

      {/* ---- 低配救援横幅：选中盘使用率 >=85% 时出现 ---- */}
      {selDrive && isDriveCritical(selDrive.used_bytes, selDrive.total_bytes) && (
        <div className="ao-rescue">
          <span className="ao-rescue-ic"><AlertTriangle size={18} /></span>
          <span className="ao-rescue-txt">
            <b>{t('overview.rescue.title', { drive: driveLetter(selDrive.path), pct: Math.round(driveUsagePct(selDrive.used_bytes, selDrive.total_bytes)) })}</b>
            <i>{selScanned
              ? selBytes > 0 ? t('overview.rescue.scannedHas', { size: formatBytes(selBytes) }) : t('overview.rescue.scannedNone')
              : t('overview.rescue.notScanned')}</i>
          </span>
          {!selScanned ? (
            <button className="ao-rescue-btn" onClick={() => onScanDrive(selDrive.path)} disabled={scanning}>
              {scanning ? t('overview.rescue.scanning') : t('overview.rescue.scanNow')}
            </button>
          ) : (
            <button className="ao-rescue-btn" onClick={scrollToClean} disabled={!cleanItems.length}>{t('overview.rescue.viewClean')}</button>
          )}
        </div>
      )}

      {/* ---- 左右两栏：左 40% / 右 60% ---- */}
      <div className="ao-grid">
        {/* ===== 左侧栏 ===== */}
        <div className="ao-left">
          {/* 磁盘详情卡片 */}
          <section className="ao-card">
            <h2 className="ao-card-title"><HardDrive size={16} /> {t('overview.detail.title')}</h2>
            {selDrive ? (
              <>
                <div className="ao-detail-head">
                  <DriveGauge usedBytes={selDrive.used_bytes} totalBytes={selDrive.total_bytes} />
                  <div className="ao-detail-info">
                    <div className="ao-detail-name">{t('overview.detail.name', { drive: driveLetter(selDrive.path) })}</div>
                    <div className="ao-detail-sub">{selScanned ? t('overview.detail.scanned', { path: selDrive.path }) : t('overview.detail.unscanned', { path: selDrive.path })}</div>
                  </div>
                </div>
                <ul className="ao-detail-list">
                  <li><i className="ao-detail-dot" style={{ background: 'var(--accent-strong)' }} />{t('overview.detail.total')}<b>{cap[0]}</b></li>
                  <li><i className="ao-detail-dot" style={{ background: 'var(--risk-caution)' }} />{t('overview.detail.used')}<b>{cap[1]}</b></li>
                  <li><i className="ao-detail-dot" style={{ background: 'var(--risk-safe)' }} />{t('overview.detail.free')}<b>{cap[2]}</b></li>
                  {/* 映射盘（subst）不是独立物理盘：详情里也要说清这份空间在哪个卷上，
                      否则用户会以为这是两块盘、两份空间。 */}
                  {selDrive.mapped_to && (
                    <li><i className="ao-detail-dot" style={{ background: 'var(--accent-strong)' }} />{t('overview.detail.mapped')}<b>{selDrive.mapped_to}</b></li>
                  )}
                </ul>
              </>
            ) : (
              <div className="ao-empty"><Info size={16} />{t('overview.noDrive')}</div>
            )}
          </section>

          {/* 磁盘空间趋势（R6）：扫描历史曲线 + 近期增量 */}
          <SpaceTrendCard selPath={selPath} drives={drives} onGoWorkspace={onGoWorkspace} />

          {/* 快速操作按钮区 */}
          <section className="ao-card">
            <h2 className="ao-card-title"><Zap size={16} /> {t('overview.quick.title')}</h2>
            <div className="ao-quick">
              {quickActions.map((a) => (
                <button key={a.id} className="ao-quick-btn" onClick={() => onQuick(a.id)} title={a.tip}>
                  <a.icon size={16} /> {a.label}
                  {a.id === 'dup' && dupScan === 'busy' && <span className="muted small">…</span>}
                </button>
              ))}
            </div>
          </section>
        </div>

        {/* ===== 右侧栏 ===== */}
        <div className="ao-right">
          {/* 文件分类占用统计 */}
          <section className="ao-card">
            <h2 className="ao-card-title">
              <BarChart3 size={16} /> {t('overview.cat.title')}
              {reclaimTotal > 0 && <span className="ao-card-note">{t('overview.cat.reclaimNote', { size: formatBytes(reclaimTotal) })}</span>}
            </h2>
            {catBase ? (
              scaffoldStats.length === 0 ? (
                <div className="ao-empty"><Info size={16} />{t('overview.cat.noHit')}</div>
              ) : (
                <div className="ao-bars">
                  {shownScaffolds.map(({ scaffold, bytes }) => {
                    const color = catColor(scaffold.id);
                    const max = scaffoldStats[0]?.bytes || 1;
                    const pct = Math.max(3, Math.min(100, (bytes / max) * 100));
                    const reclaim = reclaimByScaffold.get(scaffold.id) ?? 0;
                    // R92: 行点击 → 工作台定位到该分类体量最大的目录（同盘已扫）。
                    // R219: 该分类有可回收潜力时行点击 → 清理页预选该脚本（「可回收 X」
                    // 才是这一行的主指引，去工作台定位还得再找清理入口）；
                    // 没有可回收项的分类保持原样跳工作台。
                    const mainPath = scMainPaths.get(scaffold.id);
                    const canClean = reclaim > 0;
                    return (
                      <button
                        key={scaffold.id}
                        type="button"
                        className={'ao-bar-row' + (canClean || mainPath ? ' clickable' : '')}
                        onClick={canClean
                          ? () => onOpenCleanup(scaffold.id)
                          : mainPath ? () => onGoWorkspace(mainPath) : undefined}
                        title={canClean
                          ? t('overview.cat.gotoCleanup', { name: scaffold.name })
                          : mainPath ? `${t('overview.locateInWorkspace')} · ${mainPath}` : undefined}
                      >
                        <span className="ao-bar-dot" style={{ background: color }} />
                        <span className="ao-bar-name">{scaffold.name}</span>
                        <span className="ao-bar-track"><i style={{ width: `${pct}%`, background: color }} /></span>
                        <span className="ao-bar-size">{formatBytes(bytes)}</span>
                        <span className="ao-bar-reclaim" title={reclaim > 0 ? t('overview.cat.reclaimTip') : undefined}>
                          {reclaim > 0 ? `↳ ${t('overview.cat.reclaim', { size: formatBytes(reclaim) })}` : ''}
                        </span>
                      </button>
                    );
                  })}
                  {scaffoldStats.length > SC_LIMIT && (
                    <button className="ao-sc-more" onClick={() => setScOpen((v) => !v)}>
                      {scOpen ? t('overview.cat.collapse') : t('overview.cat.expand', { n: scaffoldStats.length - SC_LIMIT })}
                    </button>
                  )}
                </div>
              )
            ) : (
              <div className="ao-empty"><FolderOpen size={16} />{selDrive ? t('overview.cat.needScan') : t('overview.noDrive')}</div>
            )}
          </section>

          {/* 建议清理项目 + 底部操作栏 */}
          <section className="ao-card ao-clean-card" ref={resqHintRef}>
            <h2 className="ao-card-title">
              <Trash2 size={16} /> {t('overview.clean.title')}
              <span className="ao-clean-summary">{cleanSummaryText}</span>
              <button className="ghost small" onClick={() => onOpenCleanup()} style={{ marginLeft: 'auto' }}>
                {t('overview.clean.openPage')}
              </button>
            </h2>
            <div className="ao-clean-list">
              {cleanState === 'loading' ? (
                <div className="ao-empty"><RefreshCw size={16} className="ao-spin" />{t('overview.clean.checkingList')}</div>
              ) : cleanState === 'error' ? (
                <div className="ao-empty"><AlertTriangle size={16} />{t('overview.clean.loadFailed', { msg: cleanErr ?? '' })}</div>
              ) : cleanState === 'empty' ? (
                <div className="ao-empty">
                  {selScanned
                    ? <><Check size={16} />{selDrive ? t('overview.clean.spotless', { drive: driveLetter(selDrive.path) }) : t('overview.noDrive')}</>
                    : <><Info size={16} />{selDrive ? t('overview.clean.needScan') : t('overview.noDrive')}</>}
                </div>
              ) : (
                cleanItems.map((it) => (
                  <div key={it.key} className="ao-clean-item">
                    <label className="ao-clean-row">
                      <input
                        type="checkbox"
                        className="ao-clean-check"
                        checked={!!checked[it.key]}
                        onChange={() => setChecked((c) => ({ ...c, [it.key]: !c[it.key] }))}
                      />
                      <span className="ao-clean-txt">
                        <b>{it.label}</b>
                        <i>{it.desc}</i>
                      </span>
                      <span className="ao-clean-size">{formatBytes(it.bytes)}</span>
                      <button
                        type="button"
                        className="ao-clean-goto"
                        // R217：逐条建议项可直达清理页并预选该脚本——此前只有
                        // 顶部「打开清理页」（无参，进页自动挑第一个命中），
                        // 用户想针对某个建议项去清理页得自己翻脚本列表。
                        onClick={() => onOpenCleanup(it.scaffoldId)}
                        title={t('overview.clean.gotoTip', { label: it.label })}
                      >
                        {t('overview.clean.goto')}
                      </button>
                      <button
                        type="button"
                        className="ao-clean-peek"
                        onClick={() => void togglePreview(it)}
                        title={previewOpen[it.key] ? t('overview.clean.peekHide') : t('overview.clean.peekShow')}
                      >
                        {previewOpen[it.key] ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
                      </button>
                    </label>
                    {previewOpen[it.key] && (() => {
                      const st = previewState(
                        !!previewing[it.key],
                        previews[it.key]?.err,
                        previews[it.key]?.items,
                      );
                      return (
                        <div className="ao-clean-preview">
                          {st === 'loading' ? (
                            <div className="ao-clean-preview-note"><RefreshCw size={12} className="ao-spin" />{t('overview.clean.peekLoading')}</div>
                          ) : st === 'error' ? (
                            // 读失败 ≠ 空清单：明确说明预览失败（不代表没有文件）。
                            <div className="ao-clean-preview-note err">{t('overview.clean.peekFailed', { msg: previews[it.key]!.err ?? '' })}</div>
                          ) : st === 'empty' ? (
                            <div className="ao-clean-preview-note">{t('overview.clean.peekEmpty')}</div>
                          ) : (
                            <ScopePreview
                              items={previews[it.key]!.items}
                              lastClean={previews[it.key]!.lastClean}
                            />
                          )}
                        </div>
                      );
                    })()}
                  </div>
                ))
              )}
            </div>
            <div className="ao-actionbar">
              {wechatChecked && (
                <span className="ao-actionbar-warn">{t('overview.clean.wechatAll')}</span>
              )}
              <span className="ao-actionbar-info">
                {t('overview.clean.selPrefix')} <b>{selCount}</b> {t('overview.clean.selSuffix')} <b>{formatBytes(selBytes)}</b>
              </span>
              {/* R240：全选 / 清空 一键切换——逐项勾选或取消后不必逐个点，
                  一个按钮在全选与清空间切换；微信项选中仍走底部微信引导。 */}
              <button
                type="button"
                className="ghost small"
                disabled={!cleanItems.length || cleaning}
                onClick={() => {
                  const allChecked = selCount === cleanItems.length;
                  setChecked(Object.fromEntries(cleanItems.map((i) => [i.key, !allChecked])));
                  setArmed(false);
                  if (armTimeout.current) window.clearTimeout(armTimeout.current);
                }}
              >
                {selCount === cleanItems.length && cleanItems.length > 0
                  ? t('overview.clean.clearAll')
                  : t('overview.clean.selectAll')}
              </button>
              {armed && !cleaning && (
                <button
                  className="ao-clean-btn ao-clean-btn-cancel"
                  onClick={() => {
                    setArmed(false);
                    if (armTimeout.current) window.clearTimeout(armTimeout.current);
                  }}
                >
                  {t('overview.clean.cancel')}
                </button>
              )}
              <button
                className={'ao-clean-btn' + (armed ? ' armed' : '')}
                onClick={doClean}
                disabled={!selCount || cleaning}
                title={armed ? t('overview.clean.armTitle') : t('overview.clean.firstTitle')}
              >
                {cleaning ? t('overview.clean.running') : armed ? t('overview.clean.confirmArmed', { n: selCount }) : t('overview.clean.now')}
              </button>
            </div>
          </section>
        </div>
      </div>

      {/* ---- 底部：空间大头 Top 10 ---- */}
      {topDirs.length > 0 && (
        <section className="ao-card" style={{ padding: '12px 0 6px' }}>
          <h2 className="ao-card-title" style={{ padding: '0 16px' }}>
            <Package size={16} /> {t('overview.top.title')}
            <span className="ao-card-note">{t('overview.top.note')}</span>
          </h2>
          <div className="ao-toplist" style={{ marginTop: 4 }}>
            {topDirs.map((n, i) => (
              <div key={i} className="ao-top-row" onClick={() => onGoWorkspace()} title={t('overview.locateInWorkspace')}>
                <span className="ao-rank">{i + 1}</span>
                <span className="ao-top-name">{n.name}</span>
                <span className="ao-top-files">{t('overview.top.files', { count: n.files.toLocaleString() })}</span>
                <span className="ao-top-bar"><i style={{ width: `${Math.max(2, Math.min(100, n.pct))}%` }} /></span>
                <span className="ao-top-size">{formatBytes(n.size)}</span>
                <span className="ao-top-pct">{n.pct}%</span>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* ---- 底部：扫描统计 ---- */}
      <div className="ao-foot">
        <span>
          <HardDrive size={14} /> {t('overview.foot.used', { size: formatBytes(usedTotal) })}
          {usedUnknownCount > 0 && <span className="muted small"> · {t('overview.foot.usedUnread', { n: usedUnknownCount })}</span>}
        </span>
        <span>
          <Check size={14} /> {t('overview.foot.scanned', { n: scannedCount, total: drives.length })}
        </span>
        <span className="ao-foot-refresh" onClick={() => selDrive && onRefresh(selDrive.path)} title={t('overview.foot.rescanTitle')}>
          <RefreshCw size={13} /> {t('overview.foot.rescan')}{selDrive ? t('overview.foot.rescanDrive', { drive: driveLetter(selDrive.path) }) : ''}
        </span>
      </div>
    </div>
  );
}
