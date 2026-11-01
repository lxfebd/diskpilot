import { useEffect, useMemo, useRef, useState, useCallback } from 'react';
import {
  RefreshCw,
  Folder,
  Trash2,
  Search,
  Zap,
  AlertTriangle,
  X,
  FileText,
  FileDown,
  Copy,
  ChevronRight,
  Boxes,
  FolderOpen,
} from 'lucide-react';
import clsx from 'clsx';
import { api } from '../api';
import { revealWithToast } from '../reveal';
import { useStore } from '../store';
import type { SteamGame, SteamInventory } from '../types';
import { formatBytes, formatDateTime, formatDateKey } from '../format';
import { t, useT } from '../i18n';
import { SteamWorkshopModal } from './SteamWorkshopModal';
import { scrollBehavior } from '../motion';
import { activateOnKey, clearSearchOnClick, clearSearchOnEscape } from '../a11y';

type Pivot = 'sleep' | 'size' | 'library' | 'lastplayed';

// 透视标签走 i18n（表里存 key，渲染时 t() 取值，未翻译回退中文）。
const PIVOT_LABELS: Record<Pivot, string> = {
  sleep: 'steam.pivot.sleep',
  size: 'steam.pivot.size',
  library: 'steam.pivot.library',
  lastplayed: 'steam.pivot.lastplayed',
};

const PIVOT_ORDER: Pivot[] = ['sleep', 'size', 'library', 'lastplayed'];

function relativeTime(ts: number | null): string {
  if (ts == null) return t('steam.neverPlayed');
  const now = Date.now() / 1000;
  const diff = now - ts;
  if (diff < 0) return t('steam.justNow');
  const day = 86400;
  const month = day * 30.4375;
  const year = month * 12;
  if (diff < day) return t('steam.today');
  if (diff < 2 * day) return t('steam.yesterday');
  if (diff < 7 * day) return t('steam.daysAgo', { n: Math.floor(diff / day) });
  if (diff < month) return t('steam.weeksAgo', { n: Math.floor(diff / day / 7) });
  if (diff < 2 * month) return t('steam.lastMonth');
  if (diff < year) return t('steam.monthsAgo', { n: Math.floor(diff / month) });
  return t('steam.yearsAgo', { n: Math.floor(diff / year) });
}

function absoluteTime(ts: number | null): string {
  if (ts == null) return t('steam.neverPlayed');
  const d = new Date(ts * 1000);
  return formatDateKey(d);
}

function sleepScore(g: SteamGame): number {
  if (g.is_ghost) return Number.POSITIVE_INFINITY;
  const sizeGb = g.size_bytes / 1_000_000_000;
  const now = Date.now() / 1000;
  const monthsSince = g.last_played_ts == null ? 12 : (now - g.last_played_ts) / (86400 * 30.4375);
  return sizeGb * monthsSince;
}

/// 排序透视（导出供单测）：不改原数组。sleep=休眠分（大小×未玩月数）
/// 幽灵永远置顶；size=按体积；library=按库路径再按体积；lastplayed=最近
/// 游玩在前、未玩过沉底。
export function sortByPivot(games: SteamGame[], pivot: Pivot): SteamGame[] {
  const list = [...games];
  switch (pivot) {
    case 'sleep':
      list.sort((a, b) => sleepScore(b) - sleepScore(a));
      break;
    case 'size':
      list.sort((a, b) => b.size_bytes - a.size_bytes);
      break;
    case 'library':
      list.sort((a, b) => {
        const lib = a.library_root.localeCompare(b.library_root);
        if (lib !== 0) return lib;
        return b.size_bytes - a.size_bytes;
      });
      break;
    case 'lastplayed':
      list.sort((a, b) => {
        // Never-played sinks; ghost surfaces. Most-recent at top.
        const aT = a.last_played_ts ?? 0;
        const bT = b.last_played_ts ?? 0;
        return bT - aT;
      });
      break;
  }
  return list;
}

// R311：卸载残留列表的可见窗口。头部计数说的是「全部残留 N 项」，此前表格却只画前
// 12 条、不说还有几条——头部数字与可见行数对不上，用户以为清单只有这些。抽成纯
// 函数：默认给前 cap 条 + 剩余条数，展开后全给、剩余 0。
export const RESIDUE_CAP = 12;
export function residueWindow<T>(items: readonly T[], showAll: boolean, cap = RESIDUE_CAP) {
  const limit = Math.max(0, cap);
  const shown = showAll ? items : items.slice(0, limit);
  return { shown, hidden: items.length - shown.length };
}

export function SteamInspector({ onDismissBack }: { onDismissBack?: () => void }) {
  const t = useT();
  const [inventory, setInventory] = useState<SteamInventory | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedAppid, setSelectedAppid] = useState<number | null>(null);
  const [enabledLibraries, setEnabledLibraries] = useState<Set<string>>(new Set());
  const [pivot, setPivot] = useState<Pivot>('sleep');
  const [searchQuery, setSearchQuery] = useState('');
  const [showTeach, setShowTeach] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [workshopFor, setWorkshopFor] = useState<SteamGame | null>(null);
  // appid → 中文名（翻译管线异步填充；null = 翻译不可用/未命中）。
  const [nameCn, setNameCn] = useState<Record<number, string>>({});

  // R491：列表行的 roving tabindex。键盘用户 Tab 进列表只落一行（0），方向键在行间走——
  // 否则一个装上几十个游戏的书库会变成几十个 Tab 停靠点，反而更难用。
  const [rowFocus, setRowFocus] = useState(0);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  // R491：搜索或透视换了，行集合就换了一批——roving 落点收回第一行。
  useEffect(() => { setRowFocus(0); }, [searchQuery, pivot]);

  // R491:方向键在行间走。行自己的 onKeyDown（activateOnKey）处理 Enter/空格，
  // 这里只拦方向键：焦点在哪就在哪落（没聚焦任何行时默认第一行）。
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const onKeys = (e: KeyboardEvent) => {
      const rows = list.querySelectorAll<HTMLElement>('[role="option"]');
      if (rows.length === 0) return;
      const cur = Array.prototype.indexOf.call(rows, document.activeElement);
      let next = cur;
      if (e.key === 'ArrowDown') next = cur < 0 ? 0 : Math.min(rows.length - 1, cur + 1);
      else if (e.key === 'ArrowUp') next = cur <= 0 ? rows.length - 1 : cur - 1;
      else if (e.key === 'Home') next = 0;
      else if (e.key === 'End') next = rows.length - 1;
      else return;
      e.preventDefault();
      setRowFocus(next);
      rows[next]?.focus();
    };
    list.addEventListener('keydown', onKeys);
    return () => list.removeEventListener('keydown', onKeys);
  }, []);

  // 一次性 toast/教学条定时器统一登记，卸载时全部清理：避免已关闭的组件
  // 里迟到触发 setState，以及长驻的回调引用的泄漏。
  const timersRef = useRef<number[]>([]);
  // 固定身份（useCallback）：do*/refresh 会把它写进依赖数组；逐渲染重建的
  // plain fn 会让这些 useCallback 与 keydown 订阅每次渲染都抖动（2026-10-11
  // 清基线 exhaustive-deps 警告时固定）。
  const later = useCallback((fn: () => void, ms: number) => {
    const id = window.setTimeout(() => {
      timersRef.current = timersRef.current.filter((timer) => timer !== id);
      fn();
    }, ms);
    timersRef.current.push(id);
    return id;
  }, []);
  useEffect(() => () => {
    for (const timer of timersRef.current) window.clearTimeout(timer);
    timersRef.current = [];
  }, []);

  // ------------------------------------------------------------------
  // 翻译管线（§7）：本地 cache 秒出 → 未命中走 Storefront fetch。
  // 完全不依赖网络可用性；fetch 失败静默返回 {}，UI 回落英文名。
  // ------------------------------------------------------------------
  // 翻译代号：每次刷新自增。迟到/过期的翻译结果（旧扫描的 appid 集）会被
  // 丢弃，防止老库的中文名污染新库的筛选/搜索（S2 竞态守卫）。
  const translateGenRef = useRef(0);

  const translateGames = useCallback(async (inv: SteamInventory) => {
    const gen = ++translateGenRef.current;
    const ids = inv.libraries.flatMap((l) => l.games.map((g) => g.appid));
    if (ids.length === 0) return;
    const apply = (titles: Record<number, string>) => {
      if (translateGenRef.current !== gen) return; // 已过期，丢弃
      if (Object.keys(titles).length > 0) setNameCn((prev) => ({ ...prev, ...titles }));
    };
    try {
      const cached = await api.translateSteamNames(ids);
      apply(cached);
      const missing = ids.filter((id) => !(id in cached));
      if (missing.length > 0) {
        const fetched = await api.translateSteamNamesFetch(missing);
        apply(fetched);
      }
    } catch {
      // 翻译失败不打扰用户：继续用英文名渲染。
    }
  }, []);

  // ------------------------------------------------------------------
  // Data
  // ------------------------------------------------------------------
  const refresh = useCallback(() => {
    setLoading(true);
    setError(null);
    // 重置翻译表：旧库的中文名不再匹配新扫描的游戏（S2）。
    setNameCn({});
    translateGenRef.current += 1;
    api
      .listSteamGames()
      .then((inv) => {
        setInventory(inv);
        setLoading(false);
        // §6.6 teaching banner — only when scan succeeded with games.
        const total = inv.libraries.reduce((sum, l) => sum + l.games.length, 0);
        if (inv.steam_root && total > 0) {
          setShowTeach(true);
          later(() => setShowTeach(false), 3000);
        }
        // Default-enable all libraries discovered (= no filter).
        setEnabledLibraries(new Set(inv.libraries.map((l) => l.root)));
        // 异步翻译：先读本地 cache（秒出），未命中的 appid 再走网络 fetch。
        // 两者都静默降级——网络不可达时 UI 显示英文名，完全可用（§7.3）。
        void translateGames(inv);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  }, [translateGames, later]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const allGames: SteamGame[] = useMemo(() => {
    if (!inventory) return [];
    const games: SteamGame[] = [];
    for (const lib of inventory.libraries) {
      if (enabledLibraries.size > 0 && !enabledLibraries.has(lib.root)) continue;
      games.push(...lib.games);
    }
    let list = games;
    const q = searchQuery.trim().toLowerCase();
    if (q) {
      list = list.filter(
        (g) =>
          g.name_en.toLowerCase().includes(q) ||
          g.install_dir_name.toLowerCase().includes(q) ||
          (nameCn[g.appid]?.toLowerCase().includes(q) ?? false),
      );
    }
    return sortByPivot(list, pivot);
  }, [inventory, enabledLibraries, pivot, searchQuery, nameCn]);

  const selectedGame: SteamGame | null = useMemo(
    () => allGames.find((g) => g.appid === selectedAppid) ?? null,
    [allGames, selectedAppid],
  );

  // ------------------------------------------------------------------
  // Actions
  // ------------------------------------------------------------------
  const showToast = useCallback((text: string, ms = 2400) => {
    setToast(text);
    later(() => setToast(null), ms);
  }, [later]);

  // 卸载残留的可执行出口：检测到 → 移入回收站。走 store.recyclePaths 而不是自己
  // 调 api——那条链已经包含后端 recycle_paths（动作写死 Recycle、逐个校验
  // protected_path、写 undo.jsonl 可撤销）+ reclaimed 记账 + toast，别处怎么回收
  // 这里就怎么回收，不另开一条清理路径。
  // 铁律：破坏性动作禁一步即执行——垃圾桶按钮只挂出确认条，点「移入回收站」才真跑。
  const recycle = useStore((s) => s.recyclePaths);
  const [residueConfirm, setResidueConfirm] = useState<string | null>(null);
  const [residueBusy, setResidueBusy] = useState<string | null>(null);
  // R311：残留清单头部计数是全量，默认只列前 RESIDUE_CAP 条——展开后全部列出。
  const [showAllResidue, setShowAllResidue] = useState(false);

  const doRecycleResidue = useCallback(
    async (r: { path: string; libRoot: string; size_bytes: number }) => {
      setResidueConfirm(null);
      setResidueBusy(r.path);
      const res = await recycle([{ path: r.path, size_hint: r.size_bytes }], t('steam.residueReason'));
      if (res.done > 0) {
        // 本地即时剔除这一条：为一条残留重扫整库不值得（扫描要遍历全库目录）。
        setInventory((prev) =>
          prev && {
            ...prev,
            libraries: prev.libraries.map((l) =>
              l.root === r.libRoot
                ? {
                    ...l,
                    residue: l.residue.filter((x) => x.path !== r.path),
                    residue_size_bytes: Math.max(0, l.residue_size_bytes - r.size_bytes),
                  }
                : l,
            ),
          },
        );
      }
      setResidueBusy(null);
    },
    [recycle, t],
  );

  const doRevealManifest = useCallback(
    async (g: SteamGame) => {
      try {
        await api.revealInExplorer(g.appmanifest_path);
      } catch (e) {
        showToast(t('steam.errReveal', { msg: String(e) }));
      }
    },
    [showToast, t],
  );

  const doUninstallViaSteam = useCallback(async (g: SteamGame) => {
    showToast(t('steam.uninstallLaunching'));
    try {
      await api.openSteamUrl('uninstall', g.appid);
      // §6.6: if Steam doesn't come to front in 800ms, gently nudge.
      later(() => {
        showToast(t('steam.uninstallNudge'));
      }, 800);
    } catch (e) {
      showToast(t('steam.uninstallFailed', { msg: String(e) }));
    }
  }, [showToast, t, later]);

  const doOpenInstallDir = useCallback(
    async (g: SteamGame) => {
      try {
        await api.revealInExplorer(g.install_path);
      } catch (e) {
        showToast(t('steam.errOpenDir', { msg: String(e) }));
      }
    },
    [showToast, t],
  );

  const doCopyInstallPath = useCallback(async (g: SteamGame) => {
    try {
      await navigator.clipboard.writeText(g.install_path);
      showToast(t('steam.copiedPath'));
    } catch (e) {
      showToast(t('steam.copyFailed', { msg: String(e) }));
    }
  }, [showToast, t]);

  const doExportReport = useCallback(() => {
    if (!inventory) return;
    const games = inventory.libraries.flatMap((l) => l.games);
    const lines: string[] = [
      t('steam.reportTitle'),
      '',
      t('steam.reportGenerated', { time: formatDateTime(new Date()) }),
      t('steam.reportSummary', { n: games.length, size: formatBytes(games.reduce((s, g) => s + g.size_bytes, 0)) }),
      '',
      t('steam.reportHeader'),
      '| --- | --- | --- | --- |',
    ];
    const sorted = sortByPivot(games, 'sleep');
    for (const g of sorted) {
      const name = nameCn[g.appid] ?? g.name_en;
      const rec = g.recommendation_reason ?? '—';
      lines.push(`| ${name} | ${formatBytes(g.size_bytes)} | ${relativeTime(g.last_played_ts)} | ${rec} |`);
    }
    // 卸载残留：报告里单独一节——用户正是靠这份报告决定清什么。
    const residue = inventory.libraries.flatMap((l) => l.residue);
    if (residue.length > 0) {
      const residueTotal = inventory.libraries.reduce((s, l) => s + l.residue_size_bytes, 0);
      lines.push(
        '',
        t('steam.reportResidueTitle'),
        t('steam.reportResidueLead', { n: residue.length, size: formatBytes(residueTotal) }),
        t('steam.reportResidueHeader'),
        '| --- | --- | --- |',
      );
      for (const r of residue) {
        const kind = r.kind === 'workshop' ? t('steam.residueWorkshop') : t('steam.residueInstall');
        const what = r.appid_hint != null ? `appid ${r.appid_hint}` : r.dir_name;
        lines.push(`| ${kind} | ${what} | ${formatBytes(r.size_bytes)} |`);
      }
    }
    lines.push('', t('steam.reportFooter'));
    const blob = new Blob([lines.join('\n')], { type: 'text/markdown;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `steam-sleep-report-${formatDateKey(new Date())}.md`;
    a.click();
    URL.revokeObjectURL(url);
    showToast(t('steam.reportExported'));
  }, [inventory, nameCn, showToast, t]);

  const navigate = useCallback(
    (delta: number) => {
      if (allGames.length === 0) return;
      const idx = selectedAppid != null ? allGames.findIndex((g) => g.appid === selectedAppid) : -1;
      const nextIdx = Math.max(0, Math.min(allGames.length - 1, idx + delta));
      setSelectedAppid(allGames[nextIdx].appid);
      // Scroll into view
      requestAnimationFrame(() => {
        listRef.current
          ?.querySelector(`[data-appid="${allGames[nextIdx].appid}"]`)
          ?.scrollIntoView({ block: 'nearest', behavior: scrollBehavior() });
      });
    },
    [allGames, selectedAppid],
  );

  // ------------------------------------------------------------------
  // Keyboard
  // ------------------------------------------------------------------
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      // 按住 Ctrl/Meta/Alt 的组合键是应用/系统快捷键（复制、刷新、缩放等），
      // 不参与 Inspector 的字符快捷键，避免劫持 webview 原生行为（S7）。
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      // 子模态（创意工坊等）打开时禁用全局快捷键，避免误触导航/卸载
      if (workshopFor) return;
      const target = e.target as HTMLElement | null;
      const inEditable =
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        (target?.isContentEditable ?? false);
      // Allow `/` from anywhere; allow Esc from search box.
      if (e.key === '/' && !inEditable) {
        e.preventDefault();
        searchInputRef.current?.focus();
        return;
      }
      if (e.key === 'Escape' && inEditable) {
        searchInputRef.current?.blur();
        return;
      }
      if (inEditable) return;
      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          navigate(1);
          break;
        case 'ArrowUp':
          e.preventDefault();
          navigate(-1);
          break;
        case 'Enter':
          e.preventDefault();
          if (selectedAppid != null) setSelectedAppid(null);
          else if (allGames.length > 0) setSelectedAppid(allGames[0].appid);
          break;
        case 'Escape':
          e.preventDefault();
          // 第一级：关掉当前打开的详情栏；没有详情栏可关时，才把这一键
          // 交还给 Modal 层（S1：Esc 不再因为详情栏开着而无法关整个窗口）。
          if (selectedAppid != null) setSelectedAppid(null);
          else onDismissBack?.();
          break;
        case 'u':
        case 'U':
          if (selectedGame) doUninstallViaSteam(selectedGame);
          break;
        case 'o':
        case 'O':
          if (selectedGame) doOpenInstallDir(selectedGame);
          break;
        case 'c':
        case 'C':
          if (selectedGame) doCopyInstallPath(selectedGame);
          break;
        case 'r':
        case 'R':
          refresh();
          break;
        case '1':
        case '2':
        case '3':
        case '4': {
          const idx = parseInt(e.key, 10) - 1;
          setPivot(PIVOT_ORDER[idx]);
          break;
        }
      }
    }
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [navigate, selectedAppid, allGames, selectedGame, doUninstallViaSteam, doOpenInstallDir, doCopyInstallPath, refresh, workshopFor, onDismissBack]);

  // ------------------------------------------------------------------
  // Render
  // ------------------------------------------------------------------
  const totalGames = inventory?.libraries.reduce((s, l) => s + l.games.length, 0) ?? 0;
  const totalSize = inventory?.libraries.reduce((s, l) => s + l.total_size_bytes, 0) ?? 0;
  // 卸载残留（有目录无 ACF）：只统计勾选启用的库，与游戏列表口径一致。
  const residueItems =
    inventory?.libraries
      .filter((l) => enabledLibraries.has(l.root))
      .flatMap((l) => l.residue.map((r) => ({ ...r, libRoot: l.root }))) ?? [];
  const residueSize =
    inventory?.libraries
      .filter((l) => enabledLibraries.has(l.root))
      .reduce((s, l) => s + l.residue_size_bytes, 0) ?? 0;
  // R311：头部计数是全量，列表默认只画前 RESIDUE_CAP 条；hidden>0 时在列表尾
  // 给出「还有 N 条」+ 展开入口，不让人把 12 条当成全部残留。
  const residueWin = residueWindow(residueItems, showAllResidue);

  return (
    <div className="steam-inspector">
      {/* ------ progress / teach banners ------ */}
      {loading && (
        <div className="steam-progress">
          <RefreshCw size={13} className="spin" />
          <span>{t('steam.scanning')}</span>
        </div>
      )}
      {showTeach && (
        <div className="steam-teach">
          {t('steam.teachLead')}<b>{totalGames}</b>{t('steam.teachGames')}<b>{formatBytes(totalSize)}</b>{t('steam.teachTail')}
          <button className="steam-teach-close" onClick={() => setShowTeach(false)} title={t('steam.close')}>
            <X size={12} />
          </button>
        </div>
      )}
      {error && <div className="steam-error">{t('steam.scanError', { msg: error })}</div>}

      {/* ------ states without the three-column body ------ */}
      {!loading && !error && inventory && !inventory.steam_root && (
        <SteamNotFound candidates={inventory.candidates_checked} onRetry={refresh} />
      )}
      {!loading && !error && inventory && inventory.steam_root && totalGames === 0 && residueItems.length === 0 && (
        <SteamEmpty steamRoot={inventory.steam_root} onRetry={refresh} />
      )}

      {/* ------ main 3-column body ------ */}
      {!loading && !error && inventory && inventory.steam_root && (totalGames > 0 || residueItems.length > 0) && (
        <div className="steam-body">
          {/* LEFT: filters & pivots */}
          <aside className="steam-filters" aria-label={t('steam.filtersAria')}>
            <div className="steam-filter-section">
              <h4>{t('steam.libraries')}</h4>
              {inventory.libraries.map((lib) => (
                <label key={lib.root} className="steam-filter-row">
                  <input
                    type="checkbox"
                    checked={enabledLibraries.has(lib.root)}
                    onChange={(e) => {
                      setEnabledLibraries((prev) => {
                        const next = new Set(prev);
                        if (e.target.checked) next.add(lib.root);
                        else next.delete(lib.root);
                        return next;
                      });
                    }}
                  />
                  <div className="steam-filter-row-text">
                    <div className="steam-filter-row-name">{shortLibName(lib.root)}</div>
                    <div className="steam-filter-row-meta">
                      {t('steam.libMeta', { n: lib.games.length, size: formatBytes(lib.total_size_bytes) })}
                    </div>
                  </div>
                </label>
              ))}
            </div>
            <div className="steam-filter-section">
              <h4>{t('steam.pivot')}</h4>
              {PIVOT_ORDER.map((p, i) => (
                <label key={p} className={clsx('steam-pivot-row', { active: pivot === p })}>
                  <input
                    type="radio"
                    name="steam-pivot"
                    checked={pivot === p}
                    onChange={() => setPivot(p)}
                  />
                  <span>{t(PIVOT_LABELS[p])}</span>
                  <kbd className="steam-kbd">{i + 1}</kbd>
                </label>
              ))}
            </div>
            <div className="steam-filter-meta">
              {t('steam.totalMeta', { n: totalGames, size: formatBytes(totalSize) })}
              <button
                className="steam-icon-btn"
                onClick={doExportReport}
                title={t('steam.exportTitle')}
              >
                <FileDown size={12} />
              </button>
              <button className="steam-icon-btn" onClick={refresh} title={t('steam.rescanTitle')} aria-label={t('steam.rescanTitle')}>
                <RefreshCw size={12} />
              </button>
            </div>
          </aside>

          {/* MIDDLE: list */}
          <section className="steam-list-wrap">
            {residueItems.length > 0 && (
              <div className="steam-residue">
                <div className="steam-residue-head">
                  <AlertTriangle size={13} />
                  <b>{t('steam.residueTitle')}</b>
                  <span className="steam-residue-meta">
                    {t('steam.residueMeta', { n: residueItems.length, size: formatBytes(residueSize) })}
                  </span>
                </div>
                <div className="steam-residue-lead">{t('steam.residueLead')}</div>
                {residueWin.shown.map((r) => (
                  <div key={`${r.libRoot}-${r.path}`} className="steam-residue-item">
                    <div className="steam-residue-row">
                      <span className="steam-residue-kind">
                        {r.kind === 'workshop' ? t('steam.residueWorkshop') : t('steam.residueInstall')}
                      </span>
                      <span className="steam-residue-name" title={r.path}>
                        {r.appid_hint != null ? `appid ${r.appid_hint}` : r.dir_name}
                      </span>
                      <span className="steam-residue-size">{formatBytes(r.size_bytes)}</span>
                      {r.last_modified_ts > 0 && (
                        <span className="steam-residue-age">
                          {t('steam.residueAge', {
                            days: Math.max(0, Math.floor((Date.now() / 1000 - r.last_modified_ts) / 86400)),
                          })}
                        </span>
                      )}
                      <button
                        className="steam-icon-btn"
                        onClick={() => void revealWithToast(r.path)}
                        title={t('steam.residueOpenTitle')}
                        aria-label={t('steam.residueOpenTitle')}
                      >
                        <FolderOpen size={12} />
                      </button>
                      {residueBusy === r.path ? (
                        <span className="steam-residue-busy">{t('steam.residueRecycling')}</span>
                      ) : (
                        <button
                          className="steam-icon-btn steam-residue-recycle"
                          onClick={() => setResidueConfirm(r.path)}
                          title={t('steam.residueRecycleTitle')}
                          aria-label={t('steam.residueRecycleTitle')}
                        >
                          <Trash2 size={12} />
                        </button>
                      )}
                    </div>
                    {residueConfirm === r.path && (
                      <div className="steam-residue-confirm">
                        <span>
                          {t('steam.residueConfirmLead', {
                            name: r.appid_hint != null ? `appid ${r.appid_hint}` : r.dir_name,
                            size: formatBytes(r.size_bytes),
                          })}
                        </span>
                        <button className="primary small" onClick={() => void doRecycleResidue(r)}>
                          {t('steam.residueConfirmGo')}
                        </button>
                        <button className="ghost small" onClick={() => setResidueConfirm(null)}>
                          {t('steam.residueConfirmCancel')}
                        </button>
                      </div>
                    )}
                  </div>
                ))}
                {/* R311：头部计数是全量，这里明说没画全的部分并给展开入口 */}
                {(residueWin.hidden > 0 || (showAllResidue && residueItems.length > RESIDUE_CAP)) && (
                  <div className="steam-residue-more">
                    {residueWin.hidden > 0 && (
                      <span className="muted small">{t('steam.residueMore', { n: residueWin.hidden })}</span>
                    )}
                    <button
                      type="button"
                      className="ghost small"
                      onClick={() => setShowAllResidue((v) => !v)}
                    >
                      {showAllResidue ? t('steam.residueCollapse') : t('steam.residueShowAll')}
                    </button>
                  </div>
                )}
              </div>
            )}
            <div className="steam-search">
              <Search size={13} />
                            {/* R439：Esc 有文字时清空搜索，空时放行（弹窗照常关） */}
<input
                ref={searchInputRef}
                type="text"
                aria-label={t('steam.searchAria')}
                placeholder={t('steam.searchPlaceholder')}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={clearSearchOnEscape(() => setSearchQuery(''))}
              />
              {searchQuery && (
                <button className="steam-icon-btn" onClick={clearSearchOnClick(() => setSearchQuery(''))} title={t('steam.clear')}>
                  <X size={12} />
                </button>
              )}
            </div>
            <div className="steam-list" ref={listRef} role="listbox">
              {allGames.length === 0 && (
                <div className="steam-list-empty" role="status">{t('steam.noMatches')}</div>
              )}
              {allGames.map((g, i) => {
                const select = () => {
                  setRowFocus(i);
                  setSelectedAppid(g.appid === selectedAppid ? null : g.appid);
                };
                return (
                  <SteamRow
                    key={`${g.library_root}-${g.appid}`}
                    game={g}
                    nameCn={nameCn[g.appid] ?? null}
                    selected={g.appid === selectedAppid}
                    onClick={select}
                    tabIndex={i === rowFocus ? 0 : -1}
                    onRowKeyDown={activateOnKey(select)}
                  />
                );
              })}
            </div>
          </section>

          {/* RIGHT: detail rail (only when a game is selected) */}
          {selectedGame && (
            <SteamDetailRail
              game={selectedGame}
              nameCn={nameCn[selectedGame.appid] ?? null}
              onClose={() => setSelectedAppid(null)}
              onRevealManifest={() => doRevealManifest(selectedGame)}
              onRevealInstall={() => doOpenInstallDir(selectedGame)}
              onCopyInstallPath={() => doCopyInstallPath(selectedGame)}
              onUninstall={() => doUninstallViaSteam(selectedGame)}
              onShowWorkshop={() => setWorkshopFor(selectedGame)}
            />
          )}
        </div>
      )}

      {/* ------ toast ------ */}
      {toast && <div className="steam-toast">{toast}</div>}

      {/* ------ workshop sub-modal ------ */}
      {workshopFor && (
        <SteamWorkshopModal
          game={workshopFor}
          nameCn={nameCn[workshopFor.appid] ?? null}
          onClose={() => setWorkshopFor(null)}
        />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

function SteamRow({
  game,
  nameCn,
  selected,
  onClick,
  tabIndex,
  onRowKeyDown,
}: {
  game: SteamGame;
  nameCn: string | null;
  selected: boolean;
  onClick: () => void;
  tabIndex?: number;
  onRowKeyDown?: (e: { key: string; preventDefault?: () => void }) => void;
}) {
  const displayName = nameCn ?? game.name_en;
  const altName = nameCn ? game.name_en : game.install_dir_name;
  return (
    <div
      role="option"
      aria-selected={selected}
      data-appid={game.appid}
      className={clsx('steam-row', {
        selected,
        recommended: game.default_recommended && !game.is_ghost,
        ghost: game.is_ghost,
      })}
      onClick={onClick}
      tabIndex={tabIndex}
      onKeyDown={onRowKeyDown}
    >
      <div className="steam-row-icon">
        {game.is_ghost ? <AlertTriangle size={14} /> : game.default_recommended ? <Zap size={14} /> : null}
      </div>
      <div className="steam-row-name">
        <div className="steam-row-name-primary" title={displayName || `appid ${game.appid}`}>{displayName || `appid ${game.appid}`}</div>
        {altName && altName !== displayName && (
          <div className="steam-row-name-alt" title={altName}>{altName}</div>
        )}
        {game.recommendation_reason && (
          <div className="steam-row-reason">{game.recommendation_reason}</div>
        )}
      </div>
      <div className="steam-row-size">{formatBytes(game.size_bytes)}</div>
      <div className="steam-row-time">{relativeTime(game.last_played_ts)}</div>
      <div className="steam-row-chevron">
        <ChevronRight size={14} />
      </div>
    </div>
  );
}

function SteamDetailRail({
  game,
  nameCn,
  onClose,
  onRevealManifest,
  onRevealInstall,
  onCopyInstallPath,
  onUninstall,
  onShowWorkshop,
}: {
  game: SteamGame;
  nameCn: string | null;
  onClose: () => void;
  onRevealManifest: () => void;
  onRevealInstall: () => void;
  onCopyInstallPath: () => void;
  onUninstall: () => void;
  onShowWorkshop: () => void;
}) {
  const t = useT();
  return (
    <aside className="steam-detail" aria-label={t('steam.detailAria')}>
      <div className="steam-detail-head">
        <div className="steam-detail-title">{nameCn ?? game.name_en}</div>
        {nameCn && <div className="steam-detail-subtitle">{game.name_en}</div>}
        {!nameCn && game.name_en !== game.install_dir_name && (
          <div className="steam-detail-subtitle">{game.install_dir_name}</div>
        )}
        {nameCn && (
          <div className="steam-citation-label steam-translate-src">{t('steam.translateSrc')}</div>
        )}
        <button className="steam-icon-btn steam-detail-close" onClick={onClose} title={t('steam.closeEsc')} aria-label={t('steam.closeEsc')}>
          <X size={14} />
        </button>
      </div>

      {game.is_ghost && (
        <div className="steam-ghost-banner">
          <AlertTriangle size={14} />
          <div>
            <b>{t('steam.ghostTitle')}</b>
            <p>{t('steam.ghostBody')}</p>
          </div>
        </div>
      )}

      <div className="steam-detail-meta">
        <div><span>appid</span><b>{game.appid}</b></div>
        <div><span>{t('steam.size')}</span><b>{formatBytes(game.size_bytes)}</b></div>
        <div><span>{t('steam.lastPlayed')}</span><b>{absoluteTime(game.last_played_ts)} · {relativeTime(game.last_played_ts)}</b></div>
        {game.last_updated_ts != null && (
          <div><span>{t('steam.lastUpdated')}</span><b>{absoluteTime(game.last_updated_ts)} · {relativeTime(game.last_updated_ts)}</b></div>
        )}
        <div><span>{t('steam.library')}</span><b>{shortLibName(game.library_root)}</b></div>
        <div><span>{t('steam.status')}</span><b>{game.is_fully_installed ? t('steam.statusComplete') : t('steam.statusIncomplete', { flags: game.state_flags })}</b></div>
        {!game.is_fully_installed && game.bytes_to_download > 0 && (
          <div>
            <span>{t('steam.dlProgress')}</span>
            <b>{formatBytes(game.bytes_downloaded)} / {formatBytes(game.bytes_to_download)}</b>
          </div>
        )}
      </div>

      {game.default_recommended && game.recommendation_reason && (
        <div className="steam-detail-reason">
          <Zap size={13} />
          <div>
            <b>{t('steam.recTitle')}</b>
            <p>{game.recommendation_reason}</p>
          </div>
        </div>
      )}

      <div className="steam-detail-citations">
        <div className="steam-citation-label">{t('steam.dataSources')}</div>
        <button className="steam-citation" onClick={onRevealManifest} title={t('steam.revealAcfTitle')}>
          <FileText size={12} />
          <span className="steam-citation-text">{shortPath(game.appmanifest_path)}</span>
        </button>
        <div className="steam-citation steam-citation-static">
          <Folder size={12} />
          <span className="steam-citation-text">{shortPath(game.install_path)}</span>
        </div>
      </div>

      <div className="steam-detail-actions">
        <button className="steam-action steam-action-primary" onClick={onUninstall} title={t('steam.uninstallBtnTitle')}>
          <Trash2 size={13} />
          <span>{t('steam.uninstallInSteam')}</span>
          <kbd className="steam-kbd">U</kbd>
        </button>
        {game.workshop_item_count > 0 && (
          <button className="steam-action" onClick={onShowWorkshop} title={t('steam.workshopBtnTitle')}>
            <Boxes size={13} />
            <span>{t('steam.workshopBtn', { n: game.workshop_item_count })}</span>
          </button>
        )}
        <button className="steam-action" onClick={onRevealInstall} title={t('steam.openDirTitle')}>
          <Folder size={13} />
          <span>{t('steam.openDir')}</span>
          <kbd className="steam-kbd">O</kbd>
        </button>
        <button className="steam-action" onClick={onCopyInstallPath} title={t('steam.copyPathTitle')}>
          <Copy size={13} />
          <span>{t('steam.copyPath')}</span>
          <kbd className="steam-kbd">C</kbd>
        </button>
      </div>
    </aside>
  );
}

function SteamNotFound({
  candidates,
  onRetry,
}: {
  candidates: string[];
  onRetry: () => void;
}) {
  const t = useT();
  return (
    <div className="steam-empty-state">
      <div className="steam-empty-title">{t('steam.notFoundTitle')}</div>
      <p>{t('steam.notFoundBody')}</p>
      <ul className="steam-path-list">
        {candidates.map((c) => (
          <li key={c}>{c}</li>
        ))}
      </ul>
      <p className="steam-empty-hint">
        {t('steam.notFoundHint')}
      </p>
      <button className="steam-action" onClick={onRetry}>
        <RefreshCw size={13} />
        <span>{t('steam.rescan')}</span>
      </button>
    </div>
  );
}

function SteamEmpty({ steamRoot, onRetry }: { steamRoot: string; onRetry: () => void }) {
  const t = useT();
  return (
    <div className="steam-empty-state">
      <div className="steam-empty-title">{t('steam.emptyTitle')}</div>
      <p>
        {t('steam.emptyLead')}<code>{steamRoot}</code>{t('steam.emptyMid')}<code>steamapps/</code>{t('steam.emptyTail')}
      </p>
      <button className="steam-action" onClick={onRetry}>
        <RefreshCw size={13} />
        <span>{t('steam.rescan')}</span>
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function shortLibName(libRoot: string): string {
  // "C:/Program Files (x86)/Steam" -> "C: (Steam)"
  // "D:/SteamLibrary"               -> "D: (SteamLibrary)"
  const m = libRoot.match(/^([A-Z]):.*?\/([^/]+)\/?$/);
  if (m) return `${m[1]}: (${m[2]})`;
  return libRoot;
}

function shortPath(p: string): string {
  // Trim middle if too long for the rail.
  if (p.length <= 56) return p;
  const head = p.slice(0, 30);
  const tail = p.slice(-22);
  return `${head}…${tail}`;
}
