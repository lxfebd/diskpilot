// R537 持久化通知中心区块：历史页顶部展示（周报/空间预警/扫描提醒/自动导出），localStorage 持久化。
// R547：按类型筛选 tab（全部 + 四类），aria-pressed 表当前筛选。
import { useEffect, useState } from 'react';
import { Bell, X } from 'lucide-react';
import { useT } from '../../i18n';
import { loadNotices, clearNotices, filterNotices, NOTICES_KEY, NOTICE_EVENT, type Notice, type NoticeFilter } from '../../notices';

const KIND_KEY: Record<Notice['kind'], string> = {
  weekly: 'shell.notices.kindWeekly',
  'space-full': 'shell.notices.kindSpaceFull',
  'scan-stale': 'shell.notices.kindScanStale',
  'report-exported': 'shell.notices.kindReportExported',
};

const FILTERS: NoticeFilter[] = ['all', 'weekly', 'space-full', 'scan-stale', 'report-exported'];

export function NoticesBlock() {
  const t = useT();
  const [notices, setNotices] = useState<Notice[]>(() => loadNotices());
  const [filter, setFilter] = useState<NoticeFilter>('all');
  // R549：storage 事件只发给其他同源文档，同文档内的新通知靠 NOTICE_EVENT 刷新
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === NOTICES_KEY) setNotices(loadNotices());
    };
    const onNotice = () => setNotices(loadNotices());
    window.addEventListener('storage', onStorage);
    window.addEventListener(NOTICE_EVENT, onNotice);
    return () => {
      window.removeEventListener('storage', onStorage);
      window.removeEventListener(NOTICE_EVENT, onNotice);
    };
  }, []);
  if (notices.length === 0) return null;
  const shown = filterNotices(notices, filter).slice(0, 5);
  return (
    <section className="hp-notices" aria-label={t('shell.notices.title')}>
      <div className="hp-notices-head">
        <span className="hp-notices-title"><Bell size={14} /> {t('shell.notices.title')}</span>
        <button
          type="button"
          className="ghost small"
          onClick={() => { clearNotices(); setNotices([]); }}
        >
          <X size={12} /> {t('shell.notices.clear')}
        </button>
      </div>
      {/* R547 类型筛选 tab */}
      <div className="hp-notices-tabs" role="group" aria-label={t('shell.notices.filterAll')}>
        {FILTERS.map((f) => (
          <button
            key={f}
            type="button"
            className={'hp-notices-tab' + (filter === f ? ' on' : '')}
            aria-pressed={filter === f}
            onClick={() => setFilter(f)}
          >
            {f === 'all' ? t('shell.notices.filterAll') : t(KIND_KEY[f])}
          </button>
        ))}
      </div>
      {shown.length === 0 && (
        <div className="hp-notice-row muted small">{t('shell.notices.filterEmpty')}</div>
      )}
      {shown.map((n) => (
        <div key={n.id} className="hp-notice-row">
          <span className="hp-notice-kind">{t(KIND_KEY[n.kind])}</span>
          <span className="hp-notice-body" title={n.body}>{n.title} — {n.body}</span>
          <span className="muted small">{new Date(n.at).toLocaleString()}</span>
        </div>
      ))}
    </section>
  );
}
