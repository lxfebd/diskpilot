// 周巡检简报卡（health-butler P1）：历史页顶部回显最近一份周报 + 手动生成入口。
// 全只读展示：total_bytes（可清合计）/ temp_max_c（本轮最高温）/ hw_snapshots +
// hw_delta（本周硬件快照数与首尾差）。「生成」只调 generate_weekly_report_cmd——
// 后端同样是只读聚合（扫描建议 + 温度 + 硬件对比），绝不执行清理；真实清理
// 仍只走总览页确认窗（user_confirmed 硬校验）。
import { useCallback, useEffect, useMemo, useState } from 'react';
import { CalendarDays, Eraser, Flame, Loader2, ShieldAlert, ShieldCheck, ShieldQuestion, Sparkles, Thermometer } from 'lucide-react';
import { api } from '../../api';
import type { WeeklyReport } from '../../api';
import { formatBytes, formatDateKey } from '../../format';
import { TEMP_WARN_C } from '../../thresholds';
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

/** R151：巡检到期——周报按「周」节奏，距上次生成满 7 天即该重新生成。
 *  纯函数：返回 { due, daysLeft }；时间戳无效（≤0 或非有限）→ null（不编造
 *  到期日）。daysLeft 已钳到 ≥最初的差值，due=true 时 daysLeft 可能为 0。 */
export function nextInspectionDue(
  ts: number,
  nowSec: number,
): { due: boolean; daysLeft: number } | null {
  if (!Number.isFinite(ts) || ts <= 0) return null;
  const elapsed = Math.floor((nowSec - ts) / DAY_SECS);
  const left = 7 - elapsed;
  return { due: left <= 0, daysLeft: Math.max(0, left) };
}

/** R154：两份周报的间隔天数——环比结论的强度取决于对比跨度：间隔 7 天
 *  是标准周节奏，间隔 30 天则是「月度对比」，说成「本周 vs 上周」会误导。
 *  纯函数：任一 ts 无效 → null（不编造间隔）；同刻或倒序 → null。 */
export function weeklyGapDays(cur: WeeklyReport | null, prev: WeeklyReport | null): number | null {
  const a = cur?.ts ?? 0;
  const b = prev?.ts ?? 0;
  if (!Number.isFinite(a) || !Number.isFinite(b) || a <= 0 || b <= 0) return null;
  const gap = (a - b) / DAY_SECS;
  return gap > 0 ? gap : null;
}

/** R95 周报环比：cur 与 prev 两份都存在才对比，字段有一方缺失就返回 null（不编造）。 */
export interface WeeklyDelta {
  /** 可清理量差值：>0 更多可清，<0 更少，=0 持平。prev 无数据 → null。 */
  cleanBytes: number | null;
  /** 最高温差值：>0 更热，<0 更凉，=0 持平。任一缺失 → null。 */
  tempDelta: number | null;
}
export function weeklyDelta(cur: WeeklyReport | null, prev: WeeklyReport | null): WeeklyDelta | null {
  if (!cur || !prev) return null; // 没有上一份，无从对比
  const cleanBytes =
    prev.total_bytes > 0 ? cur.total_bytes - prev.total_bytes : null;
  const tempDelta =
    cur.temp_max_c != null && prev.temp_max_c != null
      ? cur.temp_max_c - prev.temp_max_c
      : null;
  return { cleanBytes, tempDelta };
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
  const [prevReport, setPrevReport] = useState<WeeklyReport | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // R95: 两份都存在才算 delta；任一缺失 → null（不显示对比区）。
  const delta = useMemo(() => weeklyDelta(report, prevReport), [report, prevReport]);
  // R154：环比间隔天数（给对比结论一个跨度参照）。
  const gapDays = useMemo(() => weeklyGapDays(report, prevReport), [report, prevReport]);

  const refresh = useCallback(() => {
    setLoadErr(null);
    // R95: 拉最近两份做环比；列表「最新在前」，list[0]=本周、list[1]=上周。
    api.getWeeklyReports(2)
      .then((list) => {
        setReport(list[0] ?? null);
        setPrevReport(list[1] ?? null);
      })
      .catch((e) => { setReport(null); setPrevReport(null); setLoadErr(String(e instanceof Error ? e.message : e)); })
      .finally(() => setLoaded(true));
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const generate = async () => {
    setBusy(true);
    try {
      const r = await api.generateWeeklyReport();
      // 新报告生成：旧报告顺位成上周（环比立刻可比，不用等下次 refresh）。
      setPrevReport(report);
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
      ) : loadErr !== null ? (
        <div className="wh-empty">{t('history.weekly.loadFailed', { msg: loadErr })}</div>
      ) : !report ? (
        <div className="wh-empty">{t('history.weekly.empty')}</div>
      ) : (
        <div className="wh-body">
          <div className="wh-stats">
            <div className="wh-stat">
              <Flame size={14} />
              <span>{t('history.weekly.total', { size: formatBytes(report.total_bytes) })}</span>
            </div>
            <div className={'wh-stat' + (report.temp_max_c != null && report.temp_max_c >= TEMP_WARN_C ? ' danger' : '')}>
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
                {' · '}
                {(() => {
                  const due = nextInspectionDue(report.ts, Date.now() / 1000);
                  if (!due) return null;
                  return due.due
                    ? <span className="wh-due">{t('history.weekly.dueNow')}</span>
                    : <span className="wh-due-ok">{t('history.weekly.dueIn', { n: due.daysLeft })}</span>;
                })()}
              </span>
            </div>
          </div>

          {/* R95 环比对比：两份都有才显示；只有一份 → 提示再生成一份；缺字段不编造。 */}
          {delta ? (
            <div className="wh-vs">
              <div className="wh-vs-title">
                {t('history.weekly.vsHead')}
                {/* R154：环比跨度——间隔 7 天是标准周节奏，间隔更久要说清 */}
                {gapDays !== null && gapDays >= 1 && (
                  <span className="muted small"> · {t('history.weekly.vsGap', { n: Math.round(gapDays) })}</span>
                )}
              </div>
              <ul className="wh-vs-list">
                {delta.cleanBytes === null ? (
                  <li className="muted small">{t('history.weekly.vsCleanNone')}</li>
                ) : (
                  <li className={delta.cleanBytes > 0 ? 'more' : delta.cleanBytes < 0 ? 'less' : ''}>
                    {delta.cleanBytes > 0
                      ? t('history.weekly.vsCleanMore', { size: formatBytes(delta.cleanBytes) })
                      : delta.cleanBytes < 0
                        ? t('history.weekly.vsCleanLess', { size: formatBytes(-delta.cleanBytes) })
                        : t('history.weekly.vsCleanSame')}
                  </li>
                )}
                {delta.tempDelta === null ? (
                  <li className="muted small">{t('history.weekly.vsTempNone')}</li>
                ) : (
                  <li className={delta.tempDelta > 0 ? 'more' : delta.tempDelta < 0 ? 'less' : ''}>
                    {delta.tempDelta > 0
                      ? t('history.weekly.vsTempUp', { delta: Math.round(delta.tempDelta) })
                      : delta.tempDelta < 0
                        ? t('history.weekly.vsTempDown', { delta: Math.round(-delta.tempDelta) })
                        : t('history.weekly.vsTempSame')}
                  </li>
                )}
              </ul>
            </div>
          ) : (
            <div className="wh-vs">
              <div className="wh-vs-title">{t('history.weekly.vsHead')}</div>
              <p className="muted small" style={{ margin: 0 }}>{t('history.weekly.vsNone')}</p>
            </div>
          )}

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
              旧归档没有 disks：整块不显示，不猜。
              R86：生成时盘健康读失败 → disks_note 带原因，显示「读失败」而
              不是把它伪装成「旧格式没数据」（整块消失）。 */}
          {disks.length === 0 && report.disks_note ? (
            <div className="wh-disks">
              <div className="wh-delta-title">{t('history.weekly.diskHead')}</div>
              <div className="wh-disks-ok">
                <ShieldQuestion size={12} />
                <span className="muted small">{report.disks_note}</span>
              </div>
            </div>
          ) : disks.length > 0 && (
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
                        {/* R440：磁盘型号/别名会被 ellipsis 截断，悬停给出完整名字 */}
                        <span className="wh-disks-name" title={d.letters ? `${d.letters}: ${d.friendly}` : d.friendly}>
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

          {/* R486：扫描趋势（后端已格式化好的行）。旧归档没有 scan_trends 字段 → 整块不显示，不猜。 */}
          {report.scan_trends !== undefined && (
            <div className="wh-delta">
              <div className="wh-delta-title">{t('history.weekly.scanTrend')}</div>
              {report.scan_trends.length > 0 ? (
                <ul className="wh-delta-list">
                  {report.scan_trends.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ul>
              ) : (
                <div className="muted small">{t('history.weekly.scanTrendNone')}</div>
              )}
            </div>
          )}

          {/* R486：观测层异常（损坏行 / 读取失败的域）。字段存在才代表后端真读到过观测层。 */}
          {report.obs_alerts !== undefined && (
            <div className="wh-delta">
              <div className="wh-delta-title">{t('history.weekly.obsAlerts')}</div>
              {report.obs_alerts.length > 0 ? (
                <ul className="wh-delta-list">
                  {report.obs_alerts.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ul>
              ) : (
                <div className="muted small">{t('history.weekly.obsNone')}</div>
              )}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
