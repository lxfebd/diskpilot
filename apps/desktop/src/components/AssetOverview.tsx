import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { HardDrive, Zap, Sparkles, FileSearch, Files, Trash2, Check, Info, RefreshCw, FolderOpen, BarChart3, Package, AlertTriangle, Thermometer, Rocket } from 'lucide-react';
import { api, type SpacePoint, type StartupItem } from '../api';
import { CAT_COLORS } from '../colors';
import { formatBytes, formatBytesTriple, normKey, driveLetter } from '../format';
import { useStore, type CleanupProposalItem } from '../store';
import { DriveStrip } from './DriveStrip';
import { t, useT } from '../i18n';
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
}

type Props = {
  root: Node | null;
  drives: DriveInfo[];
  scaffolds: Scaffold[];
  scanning: boolean;
  onScanDrive: (path: string) => void;
  onScanAll: () => void;
  onRefresh: (path: string) => void;
  onGoWorkspace: () => void;
  onOpenCleanup: () => void;
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

// 单盘使用率环形图：中心显示百分比
function DriveGauge({ usedBytes, totalBytes }: { usedBytes: number; totalBytes: number }) {
  const t = useT();
  const pct = Math.round(driveUsagePct(usedBytes, totalBytes));
  const R = 52;
  const CIRC = 2 * Math.PI * R;
  const color = isDriveCritical(usedBytes, totalBytes) ? 'var(--risk-danger)' : pct >= 60 ? 'var(--risk-caution)' : 'var(--risk-safe)';
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
          strokeDasharray={`${(pct / 100) * CIRC} ${CIRC}`}
          transform="rotate(-90 70 70)"
        />
        <text x="70" y="74" textAnchor="middle" className="ao-gauge-pct">{pct}%</text>
        <text x="70" y="92" textAnchor="middle" className="ao-gauge-label">{t('overview.gauge.used')}</text>
      </svg>
    </div>
  );
}

// ── 磁盘空间趋势纯逻辑（R6）：抽成可测函数，组件只负责渲染 ──────────

/** 路径规范化比较键：C:\ 与 C:\Users 视为同盘（取盘根两字符）。复用 format.normKey。 */
function spaceNormKey(p: string): string {
  return normKey(p);
}

/** 把 {root -> 快照} 聚合到盘级序列：按时间排序、每盘保留最新 14 条。 */
export function buildSpaceSeries(
  hist: Record<string, SpacePoint[]>,
): { drive: string; points: SpacePoint[] }[] {
  const byDrive = new Map<string, SpacePoint[]>();
  for (const pts of Object.values(hist)) {
    for (const p of pts) {
      const drive = spaceNormKey(p.root).slice(0, 2) + '\\';
      const arr = byDrive.get(drive) ?? [];
      arr.push(p);
      byDrive.set(drive, arr);
    }
  }
  const out: { drive: string; points: SpacePoint[] }[] = [];
  for (const [drive, pts] of byDrive) {
    pts.sort((a, b) => a.at - b.at);
    const points = pts.slice(-14);
    out.push({ drive, points });
  }
  return out.sort((a, b) => b.points[b.points.length - 1]?.used_bytes - a.points[a.points.length - 1]?.used_bytes);
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

// 磁盘空间趋势（R6）：拉取扫描历史，按盘画迷你曲线 + 近期增量定位。
// 纯只读展示：数据来自每次扫描后追加的 space-history.jsonl，绝不触盘 IO。
function SpaceTrendCard({ selPath, onGoWorkspace }: { selPath: string | null; onGoWorkspace: () => void }) {
  const t = useT();
  const [hist, setHist] = useState<Record<string, SpacePoint[]>>({});
  useEffect(() => {
    api.getSpaceHistory().then(setHist).catch(() => setHist({}));
  }, []);

  const selfKey = selPath ? spaceNormKey(selPath) : null;

  // 渐变 id 需要每实例唯一：用模块级自增计数而非 Math.random——随机既不可复现，
  // 多实例并发渲染时也可能撞 id（曲线引用错位）。
  const fillId = useMemo(() => `trend-fill-${trendFillSeq()}`, []);

  // 聚合到盘：只取同盘快照（C:\ vs C:\Users 视为同一盘；记录本身是扫描根，
  // 去重键取盘根）。按时间排序，保留最新 14 条。
  const series = useMemo(() => buildSpaceSeries(hist), [hist]);

  if (series.length === 0) {
    return (
      <section className="ao-card">
        <h2 className="ao-card-title"><BarChart3 size={16} /> {t('overview.trend.title')}</h2>
        <div className="ao-empty"><Info size={16} />{t('overview.trend.empty')}</div>
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
      </h2>
      <div className="ao-trend-list">
        {series.map(({ drive, points }) => {
          const { delta, days } = weekDelta(points);
          const sel = selfKey === spaceNormKey(drive);
          const last = points[points.length - 1];
          const usage = last.total_bytes > 0 ? (last.used_bytes / last.total_bytes) * 100 : 0;
          const isShrinking = delta < 0; // 负增量 = 空间释放
          return (
            <button
              key={drive}
              className={'ao-trend-row' + (sel ? ' sel' : '')}
              onClick={onGoWorkspace}
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
}

/** 勾选的建议项里是否含微信缓存（G-高3）：微信多账号必须去清理页按账号清，
 *  总览没有 wxid 选择器，一键清理会覆盖所有账号。纯函数便于回归测试。 */
export function hasCheckedWechat(items: readonly CleanItem[], checked: Record<string, boolean>): boolean {
  return items.some((i) => i.scaffoldId === 'wechat-pc' && !!checked[i.key]);
}

// ── 今日体检条（health-butler P0）──
// 全只读把散点的健康信号汇总成一条横幅：红盘数、可回收合计、温度、启动项。
// 温度走 fanCurveAdvice().temp_max_c（后端已归一化最高温，一次调用），
// 启动项走 listStartupItems。不为展示新造任何磁盘扫描。
const TEMP_WARN_C = 80;

function HealthCheckupCard({
  drives,
  reclaimTotal,
  scanning,
  onScanAll,
  onGoCleanup,
}: {
  drives: readonly DriveInfo[];
  reclaimTotal: number;
  scanning: boolean;
  onScanAll: () => void;
  onGoCleanup: () => void;
}) {
  const t = useT();
  // 温度 / 启动项每次进总览拉一次（非关键路径，失败静默降级为「未读到」）。
  const [tempC, setTempC] = useState<number | null>(null);
  const [startup, setStartup] = useState<StartupItem[] | null>(null);
  useEffect(() => {
    let alive = true;
    api.fanCurveAdvice().then((r) => { if (alive && r?.temp_max_c != null) setTempC(r.temp_max_c); }).catch(() => {});
    api.listStartupItems().then((r) => { if (alive) setStartup(r); }).catch(() => { setStartup([]); });
    return () => { alive = false; };
  }, []);

  const redCount = drives.filter((d) => isDriveCritical(d.used_bytes, d.total_bytes)).length;
  const disabledCount = disabledStartupCount(startup);
  const tempHigh = isTempHigh(tempC, TEMP_WARN_C);

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
            {redCount > 0 ? t('overview.checkup.redDrive', { n: redCount }) : t('overview.checkup.redDriveNone')}
          </span>
        </button>
        {/* 可回收 */}
        <button className={'ao-checkup-item' + (reclaimTotal > 0 ? ' accent' : '')} onClick={onGoCleanup} title={t('overview.checkup.goClean')}>
          <Zap size={14} />
          <span>
            {reclaimTotal > 0 ? t('overview.checkup.reclaimable', { size: formatBytes(reclaimTotal) }) : t('overview.checkup.reclaimableNone')}
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
            {startup
              ? t('overview.checkup.startup', { n: startup.length }) + (disabledCount > 0 ? ` · ${t('overview.checkup.startupDisabled', { n: disabledCount })}` : '')
              : t('overview.checkup.startupNone')}
          </span>
        </div>
      </div>
    </section>
  );
}

export function AssetOverview({ root, drives, scaffolds, scanning, onScanDrive, onScanAll, onRefresh, onGoWorkspace, onOpenCleanup }: Props) {
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

  // 建议清理项目：选中盘的 scope 级可回收项
  const [cleanItems, setCleanItems] = useState<CleanItem[]>([]);
  const [checked, setChecked] = useState<Record<string, boolean>>({});
  const [cleaning, setCleaning] = useState(false);
  const [cleanLoading, setCleanLoading] = useState(false);
  // 两步确认（铁律#6）：第一次点「立即清理」只进入预备态，按钮变为明确的
  // 确认文案；5 秒内再点一次才真正执行。倒计时结束自动解除，防止页面停留
  // 久了之后误点第二下。
  const [armed, setArmed] = useState(false);
  const armTimeout = useRef<number | null>(null);
  useEffect(() => () => { if (armTimeout.current) window.clearTimeout(armTimeout.current); }, []);

  const loadClean = useCallback(async (path: string) => {
    setCleanLoading(true);
    try {
      // cachedOnly=true：只读后端缓存/内存树，秒回；从不触发全盘 walk。
      // 应用刚启动还没扫过盘时，这里会拿到空列表并显示「先扫描再算」——
      // 展示建议不值得把整块磁盘遍历一遍（那就是"一开就卡"的元凶）。
      const raw = await api.cleanupSuggestions(path, undefined, true);
      const items: CleanItem[] = raw.map((r) => ({
        key: `${r.scaffold_id}:${r.scope_id}`,
        scaffoldId: r.scaffold_id,
        scopeId: r.scope_id,
        label: r.label,
        desc: r.desc,
        bytes: r.bytes,
        files: r.files,
      }));
      setCleanItems(items);
      setChecked(Object.fromEntries(items.map((i) => [i.key, true])));
    } catch {
      /* 后端统计失败视为无可清理项，不阻塞界面 */
    } finally {
      setCleanLoading(false);
    }
  }, []);

  useEffect(() => {
    if (selDrive) void loadClean(selDrive.path);
  }, [selDrive, loadClean]);

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
      for (const it of sel) {
        const sc = scaffolds.find((s) => s.id === it.scaffoldId);
        const scope = sc?.scopes.find((s) => s.id === it.scopeId);
        const days = scope?.prompt && scope.prompt.kind === 'days' ? Number(scope.prompt.default) : undefined;
        try {
          // 返回实际操作的条目列表：空数组 = 该 scope 没匹配到文件（可能已清过），
          // 不算清理成功，预估字节也不计入，避免「清了 12 项 · 0 B」的误导。
          const entries = await api.executeScope(it.scaffoldId, it.scopeId, selDrive.path, false, {
            olderThanDays: days,
            confirmed: true,
          });
          if (entries.length > 0) {
            bytes += it.bytes;
            cleaned += 1;
          }
        } catch (e) {
          failed += 1;
          if (!firstError) firstError = String(e);
          console.warn(`[diskpilot] executeScope ${it.scaffoldId}/${it.scopeId} failed:`, e);
        }
      }
      addReclaimed(bytes);
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

  const usedTotal = drives.reduce((s, d) => s + (d.used_bytes || 0), 0);
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

  // 建议清理项目：三种状态互斥，避免"选择磁盘/未发现/可释放"叠在一起
  type CleanState = 'loading' | 'empty' | 'has';
  const cleanState: CleanState = cleanLoading ? 'loading' : cleanItems.length > 0 ? 'has' : 'empty';
  const cleanSummaryText =
    cleanState === 'loading' ? t('overview.clean.checking')
      : cleanState === 'has' ? t('overview.clean.estimated', { size: formatBytes(selBytes) })
        : selScanned ? t('overview.clean.noneOnDrive')
          : t('overview.clean.needScan');

  return (
    <div className="asset-overview">
      {/* ---- 今日体检条：全只读汇总（红盘 / 可回收 / 温度 / 启动项）---- */}
      <HealthCheckupCard
        drives={drives}
        reclaimTotal={reclaimTotal}
        scanning={scanning}
        onScanAll={onScanAll}
        onGoCleanup={onOpenCleanup}
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
                </ul>
              </>
            ) : (
              <div className="ao-empty"><Info size={16} />{t('overview.noDrive')}</div>
            )}
          </section>

          {/* 磁盘空间趋势（R6）：扫描历史曲线 + 近期增量 */}
          <SpaceTrendCard selPath={selPath} onGoWorkspace={onGoWorkspace} />

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
                    return (
                      <div key={scaffold.id} className="ao-bar-row">
                        <span className="ao-bar-dot" style={{ background: color }} />
                        <span className="ao-bar-name">{scaffold.name}</span>
                        <span className="ao-bar-track"><i style={{ width: `${pct}%`, background: color }} /></span>
                        <span className="ao-bar-size">{formatBytes(bytes)}</span>
                        <span className="ao-bar-reclaim" title={reclaim > 0 ? t('overview.cat.reclaimTip') : undefined}>
                          {reclaim > 0 ? `↳ ${t('overview.cat.reclaim', { size: formatBytes(reclaim) })}` : ''}
                        </span>
                      </div>
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
              <button className="ghost small" onClick={onOpenCleanup} style={{ marginLeft: 'auto', fontSize: 11, padding: '0 8px' }}>
                {t('overview.clean.openPage')}
              </button>
            </h2>
            <div className="ao-clean-list">
              {cleanState === 'loading' ? (
                <div className="ao-empty"><RefreshCw size={16} className="ao-spin" />{t('overview.clean.checkingList')}</div>
              ) : cleanState === 'empty' ? (
                <div className="ao-empty"><Check size={16} />{selDrive ? t('overview.clean.spotless', { drive: driveLetter(selDrive.path) }) : t('overview.noDrive')}</div>
              ) : (
                cleanItems.map((it) => (
                  <label key={it.key} className="ao-clean-row">
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
                  </label>
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
              <div key={i} className="ao-top-row" onClick={onGoWorkspace} title={t('overview.locateInWorkspace')}>
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
