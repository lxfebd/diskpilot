import { useCallback, useEffect, useState } from 'react';
import { RotateCcw, History, Loader2 } from 'lucide-react';
import { api } from '../api';
import { useT } from '../i18n';
import { formatBytes } from '../format';
import type { UndoEntry } from '../types';

/// Recent-cleanup panel (P0 #3 undo UX).
///
/// Shows the last cleanup actions recorded in undo.jsonl, newest first.
/// Quarantine entries are restorable inline; recycle/delete entries are
/// shown for transparency but carry no restore action here — recycled files
/// come back from the OS recycle bin, deleted ones are gone by design.
function fmtTime(ts: string): string {
  // ts is RFC3339 from chrono. Keep it short & local.
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getMonth() + 1}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 文案函数类型（useT() 的返回值）：模块级纯函数按这个签名收 t，保证在渲染时求值。 */
type TFunc = ReturnType<typeof useT>;

function actionLabel(a: UndoEntry['action'], t: TFunc): string {
  switch (a) {
    case 'recycle': return t('cleanup.actionRecycle');
    case 'quarantine': return t('cleanup.actionQuarantine');
    case 'delete': return t('cleanup.actionDelete');
  }
}

const ACTION_CLASS: Record<UndoEntry['action'], string> = {
  recycle: 'undo-tag-recycle',
  quarantine: 'undo-tag-quarantine',
  delete: 'undo-tag-delete',
};

export function UndoPanel() {
  const t = useT();
  const [entries, setEntries] = useState<UndoEntry[] | null>(null);
  const [busy, setBusy] = useState<number | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api.listUndo(50).then(setEntries).catch(() => setEntries([]));
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

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

  if (entries === null) {
    return (
      <div className="undo-panel">
        <div className="studio-head">
          <span><History size={13} /> {t('cleanup.recent')}</span>
          <Loader2 size={12} className="spin" />
        </div>
      </div>
    );
  }

  if (entries.length === 0) return null;

  return (
    <div className="undo-panel">
      <div className="studio-head">
        <span><History size={13} /> {t('cleanup.recent')}</span>
        <span className="muted small">{t('cleanup.entriesCount', { n: entries.length })}</span>
      </div>
      <ul className="undo-list">
        {entries.map((e, i) => (
          <li key={`${e.timestamp}-${e.source}-${i}`} className={'undo-item ' + ACTION_CLASS[e.action]}>
            <div className="undo-item-line">
              <span className="undo-tag">{actionLabel(e.action, t)}</span>
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
                disabled={busy === i}
                onClick={() => restore(i, e.source)}
              >
                {busy === i
                  ? <><Loader2 size={11} className="spin" /> {t('cleanup.restoring')}</>
                  : <><RotateCcw size={11} /> {t('cleanup.restore')}</>}
              </button>
            )}
          </li>
        ))}
      </ul>
      {msg && <div className="ok undo-msg">{msg}</div>}
      {err && <div className="error undo-msg">{err}</div>}
    </div>
  );
}