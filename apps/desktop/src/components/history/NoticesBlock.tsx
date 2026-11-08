// R537 持久化通知中心区块：历史页顶部展示（周报/空间预警/扫描提醒/自动导出），localStorage 持久化。
import { useEffect, useState } from 'react';
import { Bell, X } from 'lucide-react';
import { useT } from '../../i18n';
import { loadNotices, clearNotices, NOTICES_KEY, type Notice } from '../../notices';

const KIND_KEY: Record<Notice['kind'], string> = {
  weekly: 'shell.notices.kindWeekly',
  'space-full': 'shell.notices.kindSpaceFull',
  'scan-stale': 'shell.notices.kindScanStale',
  'report-exported': 'shell.notices.kindReportExported',
};

export function NoticesBlock() {
  const t = useT();
  const [notices, setNotices] = useState<Notice[]>(() => loadNotices());
  // 同页其它入口写入后靠 storage 事件刷新（多窗口/同页重复挂载也能看到最新）
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === NOTICES_KEY) setNotices(loadNotices());
    };
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }, []);
  if (notices.length === 0) return null;
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
      {notices.slice(0, 5).map((n) => (
        <div key={n.id} className="hp-notice-row">
          <span className="hp-notice-kind">{t(KIND_KEY[n.kind])}</span>
          <span className="hp-notice-body" title={n.body}>{n.title} — {n.body}</span>
          <span className="muted small">{new Date(n.at).toLocaleString()}</span>
        </div>
      ))}
    </section>
  );
}
