import { useEffect, useState } from 'react';
import { Trash2, RefreshCw, DownloadCloud, Fan, AlertTriangle } from 'lucide-react';
import { api, type StartupItem, type FanCurveAdvice, type FanStatus, type InstalledDriver, type DriverUpdate } from '../api';
import { isPermEnabled } from '../permissions';
import { isTauri } from '../env';
import { ErrorBoundary } from './ErrorBoundary';
import { listen } from '@tauri-apps/api/event';
import { useT } from '../i18n';

// 启动项来源标签：switch 比较的是后端返回的标识符（不译），文案在渲染处求值——
// 模块顶层求值会把中文烤死在首次 import，切语言不再生效。未知来源原样显示。
function startupLocationLabel(t: (key: string) => string, location: string): string {
  switch (location) {
    case 'registry_hkcu':
      return t('system.startup.locHkcu');
    case 'registry_hklm':
      return t('system.startup.locHklm');
    case 'folder_user':
      return t('system.startup.locFolderUser');
    case 'folder_machine':
      return t('system.startup.locFolderMachine');
    default:
      return location;
  }
}

const LEVEL_COLOR: Record<string, string> = {
  urgent: '#d93025',
  high: '#ea8600',
  balanced_plus: '#d97706',
  balanced: '#16a34a',
  quiet_ok: '#16a34a',
  temp_only: '#0d9488',
  unknown_temp: '#64748b',
  unknown: '#94a3b8',
};

/** 设置页「系统工具」标签：启动项管理 / 风扇曲线建议 / 驱动更新检查。
 *  三个能力都是 L2（权限中心开关 + 每次执行确认），组件内做两步确认，
 *  删除/禁用的破坏性操作绝不直接执行。 */
export function SystemTools() {
  return (
    <ErrorBoundary>
      <SystemToolsInner />
    </ErrorBoundary>
  );
}

function SystemToolsInner() {
  const t = useT();
  const [startup, setStartup] = useState<StartupItem[] | null>(null);
  const [startupErr, setStartupErr] = useState<string | null>(null);
  const [startupLoading, setStartupLoading] = useState(false);
  // 两步确认：pendingId 待确认项（点了「禁用/删除」先变「确认？」按钮）。
  const [pendingToggle, setPendingToggle] = useState<string | null>(null);
  const [pendingDel, setPendingDel] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const [fan, setFan] = useState<FanCurveAdvice | null>(null);
  const [fanErr, setFanErr] = useState<string | null>(null);
  const [fanLoading, setFanLoading] = useState(false);
  // 风扇控制（fan.control，L2）：状态 + 档位 + 确认 + 熔断通知。
  const [fanStatus, setFanStatus] = useState<FanStatus | null>(null);
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

  const startupPerm = isPermEnabled('startup.manage');
  const fanPerm = isPermEnabled('fan.adjust');
  const fanCtrlPerm = isPermEnabled('fan.control');
  const driverPerm = isPermEnabled('driver.check');

  const flashMsg = (s: string) => {
    setMsg(s);
    setTimeout(() => setMsg(null), 3500);
  };

  const loadStartup = async () => {
    setStartupLoading(true);
    setStartupErr(null);
    try {
      setStartup(await api.listStartupItems());
    } catch (e) {
      setStartupErr(String(e instanceof Error ? e.message : e));
    } finally {
      setStartupLoading(false);
    }
  };

  const loadFan = async () => {
    setFanLoading(true);
    setFanErr(null);
    try {
      setFan(await api.fanCurveAdvice());
    } catch (e) {
      setFanErr(String(e instanceof Error ? e.message : e));
    } finally {
      setFanLoading(false);
    }
  };

  const loadFanStatus = async () => {
    try {
      setFanStatus(await api.fanStatus());
    } catch {
      // 只读状态读不到不致命（探测失败保持现状）。
    }
  };

  const applyFanControl = async () => {
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
    if (startupPerm) loadStartup();
    if (fanPerm) loadFan();
    if (driverPerm) loadDrivers();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [startupPerm, fanPerm, driverPerm]);

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
  }, [fanCtrlPerm]);

  const toggleStartup = async (item: StartupItem, enable: boolean) => {
    try {
      await api.setStartupItem(item.id, enable, true);
      flashMsg(t('system.startup.toastToggled', {
        action: enable ? t('system.startup.enabled') : t('system.startup.disabled'),
        name: item.name,
      }));
      await loadStartup();
    } catch (e) {
      setStartupErr(String(e instanceof Error ? e.message : e));
    } finally {
      setPendingToggle(null);
    }
  };

  const removeStartup = async (item: StartupItem) => {
    try {
      await api.removeStartupItem(item.id, true);
      flashMsg(t('system.startup.toastDeleted', { name: item.name }));
      await loadStartup();
    } catch (e) {
      setStartupErr(String(e instanceof Error ? e.message : e));
    } finally {
      setPendingDel(null);
    }
  };

  return (
    <div className="settings-pane sys-tools">
      {msg && <div className="ok">{msg}</div>}

      {/* ── 启动项管理 ── */}
      <div className="settings-group-title">{t('system.startup.title')}</div>
      <div className="hint" style={{ marginBottom: 8 }}>
        <span>
          {t('system.startup.hintA')} <code>.disabled</code>{' '}
          {t('system.startup.hintB')}
          {!startupPerm && (
            <strong style={{ color: 'var(--ink-1)' }}> {t('system.startup.permOff')}</strong>
          )}
        </span>
      </div>
      {startupErr && <div className="error">{startupErr}</div>}
      {startupLoading && <div className="muted small">{t('system.startup.loading')}</div>}
      {startup && startup.length === 0 && !startupLoading && (
        <div className="muted small">{t('system.startup.empty')}</div>
      )}
      {startup && startup.length > 0 && (
        <div className="sys-list">
          {startup.map((item) => (
            <div key={item.id} className={'sys-row' + (item.enabled ? '' : ' sys-row-off')}>
              <div className="sys-row-main">
                <span className="sys-name">{item.name}</span>
                <span className="sys-sub muted">
                  {startupLocationLabel(t, item.location)}
                  {item.command ? ` · ${item.command.length > 70 ? item.command.slice(0, 70) + '…' : item.command}` : ''}
                </span>
              </div>
              <div className="sys-row-actions">
                {item.location.startsWith('registry') && (
                  <button
                    type="button"
                    className="ghost icon"
                    title={t('system.startup.delRegistryTitle')}
                    onClick={() => setPendingDel(pendingDel === item.id ? null : item.id)}
                  >
                    <Trash2 size={13} />
                  </button>
                )}
                {item.location.startsWith('folder') && (
                  <button
                    type="button"
                    className="ghost icon"
                    title={t('system.startup.delFolderTitle')}
                    onClick={() => setPendingDel(pendingDel === item.id ? null : item.id)}
                  >
                    <Trash2 size={13} />
                  </button>
                )}
                {pendingDel === item.id ? (
                  <span className="sys-confirm">
                    <span className="muted small">{t('system.common.confirmDelete')}</span>
                    <button type="button" className="danger small" onClick={() => removeStartup(item)}>{t('system.common.delete')}</button>
                    <button type="button" className="ghost small" onClick={() => setPendingDel(null)}>{t('system.common.cancel')}</button>
                  </span>
                ) : pendingToggle === item.id ? (
                  <span className="sys-confirm">
                    <span className="muted small">{item.enabled ? t('system.startup.confirmDisable') : t('system.startup.confirmRestore')}</span>
                    <button
                      type="button"
                      className={item.enabled ? 'danger small' : 'primary small'}
                      onClick={() => toggleStartup(item, !item.enabled)}
                    >
                      {item.enabled ? t('system.common.disable') : t('system.common.restore')}
                    </button>
                    <button type="button" className="ghost small" onClick={() => setPendingToggle(null)}>{t('system.common.cancel')}</button>
                  </span>
                ) : (
                  <button
                    type="button"
                    className={'switch' + (item.enabled ? ' on' : '')}
                    aria-checked={item.enabled}
                    aria-label={t('system.startup.ariaToggle', { action: item.enabled ? t('system.common.disable') : t('system.common.restore'), name: item.name })}
                    title={item.enabled ? t('system.startup.switchOffTitle') : t('system.startup.switchOnTitle')}
                    onClick={() => setPendingToggle(item.id)}
                  />
                )}
              </div>
            </div>
          ))}
        </div>
      )}
      <button type="button" className="ghost small" onClick={loadStartup} disabled={startupLoading || !startupPerm}>
        <RefreshCw size={11} className={startupLoading ? 'spin' : undefined} /> {t('system.startup.refresh')}
      </button>

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
            <strong style={{ color: 'var(--ink-1)' }}> {t('system.fan.permOff')}</strong>
          )}
        </span>
      </div>
      {fanErr && <div className="error">{fanErr}</div>}
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
            {fanStatus?.writable ? (
              <span className="sys-tag sys-tag-green">{t('system.fan.channelOk', { name: fanStatus.probe?.cli?.name ?? 'CLI' })}</span>
            ) : (
              <span className="sys-tag sys-tag-grey">{t('system.fan.channelNone')}</span>
            )}
          </div>
          {fanBreakerNote && (
            <div className="error" style={{ marginTop: 6 }}>
              <AlertTriangle size={12} style={{ verticalAlign: '-2px', marginRight: 4 }} />{fanBreakerNote}
            </div>
          )}
          {fanCtrlErr && <div className="error" style={{ marginTop: 6 }}>{fanCtrlErr}</div>}
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
            <strong style={{ color: 'var(--ink-1)' }}> {t('system.driver.permOff')}</strong>
          )}
        </span>
      </div>
      {drvErr && <div className="error">{drvErr}</div>}
      {drvLoading && <div className="muted small">{t('system.driver.loading')}</div>}
      {drivers && (
        <div className="muted small" style={{ marginBottom: 4 }}>
          {t('system.driver.installedCount', { n: drivers.length })}
        </div>
      )}
      {drivers && drivers.length > 0 && (
        <div className="sys-list sys-list-scroll">
          {drivers.slice(0, 200).map((d, i) => (
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
        <div className="sys-list" style={{ marginTop: 10 }}>
          {updates.map((u, i) => (
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
          ))}
        </div>
      )}
      {updates && updates.length === 0 && !wuLoading && (
        <div className="muted small" style={{ marginTop: 6 }}>{t('system.driver.noneAvailable')}</div>
      )}
    </div>
  );
}
