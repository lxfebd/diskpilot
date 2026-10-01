// 操作历史整页（P2c）：undo.jsonl 全量台账。Studio 侧栏的 UndoPanel 只看
// 最近 50 条，这里取后端上限内的全量 + 动作过滤 + 关键词搜索 + 按天分组；
// 隔离条目可原地恢复（与 UndoPanel 同一 api.undo 双校验语义：index + source）。
import { useCallback, useEffect, useMemo, useState } from 'react';
import { History, Loader2, RotateCcw, Search } from 'lucide-react';
import { api } from '../../api';
import { useT } from '../../i18n';
import { formatBytes } from '../../format';
import type { UndoEntry } from '../../types';
import { WeeklyHealthCard } from './WeeklyHealthCard';
import {
  dayGroupLabel,
  filterUndoEntries,
  groupUndoByDay,
  summarizeUndo,
  type UndoActionFilter,
} from '../../history/model';

// 后端 list_undo 在 take 上限内尽量多取；undo.jsonl 自带裁剪，正常远小于此。
const LOAD_LIMIT = 500;

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

export function HistoryPage() {
  const t = useT();
  const [entries, setEntries] = useState<UndoEntry[] | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [action, setAction] = useState<UndoActionFilter>('all');
  const [query, setQuery] = useState('');
  const [busy, setBusy] = useState<number | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(() => {
    setLoadErr(null);
    api.listUndo(LOAD_LIMIT).then(setEntries).catch((e) => {
      setEntries([]);
      setLoadErr(t('history.loadFailed', { msg: String(e) }));
    });
  }, [t]);

  useEffect(() => { refresh(); }, [refresh]);

  const summary = useMemo(() => summarizeUndo(entries ?? []), [entries]);
  const filtered = useMemo(
    () => filterUndoEntries(entries ?? [], { action, query }),
    [entries, action, query],
  );
  const groups = useMemo(() => groupUndoByDay(filtered), [filtered]);
  const now = new Date();

  const restore = async (index: number, source: string) => {
    setBusy(index);
    setMsg(null);
    setErr(null);
    try {
      const restored = await api.undo(index, source);
      if (restored) {
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
        <WeeklyHealthCard />
        <div className="hp-head">
        <span className="hp-title"><History size={15} /> {t('history.title')}</span>
        <span className="muted small">{t('history.total', { n: summary.total })}</span>
        {summary.bytesFreed != null && (
          <span className="hp-freed">{t('history.bytesFreed', { size: formatBytes(summary.bytesFreed) })}</span>
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
            className={'hp-chip' + (action === a ? ' active' : '')}
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

      {loadErr && <div className="error hp-msg">{loadErr}</div>}

      {entries === null ? (
        <div className="hp-empty"><Loader2 size={14} className="spin" /></div>
      ) : entries.length === 0 ? (
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
                    {e.action === 'quarantine' && (
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
                  </li>
                );
              })}
            </ul>
          </section>
        ))
      )}

      {msg && <div className="ok hp-msg">{msg}</div>}
      {err && <div className="error hp-msg">{err}</div>}
    </div>
  );
}

function fmtTime(ts: string): string {
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
