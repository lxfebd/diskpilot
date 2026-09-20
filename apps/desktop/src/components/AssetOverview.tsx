import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { HardDrive, Zap, Sparkles, FileSearch, Files, Trash2, Check, Info, RefreshCw, FolderOpen, BarChart3, Package, AlertTriangle } from 'lucide-react';
import { api, type SpacePoint } from '../api';
import { CAT_COLORS } from '../colors';
import { formatBytes, formatBytesTriple } from '../format';
import { useStore, type CleanupProposalItem } from '../store';
import { DriveStrip } from './DriveStrip';
import { t, useT } from '../i18n';
import type { Node, Scaffold } from '../types';

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
};

function normKey(p: string): string {
  return p.replace(/[\\/]+$/, '').toUpperCase();
}

function driveLetter(p: string): string {
  return p.replace(/\\$/, '').replace(/:$/, '');
}

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

/** 路径规范化比较键：C:\ 与 C:\Users 视为同盘（取盘根两字符）。 */
export function spaceNormKey(p: string): string {
  return p.replace(/[\\/]+$/, '').toUpperCase();
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

// 磁盘空间趋势（R6）：拉取扫描历史，按盘画迷你曲线 + 近期增量定位。
// 纯只读展示：数据来自每次扫描后追加的 space-history.jsonl，绝不触盘 IO。
function SpaceTrendCard({ selPath, onGoWorkspace }: { selPath: string | null; onGoWorkspace: () => void }) {
  const t = useT();
  const [hist, setHist] = useState<Record<string, SpacePoint[]>>({});
  useEffect(() => {
    api.getSpaceHistory().then(setHist).catch(() => setHist({}));
  }, []);

  const selfKey = selPath ? spaceNormKey(selPath) : null;

  // 渐变 id 需要唯一（多条 row 共用同一 svg defs namespace）
  const fillId = useMemo(() => `trend-fill-${Math.random().toString(36).slice(2, 8)}`, []);

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

export function AssetOverview({ root, drives, scaffolds, scanning, onScanDrive, onScanAll, onRefresh, onGoWorkspace }: Props) {
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

  // 分类占用聚合（父节点已打标则子节点不再重复累计）
  const byScaffold = useMemo(() => {
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
  }, [root]);

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

  const doClean = async () => {
    const sel = cleanItems.filter((i) => checked[i.key]);
    if (!sel.length || !selDrive) return;
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
  const selScanned = selDrive ? !!scanCache[normKey(selDrive.path)] : false;

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
            <h2 className="ao-card-title"><BarChart3 size={16} /> {t('overview.cat.title')}</h2>
            {root ? (
              scaffoldStats.length === 0 ? (
                <div className="ao-empty"><Info size={16} />{t('overview.cat.noHit')}</div>
              ) : (
                <div className="ao-bars">
                  {shownScaffolds.map(({ scaffold, bytes }) => {
                    const color = catColor(scaffold.id);
                    const max = scaffoldStats[0]?.bytes || 1;
                    const pct = Math.max(3, Math.min(100, (bytes / max) * 100));
                    return (
                      <div key={scaffold.id} className="ao-bar-row">
                        <span className="ao-bar-dot" style={{ background: color }} />
                        <span className="ao-bar-name">{scaffold.name}</span>
                        <span className="ao-bar-track"><i style={{ width: `${pct}%`, background: color }} /></span>
                        <span className="ao-bar-size">{formatBytes(bytes)}</span>
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
              <div className="ao-empty"><FolderOpen size={16} />{t('overview.cat.needScan')}</div>
            )}
          </section>

          {/* 建议清理项目 + 底部操作栏 */}
          <section className="ao-card ao-clean-card" ref={resqHintRef}>
            <h2 className="ao-card-title">
              <Trash2 size={16} /> {t('overview.clean.title')}
              <span className="ao-clean-summary">{cleanSummaryText}</span>
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
