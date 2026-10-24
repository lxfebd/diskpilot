// 操作历史整页（P2c）：undo.jsonl 全量台账。Studio 侧栏的 UndoPanel 只看
// 最近 50 条，这里取后端上限内的全量 + 动作过滤 + 关键词搜索 + 按天分组；
// 隔离条目可原地恢复（与 UndoPanel 同一 api.undo 双校验语义：index + source）。
import { useCallback, useEffect, useMemo, useState } from 'react';
import { History, Loader2, RotateCcw, Search } from 'lucide-react';
import { api } from '../../api';
import { useStore } from '../../store';
import { useT } from '../../i18n';
import { formatBytes, formatTime } from '../../format';
import type { UndoEntry } from '../../types';
import { WeeklyHealthCard } from './WeeklyHealthCard';
import {
  dayGroupLabel,
  filterUndoEntries,
  groupUndoByDay,
  restoreKind,
  summarizeUndo,
  type UndoActionFilter,
} from '../../history/model';

// 后端 list_undo 是「读整个 jsonl 再 take」——R310 之前这里写的「undo.jsonl 自带裁剪」
// 与事实不符，条数够到上限时靠下面的截断横幅说明。
const LOAD_LIMIT = 500;
// R310：台账条数够到上限时，页面此前把 500 当成「共 N 条」——更早的记录既看不见也
// 不说没加载。点「查看全部」按更大上限重拉一次；仍够到上限就只提示、不再给按钮。
// R388：后端 undo.jsonl 从此也有保留上限（diskpilot_executor::UNDO_LOG_MAX = 5000，
// 每轮清理后裁剪到最新 5000 条），与这里的 ALL_LIMIT 同值——「查看全部」看到的就是
// 台账里真实存在的全部记录，不会再有「更早的还在文件里但前端没加载」的隐性缺口。
const ALL_LIMIT = 5000;

/** R310：是否可能被拉取上限截断——恰好等量时保守视为可能还有更早记录（无 total 可查）。 */
export function historyTruncated(entries: readonly unknown[] | null, limit: number): boolean {
  return entries != null && limit > 0 && entries.length >= limit;
}

/** R149：时间戳 → 本地日期（YYYY-MM-DD）——台账范围文案用，不显示时刻。 */
function fmtDay(iso: string): string {
  const d = new Date(iso);
  if (!Number.isFinite(d.getTime())) return '';
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

const ACTION_FILTERS: UndoActionFilter[] = ['all', 'recycle', 'quarantine', 'delete'];

/** 过滤按钮文案：all 走 history 键，动作标签复用 cleanup 既有键（口径不漂移）。 */
function filterLabel(a: UndoActionFilter, t: ReturnType<typeof useT>): string {
  if (a === 'all') return t('history.filterAll');
  return a === 'recycle'
    ? t('cleanup.actionRecycle')
    : a === 'quarantine'
      ? t('cleanup.actionQuarantine')
      : t('cleanup.actionDelete');
}

// R73 列表体三态分支顺序核心不变量：读失败（loadErr）必须在空态之前判定——
// entries 折叠成 [] 时若 loadErr 在，显示「未读到」而不是「还没有清理记录」。
export type HistoryListState = 'loading' | 'unread' | 'empty' | 'nonempty';
export function historyListState(
  entries: readonly unknown[] | null,
  loadErr: string | null,
): HistoryListState {
  if (entries === null) return 'loading';
  if (loadErr) return 'unread';
  if (entries.length === 0) return 'empty';
  return 'nonempty';
}

export function HistoryPage({ onOpenCleanup }: { onOpenCleanup?: () => void } = {}) {
  const t = useT();
  const [entries, setEntries] = useState<UndoEntry[] | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [action, setAction] = useState<UndoActionFilter>('all');
  const [query, setQuery] = useState('');
  // R310：拉取上限可放宽（「查看全部」），默认 LOAD_LIMIT。
  const [limit, setLimit] = useState<number>(LOAD_LIMIT);
  const [busy, setBusy] = useState<number | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(() => {
    setLoadErr(null);
    api.listUndo(limit).then(setEntries).catch((e) => {
      setEntries([]);
      setLoadErr(t('history.loadFailed', { msg: String(e) }));
    });
  }, [t, limit]);

  // limit 放宽 → refresh 身份变化 → 自动重拉一次（无需额外按钮状态机）。
  useEffect(() => { refresh(); }, [refresh]);

  const summary = useMemo(() => summarizeUndo(entries ?? []), [entries]);
  const filtered = useMemo(
    () => filterUndoEntries(entries ?? [], { action, query }),
    [entries, action, query],
  );
  // R150：头部统计改为基于「当前筛选集」——筛选 recycle/关键词后，「共 N 条
  // · 累计释放 · 最早日期」应反映筛选结果而不是永远全量，否则统计与列表脱节。
  const filteredSummary = useMemo(() => summarizeUndo(filtered), [filtered]);
  const groups = useMemo(() => groupUndoByDay(filtered), [filtered]);
  const now = new Date();

  const restore = async (index: number, source: string) => {
    setBusy(index);
    setMsg(null);
    setErr(null);
    try {
      const restored = await api.undo(index, source);
      if (restored) {
        // R242：还原成功 → 刷新磁盘卡/趋势（scanSeq 驱动 listDrives 重拉）；
        // 与 UndoPanel 同一语义，真实后端隔离还原后空间回涨由 reclaimedBytes 覆盖。
        useStore.getState().bumpScanSeq();
        setMsg(t('cleanup.restored', { path: restored.source }));
        refresh();
      } else {
        setErr(t('cleanup.notRestorable'));
        refresh();
      }
    } catch (e) {
      setErr(t('cleanup.restoreFailed', { msg: String(e) }));
    } finally {
      setBusy(null);
    }
  };

  // 过滤后的展示序 ≠ 后端列表序：api.undo 按「后端位置 + source」双校验，
  // 恢复时用 entries.indexOf 反查真实位置。
  const backendIndex = (e: UndoEntry): number => (entries ?? []).indexOf(e);

  return (
      <div className="hp-shell">
        <WeeklyHealthCard onOpenCleanup={onOpenCleanup} />
        <div className="hp-head">
        <span className="hp-title"><History size={15} /> {t('history.title')}</span>
        {loadErr ? (
          <span className="muted small">{t('history.unread')}</span>
        ) : (
          <span className="muted small">
            {t('history.total', { n: summary.total })}
            {/* R150：有筛选时补一句「筛出 M 条」——否则「共 N 条」与下列
                列表长度对不上，用户会以为漏了记录。 */}
            {filtered.length !== summary.total && (
              <> · {t('history.filteredN', { n: filtered.length })}</>
            )}
          </span>
        )}
        {filteredSummary.bytesFreed != null && (
          <span className="hp-freed">{t('history.bytesFreed', { size: formatBytes(filteredSummary.bytesFreed) })}</span>
        )}
        {/* R149+R150：台账时间范围（按当前筛选集）；没读到就不显示 */}
        {filteredSummary.oldestAt != null && (
          <span className="muted small" title={t('history.rangeTitle', { from: fmtDay(filteredSummary.oldestAt), to: fmtDay(filteredSummary.newestAt ?? filteredSummary.oldestAt) })}>
            {t('history.rangeFrom', { date: fmtDay(filteredSummary.oldestAt) })}
          </span>
        )}
        <button type="button" className="ghost hp-refresh" onClick={refresh} title={t('history.refresh')}>
          <RotateCcw size={12} /> {t('history.refresh')}
        </button>
      </div>

      <div className="hp-filters">
        {ACTION_FILTERS.map((a) => (
          <button
            key={a}
            type="button"
            className={'hp-chip' + (action === a ? ' active' : '')} aria-pressed={action === a}
            onClick={() => setAction(a)}
          >
            {filterLabel(a, t)}
            <span className="hp-chip-n">{a === 'all' ? summary.total : summary.byAction[a]}</span>
          </button>
        ))}
        <label className="hp-search">
          <Search size={13} />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t('history.searchPlaceholder')}
          />
        </label>
      </div>

      {loadErr && <div className="error hp-msg" role="alert">{loadErr}</div>}

      {/* R310：条数够到拉取上限时明说——「共 N 条 / 最早日期」都只是已加载部分，
          不让人把 500 当全量台账。有“查看全部”→ 放宽上限重拉一次。 */}
      {historyTruncated(entries, limit) && (
        <div className="hp-truncated muted small">
          <span>{t('history.truncated', { n: limit })}</span>
          {limit < ALL_LIMIT && (
            <button type="button" className="ghost small" onClick={() => setLimit(ALL_LIMIT)} title={t('history.viewAllTitle')}>
              {t('history.viewAll')}
            </button>
          )}
        </div>
      )}

      {historyListState(entries, loadErr) === 'loading' ? (
        <div className="hp-empty"><Loader2 size={14} className="spin" /></div>
      ) : historyListState(entries, loadErr) === 'unread' ? (
        // 读失败 ≠ 空记录：不渲染「还没有清理记录」空态（标题已是「未读到」，
        // 错误横幅带原因），避免失败与空态并存。
        <div className="hp-empty muted small">{t('history.unreadBody')}</div>
      ) : historyListState(entries, loadErr) === 'empty' ? (
        <div className="hp-empty">{t('history.empty')}</div>
      ) : filtered.length === 0 ? (
        <div className="hp-empty">{t('history.emptyFiltered')}</div>
      ) : (
        groups.map((g) => (
          <section key={g.key || 'unknown'} className="hp-day">
            <div className="hp-day-head">
              <span>{dayGroupLabel(g.key, now, t)}</span>
              <span className="muted small">{t('history.dayCount', { n: g.items.length })}</span>
            </div>
            <ul className="undo-list hp-list">
              {g.items.map((e) => {
                const idx = backendIndex(e);
                return (
                  <li
                    key={`${e.timestamp}-${e.source}-${idx}`}
                    className={
                      'undo-item ' +
                      (e.action === 'recycle'
                        ? 'undo-tag-recycle'
                        : e.action === 'quarantine'
                          ? 'undo-tag-quarantine'
                          : 'undo-tag-delete')
                    }
                  >
                    <div className="undo-item-line">
                      <span className="undo-tag">{filterLabel(e.action, t)}</span>
                      <span className="undo-time">{fmtTime(e.timestamp)}</span>
                    </div>
                    <div className="undo-path" title={e.source}>{e.source}</div>
                    {e.bytes_freed != null && (
                      <div className="undo-reason">{t('cleanup.bytesFreed', { size: formatBytes(e.bytes_freed) })}</div>
                    )}
                    {e.reason && <div className="undo-reason">{e.reason}</div>}
                    {restoreKind(e) === 'inline' && (
                      <button
                        type="button"
                        className="ghost undo-restore"
                        disabled={busy === idx}
                        onClick={() => { void restore(idx, e.source); }}
                      >
                        {busy === idx
                          ? <><Loader2 size={11} className="spin" /> {t('cleanup.restoring')}</>
                          : <><RotateCcw size={11} /> {t('cleanup.restore')}</>}
                      </button>
                    )}
                    {restoreKind(e) === 'osRecycleBin' && (
                      <div className="undo-restore-hint">
                        <span className="muted small">{t('cleanup.recycleUndoHint')}</span>
                        <button
                          type="button"
                          className="ghost undo-restore"
                          onClick={() => {
                            setErr(null);
                            setMsg(null);
                            void api.openRecycleBin().catch((e2) => {
                              setErr(t('cleanup.openRecycleBinFailed', { msg: String(e2) }));
                            });
                          }}
                        >
                          {t('cleanup.openRecycleBin')}
                        </button>
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
        ))
      )}

      {msg && <div className="ok hp-msg" role="status">{msg}</div>}
      {err && <div className="error hp-msg" role="alert">{err}</div>}
    </div>
  );
}

function fmtTime(ts: string): string {
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return formatTime(d);
}
