import { useEffect, useState } from 'react';
import { RefreshCw, DownloadCloud, Fan, AlertTriangle, Search } from 'lucide-react';
import { api, type FanCurveAdvice, type FanStatus, type InstalledDriver, type DriverUpdate } from '../api';
import {
  FAN_SNAP_KEY,
  fanAdviceDelta,
  formatFanSnapshot,
  parseFanSnapshot,
  type FanSnapshot,
} from '../fanHistory';
import { isPermEnabled } from '../permissions';
import { isTauri } from '../env';
import { ErrorBoundary } from './ErrorBoundary';
import { listen } from '@tauri-apps/api/event';
import { useT } from '../i18n';
import { StartupPanel } from './StartupPanel';
import { FAN_LEVEL_COLORS } from '../colors';
import { useFlashText } from '../flash';

// 风扇调速档位 → 语义色（urgent 红 → balanced 绿，未知灰）。
// R108：此前在本文件硬编码 9 个 hex，是 colors.ts（W4 单源）之外的散板——
// 移入 colors.ts 的 FAN_LEVEL_COLORS，全仓只此一处取色。
const LEVEL_COLOR = FAN_LEVEL_COLORS;

/** 风扇可调通道三态（R71）：
 *  - 'ok'：可写通道在位（writable=true）；
 *  - 'unread'：状态探测失败——≠「无通道」，绝不可渲染成 channelNone；
 *  - 'none'：真空（探测成功但无可用通道）。
 *  让 JSX 分支与文案一一对应，读失败永不伪装成「无可用调速通道」。 */
export type FanChannelState = 'ok' | 'unread' | 'none';
export function fanChannelState(status: FanStatus | null, statusErr: string | null): FanChannelState {
  if (status?.writable) return 'ok';
  if (statusErr) return 'unread';
  return 'none';
}

/** 设置页「系统工具」标签：启动项管理 / 风扇曲线建议 / 驱动更新检查。
 *  三个能力都是 L2（权限中心开关 + 每次执行确认），组件内做两步确认，
 *  删除/禁用的破坏性操作绝不直接执行。 */
/** R166：已装驱动筛选——200 条上限下列表很长，按名找很难；搜索匹配
 *  name/provider/version，分类筛按 d.class。纯函数：空词 + null 分类原样返回。 */
export function filterDrivers(
  list: readonly InstalledDriver[],
  query: string,
  cls: string | null,
): InstalledDriver[] {
  const q = query.trim().toLowerCase();
  return list.filter((d) => {
    if (cls !== null && (d.class ?? '') !== cls) return false;
    if (!q) return true;
    return (
      (d.name ?? '').toLowerCase().includes(q) ||
      (d.provider ?? '').toLowerCase().includes(q) ||
      (d.version ?? '').toLowerCase().includes(q)
    );
  });
}

/** R166：分类清单 + 计数（供筛选 chips；按名称升序稳定，空分类归到 '' 键）。 */
export function driverClassCounts(list: readonly InstalledDriver[]): { cls: string; n: number }[] {
  const m = new Map<string, number>();
  for (const d of list) {
    const k = d.class ?? '';
    m.set(k, (m.get(k) ?? 0) + 1);
  }
  return [...m.entries()]
    .map(([cls, n]) => ({ cls, n }))
    .sort((a, b) => b.n - a.n || a.cls.localeCompare(b.cls));
}

/** R167：Windows Update 可选更新里 `is_driver` 一直没被用过——驱动更新与
 *  语言包/功能更新混在一起，用户常只想看驱动。纯函数：只挑 is_driver 的。 */
export function driverOnlyUpdates(list: readonly DriverUpdate[]): DriverUpdate[] {
  return list.filter((u) => u.is_driver);
}

/** R167：可选更新计数——「共 N 项（其中驱动 M 项）」。 */
export function wuUpdateCounts(list: readonly DriverUpdate[]): { total: number; drivers: number } {
  return { total: list.length, drivers: list.filter((u) => u.is_driver).length };
}

export function SystemTools() {
  return (
    <ErrorBoundary>
      <SystemToolsInner />
    </ErrorBoundary>
  );
}

function SystemToolsInner() {
  const t = useT();
  const [fan, setFan] = useState<FanCurveAdvice | null>(null);
  // R173：上一次风扇建议快照（localStorage 持久化），用于「变了没」对比。
  const [prevFan, setPrevFan] = useState<FanSnapshot | null>(null);
  const [fanErr, setFanErr] = useState<string | null>(null);
  const [fanLoading, setFanLoading] = useState(false);
  // 风扇控制（fan.control，L2）：状态 + 档位 + 确认 + 熔断通知。
  const [fanStatus, setFanStatus] = useState<FanStatus | null>(null);
  // fanStatus 读失败单独记：探测失败 ≠ 「无通道」（控制区会禁用，但标签不能
  // 说成"当前无可用调速通道"——那是把读失败伪装成不存在）。
  const [fanStatusErr, setFanStatusErr] = useState<string | null>(null);
  const [fanCtrlErr, setFanCtrlErr] = useState<string | null>(null);
  const [fanLevel, setFanLevel] = useState<string>('balanced');
  const [fanCtrlBusy, setFanCtrlBusy] = useState(false);
  const [fanConfirming, setFanConfirming] = useState(false);
  const [fanRestoring, setFanRestoring] = useState(false);
  const [fanRestoreAsk, setFanRestoreAsk] = useState(false);
  const [fanBreakerNote, setFanBreakerNote] = useState<string | null>(null);

  const [drivers, setDrivers] = useState<InstalledDriver[] | null>(null);
  const [updates, setUpdates] = useState<DriverUpdate[] | null>(null);
  const [drvErr, setDrvErr] = useState<string | null>(null);
  const [drvLoading, setDrvLoading] = useState(false);
  const [wuLoading, setWuLoading] = useState(false);
  // R166：已装驱动搜索 + 分类筛选（200 条上限下列表很长）。
  const [drvQuery, setDrvQuery] = useState('');
  const [drvClass, setDrvClass] = useState<string | null>(null);
  // R167：可选更新只看驱动（Windows Update 混了语言包/功能更新）。
  const [wuDriverOnly, setWuDriverOnly] = useState(false);
  // R166：筛选结果与分类计数（渲染只读）。
  const visibleDrivers = filterDrivers(drivers ?? [], drvQuery, drvClass);
  const classCounts = driverClassCounts(drivers ?? []);
  // R167：可选更新的计数与「仅驱动」筛选结果。
  const wuCounts = wuUpdateCounts(updates ?? []);
  const visibleUpdates = wuDriverOnly ? driverOnlyUpdates(updates ?? []) : (updates ?? []);
  // R173：与上次风扇建议对比（档位是否抬升 / 温度差）。
  const fanDelta = fan ? fanAdviceDelta(
    { level: fan.level, levelLabel: fan.level_label, tempMaxC: fan.temp_max_c },
    prevFan,
  ) : null;

  // R358：提示文案统一走 useFlashText（见 flash.ts）。
  const [msg, flashMsg] = useFlashText();

  const fanPerm = isPermEnabled('fan.adjust');
  const fanCtrlPerm = isPermEnabled('fan.control');
  const driverPerm = isPermEnabled('driver.check');

  const loadFan = async () => {
    setFanLoading(true);
    setFanErr(null);
    try {
      const advice = await api.fanCurveAdvice();
      setFan(advice);
      // R173：读一次记一次快照——下次对照「变了没」。读到失败不记（保持上次）。
      const snap: FanSnapshot = {
        level: advice.level,
        levelLabel: advice.level_label,
        tempMaxC: advice.temp_max_c,
        atMs: Date.now(),
      };
      setPrevFan(parseFanSnapshot(localStorage.getItem(FAN_SNAP_KEY)));
      localStorage.setItem(FAN_SNAP_KEY, formatFanSnapshot(snap));
    } catch (e) {
      setFanErr(String(e instanceof Error ? e.message : e));
    } finally {
      setFanLoading(false);
    }
  };

  const loadFanStatus = async () => {
    try {
      setFanStatus(await api.fanStatus());
      setFanStatusErr(null);
    } catch (e) {
      // 只读状态读不到不致命（控制按钮禁用即可），但不能把「读失败」说成
      // 「无可用通道」——状态未知 ≠ 不存在。
      setFanStatusErr(String(e instanceof Error ? e.message : e));
    }
  };

  const applyFanControl = async () => {
    // R263：执行时权限门（fan.control L2 高危，写风扇档位）——UI 之外也拦。
    if (!isPermEnabled('fan.control')) {
      setFanCtrlErr(t('system.fan.permOff'));
      return;
    }
    try {
      setFanCtrlBusy(true);
      const res = await api.fanControl(fanLevel, undefined, 90, true);
      flashMsg(res.note || (res.ok ? t('system.fan.toastApplied') : t('system.fan.toastApplyFailed')));
      if (res.applied) {
        setFanBreakerNote(null);
        await loadFanStatus();
      }
    } catch (e) {
      setFanCtrlErr(String(e instanceof Error ? e.message : e));
    } finally {
      setFanCtrlBusy(false);
      setFanConfirming(false);
    }
  };

  const restoreDefaultFan = async () => {
    // R263：执行时权限门（恢复默认风扇曲线也是写路径，fan.adjust 门）。
    if (!isPermEnabled('fan.adjust')) {
      setFanCtrlErr(t('system.fan.permOff'));
      return;
    }
    try {
      setFanRestoring(true);
      const res = await api.fanCurveApply(true, true);
      flashMsg(res.note || (res.ok ? t('system.fan.toastRestored') : t('system.fan.toastRestoreFailed')));
      if (res.applied) await loadFanStatus();
    } catch (e) {
      setFanCtrlErr(String(e instanceof Error ? e.message : e));
    } finally {
      setFanRestoring(false);
      setFanRestoreAsk(false);
    }
  };

  const loadDrivers = async () => {
    setDrvLoading(true);
    setDrvErr(null);
    try {
      setDrivers(await api.listInstalledDrivers());
    } catch (e) {
      setDrvErr(String(e instanceof Error ? e.message : e));
    } finally {
      setDrvLoading(false);
    }
  };

  const checkWuUpdates = async () => {
    // R263：执行时权限门（driver.check L2，联网检查并返回可更新驱动）。
    if (!isPermEnabled('driver.check')) {
      setDrvErr(t('system.driver.permOff'));
      return;
    }
    setWuLoading(true);
    setDrvErr(null);
    try {
      const list = await api.checkDriverUpdates();
      setUpdates(list);
      if (list.length === 0) flashMsg(t('system.driver.wuEmpty'));
    } catch (e) {
      setDrvErr(String(e instanceof Error ? e.message : e));
    } finally {
      setWuLoading(false);
    }
  };

  useEffect(() => {
    if (fanPerm) loadFan();
    if (driverPerm) loadDrivers();
  }, [fanPerm, driverPerm]);

  // 风扇控制状态轮询：仅当 fan.control 权限开启且本页可见时跑（5s 门控）。
  useEffect(() => {
    if (!fanCtrlPerm) return;
    const tick = () => {
      if (document.visibilityState === 'visible') loadFanStatus();
    };
    tick();
    const id = setInterval(tick, 5000);
    return () => clearInterval(id);
  }, [fanCtrlPerm]);

  // 熔断事件：后端温度超阈值自动回退时前端提示。
  useEffect(() => {
    if (!fanCtrlPerm || !isTauri) return;
    let un: (() => void) | undefined;
    listen<{ note: string; temp_c: number }>('fan-temp-breaker', (ev) => {
      const d = ev.payload;
      setFanBreakerNote(d.note ?? t('system.fan.breakerFallback', { temp: d.temp_c }));
      void loadFanStatus();
    }).then((fn) => { un = fn; });
    return () => un?.();
  }, [fanCtrlPerm, t]);

  return (
    // R365：作为设置窗「分栏」的内容面板（tabpanel），id 与 Settings.tsx 的 tab 一一对应。
    <div className="settings-pane sys-tools" role="tabpanel" id="settings-pane-system" aria-labelledby="settings-tab-system">
      {msg && <div className="ok" role="status">{msg}</div>}

      {/* ── 启动项管理（P4-4 抽取为共用面板）── */}
      <StartupPanel />

      {/* ── 风扇曲线建议 + 风扇控制 ── */}
      <div className="settings-group-title" style={{ marginTop: 22 }}>{t('system.fan.title')}</div>
      <div className="hint" style={{ marginBottom: 8 }}>
        <span>
          {t('system.fan.hint')}
          {fanCtrlPerm ? (
            <> {t('system.fan.hintCtrlOn')}</>
          ) : (
            <> {t('system.fan.hintCtrlOff')}</>
          )}
          {!fanPerm && (
            <strong style={{ color: 'var(--risk-caution)' }}> {t('system.fan.permOff')}</strong>
          )}
        </span>
      </div>
      {fanErr && <div className="error" role="alert">{fanErr}</div>}
      {fanLoading && <div className="muted small">{t('system.fan.loading')}</div>}
      {fan && (
        <div className="sys-fan" style={{ borderLeftColor: LEVEL_COLOR[fan.level] ?? 'var(--accent)' }}>
          <div className="sys-fan-head">
            <span className="sys-fan-level" style={{ color: LEVEL_COLOR[fan.level] ?? 'var(--accent)' }}>
              {fan.level_label}
            </span>
            <span className="sys-fan-metrics muted small">
              {fan.temp_max_c != null && <>{t('system.fan.tempMax')} <b>{Math.round(fan.temp_max_c)}℃</b></>}
              {fan.temp_max_c != null && fan.cpu_load_pct != null && ' · '}
              {fan.cpu_load_pct != null && <>{t('system.fan.cpuLoad')} <b>{Math.round(fan.cpu_load_pct)}%</b></>}
            </span>
          </div>
          <p className="sys-fan-reason small">{fan.reason}</p>
          {/* R173：与上次建议对比——档位抬升/温度差让「变了没」一眼可见 */}
          {fanDelta && (
            <p className="muted small sys-fan-delta">
              {fanDelta.levelChanged
                ? t('system.fan.deltaLevelChanged', { from: fanDelta.prevLevelLabel, to: fan.level_label })
                : t('system.fan.deltaLevelSame', { level: fan.level_label })}
              {fanDelta.tempDeltaC != null && (
                <> · {fanDelta.tempDeltaC > 0
                  ? t('system.fan.deltaTempUp', { delta: Math.round(fanDelta.tempDeltaC) })
                  : fanDelta.tempDeltaC < 0
                    ? t('system.fan.deltaTempDown', { delta: Math.round(-fanDelta.tempDeltaC) })
                    : t('system.fan.deltaTempSame')}</>
              )}
            </p>
          )}
          <div className="sys-fan-curve">
            <span className="muted small" style={{ marginBottom: 3 }}>{t('system.fan.curveAnchors')}</span>
            <div className="sys-fan-anchors">
              {fan.curve.map((pt) => (
                <span key={pt.load_pct} className="sys-fan-anchor">
                  <b>{pt.load_pct}%</b> → {pt.fan_pct}%
                </span>
              ))}
            </div>
          </div>
          {fan.fans.length > 0 && (
            <div className="muted small" style={{ marginTop: 6 }}>
              {t('system.fan.sensorsLabel')}{fan.fans.map((f) => String(f.name ?? '')).filter(Boolean).join(t('system.fan.listSep')) || t('system.fan.sensorsNone')}
            </div>
          )}
          <p className="muted small" style={{ marginTop: 4 }}>{fan.note}</p>
        </div>
      )}
      <button type="button" className="ghost small" onClick={loadFan} disabled={fanLoading || !fanPerm}>
        <RefreshCw size={11} className={fanLoading ? 'spin' : undefined} /> {t('system.fan.reread')}
      </button>

      {fanCtrlPerm && (
        <div className="sys-fan-control" style={{ marginTop: 12 }}>
          <div className="sys-fan-control-head">
            <span className="muted small"><Fan size={12} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('system.fan.controlTitle')}</span>
            {fanChannelState(fanStatus, fanStatusErr) === 'ok' ? (
              <span className="sys-tag sys-tag-green">{t('system.fan.channelOk', { name: fanStatus!.probe?.cli?.name ?? 'CLI' })}</span>
            ) : fanChannelState(fanStatus, fanStatusErr) === 'unread' ? (
              <span className="sys-tag sys-tag-grey" title={fanStatusErr ?? undefined}>{t('system.fan.channelUnread')}</span>
            ) : (
              <span className="sys-tag sys-tag-grey">{t('system.fan.channelNone')}</span>
            )}
          </div>
          {fanBreakerNote && (
            <div className="error" role="alert" style={{ marginTop: 6 }}>
              <AlertTriangle size={12} style={{ verticalAlign: '-2px', marginRight: 4 }} />{fanBreakerNote}
            </div>
          )}
          {fanCtrlErr && <div className="error" role="alert" style={{ marginTop: 6 }}>{fanCtrlErr}</div>}
          {fanStatus?.guard_running && (
            <div className="muted small" style={{ marginTop: 6 }}>
              {t('system.fan.guardRunning')}
            </div>
          )}
          <div className="sys-fan-control-row" style={{ marginTop: 8, display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            <select
              value={fanLevel}
              onChange={(e) => setFanLevel(e.target.value)}
              disabled={fanCtrlBusy || !fanStatus?.writable}
              aria-label={t('system.fan.levelAria')}
            >
              <option value="quiet">{t('system.fan.levelQuiet')}</option>
              <option value="balanced">{t('system.fan.levelBalanced')}</option>
              <option value="balanced_plus">{t('system.fan.levelBalancedPlus')}</option>
              <option value="high">{t('system.fan.levelHigh')}</option>
              <option value="urgent">{t('system.fan.levelUrgent')}</option>
            </select>
            {fanConfirming ? (
              <span className="sys-confirm">
                <span className="muted small">{t('system.fan.confirmApply', { level: fanLevel })}</span>
                <button type="button" className="danger small" onClick={applyFanControl} disabled={fanCtrlBusy}>
                  {fanCtrlBusy ? t('system.fan.applying') : t('system.fan.confirmApplyBtn')}
                </button>
                <button type="button" className="ghost small" onClick={() => setFanConfirming(false)}>{t('system.common.cancel')}</button>
              </span>
            ) : (
              <button
                type="button"
                className="primary small"
                onClick={() => setFanConfirming(true)}
                disabled={fanCtrlBusy || !fanStatus?.writable}
              >
                {t('system.fan.applyLevel')}
              </button>
            )}
            {fanRestoreAsk ? (
              <span className="sys-confirm">
                <span className="muted small">{t('system.fan.confirmRestore')}</span>
                <button type="button" className="danger small" onClick={restoreDefaultFan} disabled={fanRestoring}>
                  {fanRestoring ? t('system.fan.restoring') : t('system.fan.confirmRestoreBtn')}
                </button>
                <button type="button" className="ghost small" onClick={() => setFanRestoreAsk(false)}>{t('system.common.cancel')}</button>
              </span>
            ) : (
              <button
                type="button"
                className="ghost small"
                onClick={() => setFanRestoreAsk(true)}
                disabled={fanRestoring || !fanStatus?.writable}
                title={t('system.fan.restoreTitle')}
              >
                {t('system.fan.restoreDefault')}
              </button>
            )}
          </div>
        </div>
      )}

      {/* ── 驱动更新检查 ── */}
      <div className="settings-group-title" style={{ marginTop: 22 }}>{t('system.driver.title')}</div>
      <div className="hint" style={{ marginBottom: 8 }}>
        <span>
          {t('system.driver.hint')}
          {!driverPerm && (
            <strong style={{ color: 'var(--risk-caution)' }}> {t('system.driver.permOff')}</strong>
          )}
        </span>
      </div>
      {drvErr && <div className="error" role="alert">{drvErr}</div>}
      {drvLoading && <div className="muted small">{t('system.driver.loading')}</div>}
      {drivers && (
        <div className="muted small" style={{ marginBottom: 4 }}>
          {t('system.driver.installedCount', { n: drivers.length })}
        </div>
      )}
      {/* R166：搜索 + 分类筛选——200 条上限下列表很长，按名找很难 */}
      {drivers && drivers.length > 0 && (
        <div className="sys-filter-bar">
          <span className="sys-filter-chips">
            <button
              type="button"
              className={'chip' + (drvClass === null ? ' active' : '')} aria-pressed={drvClass === null}
              onClick={() => setDrvClass(null)}
            >
              {t('system.driver.classAll')}
            </button>
            {classCounts.map((c) => (
              <button
                key={c.cls || '(blank)'}
                type="button"
                className={'chip' + (drvClass === c.cls ? ' active' : '')} aria-pressed={drvClass === c.cls}
                onClick={() => setDrvClass(c.cls)}
              >
                {c.cls || t('system.driver.classUnknown')} <b>{c.n}</b>
              </button>
            ))}
          </span>
          <label className="sys-filter-search">
            <Search size={13} />
            <input
              value={drvQuery}
              onChange={(e) => setDrvQuery(e.target.value)}
              placeholder={t('system.driver.searchPlaceholder')}
            />
          </label>
        </div>
      )}
      {drivers && drivers.length > 0 && visibleDrivers.length === 0 && (
        <div className="muted small">{t('system.driver.noneMatched')}</div>
      )}
      {drivers && drivers.length > 0 && visibleDrivers.length > 0 && (
        <div className="sys-list sys-list-scroll">
          {visibleDrivers.slice(0, 200).map((d, i) => (
            <div key={i} className="sys-row">
              <div className="sys-row-main">
                <span className="sys-name">{d.name}</span>
                <span className="sys-sub muted">
                  {d.provider || t('system.driver.unknownProvider')} · v{d.version}{d.date ? ` · ${d.date}` : ''}
                </span>
              </div>
              {d.class && <span className="sys-tag">{d.class}</span>}
            </div>
          ))}
          {drivers.length > 200 && <div className="muted small">{t('system.driver.onlyFirst200')}</div>}
        </div>
      )}
      <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginTop: 8 }}>
        <button type="button" className="ghost small" onClick={loadDrivers} disabled={drvLoading || !driverPerm}>
          <RefreshCw size={11} className={drvLoading ? 'spin' : undefined} /> {t('system.driver.refreshList')}
        </button>
        <button
          type="button"
          className="ghost small"
          onClick={checkWuUpdates}
          disabled={wuLoading || !driverPerm}
          title={t('system.driver.wuTitle')}
        >
          {wuLoading ? <RefreshCw size={11} className="spin" /> : <DownloadCloud size={11} />}{' '}
          {wuLoading ? t('system.driver.wuLoading') : t('system.driver.wuQuery')}
        </button>
      </div>
      {updates && updates.length > 0 && (
        <>
          {/* R167：可选更新里驱动与语言包/功能更新混排——只看驱动的人要能筛 */}
          <div className="sys-filter-bar">
            <span className="muted small">
              {t('system.driver.wuCounts', { total: wuCounts.total, drivers: wuCounts.drivers })}
            </span>
            <span className="sys-filter-chips">
              <button
                type="button"
                className={'chip' + (wuDriverOnly ? '' : ' active')} aria-pressed={!wuDriverOnly}
                onClick={() => setWuDriverOnly(false)}
              >
                {t('system.driver.wuFilterAll')}
              </button>
              <button
                type="button"
                className={'chip' + (wuDriverOnly ? ' active' : '')} aria-pressed={wuDriverOnly}
                onClick={() => setWuDriverOnly(true)}
                disabled={wuCounts.drivers === 0}
              >
                {t('system.driver.wuFilterDrivers')}
              </button>
            </span>
          </div>
          <div className="sys-list" style={{ marginTop: 10 }}>
            {visibleUpdates.length === 0 ? (
              <div className="muted small">{t('system.driver.wuNoDrivers')}</div>
            ) : (
              visibleUpdates.map((u, i) => (
              <div key={i} className="sys-row">
                <div className="sys-row-main">
                  <span className="sys-name">{u.title}</span>
                  <span className="sys-sub muted">
                    {u.kb && <>KB{u.kb} · </>}
                    {u.category || t('system.driver.catDefault')}
                  </span>
                </div>
                <span className="sys-tag sys-tag-blue">
                  <DownloadCloud size={10} style={{ verticalAlign: '-1px', marginRight: 3 }} />
                  {t('system.driver.updatable')}
                </span>
              </div>
              ))
            )}
          </div>
        </>
      )}
      {updates && updates.length === 0 && !wuLoading && (
        <div className="muted small" style={{ marginTop: 6 }}>{t('system.driver.noneAvailable')}</div>
      )}
    </div>
  );
}
