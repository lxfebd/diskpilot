import { useCallback, useEffect, useState } from 'react';
import { RotateCcw, History, Loader2 } from 'lucide-react';
import { api } from '../api';
import { restoreKind } from '../history/model';
import { useStore } from '../store';
import { useT } from '../i18n';
import { formatBytes, formatMonthDay, formatTime } from '../format';
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
  return `${formatMonthDay(d)} ${formatTime(d)}`;
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

export function UndoPanel({ onOpenHistory }: { onOpenHistory?: () => void }) {
  const t = useT();
  const [entries, setEntries] = useState<UndoEntry[] | null>(null);
  const [busy, setBusy] = useState<number | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);

  const refresh = useCallback(() => {
    setLoadErr(null);
    api.listUndo(50).then(setEntries).catch((e) => {
      // 读取失败 ≠ 没有记录：如实显示读不到，不能伪装成空列表消失。
      setEntries([]);
      setLoadErr(String(e instanceof Error ? e.message : e));
    });
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const restore = async (index: number, source: string) => {
    setBusy(index);
    setMsg(null);
    setErr(null);
    try {
      const restored = await api.undo(index, source);
      if (restored) {
        // R242：还原成功 → 刷新磁盘卡/趋势（scanSeq 驱动 listDrives 重拉）。
        // 真实后端隔离还原后空间回涨，reclaimedBytes 订阅已覆盖；即便种子
        // 条目未入账（空间不变），也让磁盘卡反映「台账已变」的当前值。
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

  // 读取失败：如实说「没读到」并给出原因——绝不能跟「真没记录」一样整个消失。
  if (loadErr !== null) {
    return (
      <div className="undo-panel">
        <div className="studio-head">
          <span><History size={13} /> {t('cleanup.recent')}</span>
          <span className="muted small">{t('cleanup.loadFailedShort')}</span>
        </div>
        <div className="error undo-msg" role="alert">{t('cleanup.loadFailedMsg', { msg: loadErr })}</div>
        <button type="button" className="ghost" onClick={refresh}>{t('cleanup.retry')}</button>
      </div>
    );
  }

  if (entries.length === 0) return null;

  return (
    <div className="undo-panel">
      <div className="studio-head">
        <span><History size={13} /> {t('cleanup.recent')}</span>
        <span className="muted small">{t('cleanup.entriesCount', { n: entries.length })}</span>
        {onOpenHistory && (
          <button type="button" className="ghost" onClick={onOpenHistory}>
            {t('cleanup.viewAll')}
          </button>
        )}
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
            {restoreKind(e) === 'inline' && (
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
        ))}
      </ul>
      {msg && <div className="ok undo-msg" role="status">{msg}</div>}
      {err && <div className="error undo-msg" role="alert">{err}</div>}
    </div>
  );
}