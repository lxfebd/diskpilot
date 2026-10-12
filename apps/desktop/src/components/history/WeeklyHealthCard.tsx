// 周巡检简报卡（health-butler P1）：历史页顶部回显最近一份周报 + 手动生成入口。
// 全只读展示：total_bytes（可清合计）/ temp_max_c（本轮最高温）/ hw_snapshots +
// hw_delta（本周硬件快照数与首尾差）。「生成」只调 generate_weekly_report_cmd——
// 后端同样是只读聚合（扫描建议 + 温度 + 硬件对比），绝不执行清理；真实清理
// 仍只走总览页确认窗（user_confirmed 硬校验）。
import { useCallback, useEffect, useState } from 'react';
import { CalendarDays, Eraser, Flame, Loader2, ShieldAlert, ShieldCheck, ShieldQuestion, Sparkles, Thermometer } from 'lucide-react';
import { api } from '../../api';
import type { WeeklyReport } from '../../api';
import { formatBytes, formatDateKey } from '../../format';
import { useStore } from '../../store';
import { useT } from '../../i18n';

const DAY_SECS = 86400;

/** 距今天数文案（「刚刚」/「N 天前」/具体日期）。纯函数便于测试。 */
export function reportAgeLabel(ts: number, nowSec: number, t: ReturnType<typeof useT>): string {
  if (ts <= 0) return '';
  const days = Math.floor((nowSec - ts) / DAY_SECS);
  if (days <= 0) return t('history.weekly.justNow');
  if (days <= 30) return t('history.weekly.daysAgo', { n: days });
  return formatDateKey(new Date(ts * 1000));
}

export function WeeklyHealthCard({
  onGenerated,
  onOpenCleanup,
}: {
  onGenerated?: () => void;
  onOpenCleanup?: () => void;
}) {
  const t = useT();
  const [report, setReport] = useState<WeeklyReport | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    api.getWeeklyReports(1)
      .then((list) => { setReport(list[0] ?? null); })
      .catch(() => { setReport(null); })
      .finally(() => setLoaded(true));
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const generate = async () => {
    setBusy(true);
    try {
      const r = await api.generateWeeklyReport();
      setReport(r);
      useStore.getState().toast(t('history.weekly.generated', { size: formatBytes(r.total_bytes) }), 'ok');
      onGenerated?.();
    } catch (e) {
      useStore.getState().toast(
        t('history.weekly.genFailed', { err: String(e instanceof Error ? e.message : e) }),
        'err',
      );
    } finally {
      setBusy(false);
    }
  };

  // 盘健康摘要：旧归档没有 disks → 空数组，整块不显示（不猜盘是好是坏）。
  const disks = report?.disks ?? [];
  const riskyDisks = disks.filter((d) => d.grade === 'caution' || d.grade === 'bad');
  const unknownDisks = disks.filter((d) => d.grade === 'unknown');

  return (
    <section className="wh-card">
      <div className="wh-head">
        <Sparkles size={14} className="wh-icon" />
        <b>{t('history.weekly.title')}</b>
        <span className="muted small">{t('history.weekly.subtitle')}</span>
        <button type="button" className="ghost wh-gen" onClick={() => { void generate(); }} disabled={busy}>
          {busy
            ? <><Loader2 size={12} className="spin" /> {t('history.weekly.generating')}</>
            : <><CalendarDays size={12} /> {t('history.weekly.generate')}</>}
        </button>
      </div>

      {!loaded ? (
        <div className="wh-loading"><Loader2 size={14} className="spin" /></div>
      ) : !report ? (
        <div className="wh-empty">{t('history.weekly.empty')}</div>
      ) : (
        <div className="wh-body">
          <div className="wh-stats">
            <div className="wh-stat">
              <Flame size={14} />
              <span>{t('history.weekly.total', { size: formatBytes(report.total_bytes) })}</span>
            </div>
            <div className={'wh-stat' + (report.temp_max_c != null && report.temp_max_c >= 80 ? ' danger' : '')}>
              <Thermometer size={14} />
              <span>
                {report.temp_max_c != null
                  ? t('history.weekly.temp', { temp: Math.round(report.temp_max_c) })
                  : t('history.weekly.tempNone')}
              </span>
            </div>
            <div className="wh-stat muted">
              <CalendarDays size={14} />
              <span>
                {reportAgeLabel(report.ts, Date.now() / 1000, t)}
                {' · '}
                {t('history.weekly.snapshots', { n: report.hw_snapshots })}
              </span>
            </div>
          </div>

          {/* 周报说「可清 X GB」却给不出出口，等于只读了一半：这里直接跳到清理页，
              真实清理仍走清理页的确认窗（周报本身依旧只读）。 */}
          {report.total_bytes > 0 && onOpenCleanup && (
            <button type="button" className="ghost small" onClick={onOpenCleanup}>
              <Eraser size={11} /> {t('history.weekly.goCleanup')}
            </button>
          )}

          {report.drives.length > 0 && (
            <ul className="wh-drives">
              {report.drives.map((d) => (
                <li key={d.path} className="wh-drive">
                  <span className="wh-drive-path">{d.path}</span>
                  <span className="wh-drive-size">{formatBytes(d.bytes)}</span>
                </li>
              ))}
            </ul>
          )}

          {/* 周报只报「可清多少」却不报盘的健康，等于把最要紧的一件事漏掉了。
              旧归档没有 disks：整块不显示，不猜。 */}
          {disks.length > 0 && (
            <div className="wh-disks">
              <div className="wh-delta-title">{t('history.weekly.diskHead')}</div>
              {riskyDisks.length > 0 ? (
                <>
                  <ul className="wh-disks-list">
                    {riskyDisks.map((d, i) => (
                      <li
                        key={`${d.friendly}-${i}`}
                        className={d.grade === 'bad' ? 'wh-disks-item bad' : 'wh-disks-item caution'}
                      >
                        <ShieldAlert size={12} />
                        <span className="wh-disks-name">
                          {d.letters ? `${d.letters}: ${d.friendly}` : d.friendly}
                        </span>
                        <span className="wh-disks-grade" title={d.health}>
                          {t(d.grade === 'bad' ? 'hw.healthBad' : 'hw.healthCaution')}
                        </span>
                      </li>
                    ))}
                  </ul>
                  {disks.length > riskyDisks.length && (
                    <div className="muted small">
                      {t('history.weekly.diskRest', { n: disks.length - riskyDisks.length })}
                    </div>
                  )}
                </>
              ) : unknownDisks.length === disks.length ? (
                <div className="wh-disks-ok">
                  <ShieldQuestion size={12} />
                  {t('history.weekly.diskNoneReported', { m: unknownDisks.length })}
                </div>
              ) : unknownDisks.length > 0 ? (
                <div className="wh-disks-ok">
                  <ShieldQuestion size={12} />
                  {t('history.weekly.diskMixed', {
                    n: disks.length - unknownDisks.length,
                    m: unknownDisks.length,
                  })}
                </div>
              ) : (
                <div className="wh-disks-ok">
                  <ShieldCheck size={12} />
                  {t('history.weekly.diskAllGood', { n: disks.length })}
                </div>
              )}
            </div>
          )}

          <div className="wh-delta">
            <div className="wh-delta-title">{t('history.weekly.delta')}</div>
            {report.hw_delta.length > 0 ? (
              <ul className="wh-delta-list">
                {report.hw_delta.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            ) : (
              <div className="muted small">{t('history.weekly.deltaNone')}</div>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
