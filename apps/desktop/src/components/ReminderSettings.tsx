import { useEffect, useState } from 'react';
import { Play, BellOff, TrendingUp, ListPlus, Trash2, AlertTriangle } from 'lucide-react';
import { api } from '../api';
import type { ReminderConfig, ReminderPayload, SpaceForecast, AutoListConfigView, AutoListRule } from '../api';
import { formatBytes } from '../format';
import { useStore } from '../store';
import { useT } from '../i18n';

/**
 * 按项授权（health-butler P2-1）的纯规则变换，与后端 auto_list.rs 同语义。
 * 覆写 = 调用方持有完整规则集；添加按 id 排序（多规则命中同 scaffold 取
 * 更低阈值，后端 should_auto_list 已保证）；删除按 scaffold_id 过滤。
 */
export function withRule(
  rules: AutoListRule[],
  op: 'add' | 'remove',
  arg: { scaffold_id: string; min_bytes?: number },
): AutoListRule[] {
  if (op === 'remove') {
    return rules.filter((r) => r.scaffold_id !== arg.scaffold_id);
  }
  if (arg.min_bytes === undefined) return rules;
  // 同 id 先剔除再插入：用户再选一次同一项，就是改阈值（最新一次选择即意图）。
  // 若保留两条同 id 规则，后端按最低阈值命中，用户刚填的更大值会静默失效。
  const next = rules
    .filter((r) => r.scaffold_id !== arg.scaffold_id)
    .map((r) => ({ scaffold_id: r.scaffold_id, min_bytes: r.min_bytes }));
  next.push({ scaffold_id: arg.scaffold_id, min_bytes: arg.min_bytes });
  next.sort((a, b) => a.scaffold_id.localeCompare(b.scaffold_id));
  return next;
}

/**
 * 清理提醒设置（R3）：开关 + 间隔 + 阈值 + 手动检查一次。
 * 铁律：只出清单 + 通知，绝不自动清理——检查按钮也只跑引擎真算并发通知，
 * 清理仍走总览页确认窗（execute_ai_plan user_confirmed）。
 */
export function ReminderSettings() {
  const t = useT();
  // 本机已安装清理项（规则候选 + 生效状态判定用）。来自总览的 listScaffolds 缓存。
  const scaffolds = useStore((s) => s.scaffolds);
  const [cfg, setCfg] = useState<ReminderConfig | null>(null);
  // 配置读失败单独记：失败 ≠ 加载中（R79），绝不永久转圈。
  const [cfgErr, setCfgErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const [lastPayload, setLastPayload] = useState<ReminderPayload | null>(null);
  // R100 逐盘趋势检查：独立忙碌态 + 结果 + 读失败原因（失败 ≠ 空列表）。
  const [forecastBusy, setForecastBusy] = useState(false);
  const [forecast, setForecast] = useState<SpaceForecast[] | null>(null);
  const [forecastErr, setForecastErr] = useState<string | null>(null);
  // health-butler P2-1 按项授权：规则集（含每条生效状态）+ 待添加的 scaffold + 阈值。
  const [autoList, setAutoList] = useState<AutoListConfigView | null>(null);
  const [autoListErr, setAutoListErr] = useState<string | null>(null);
  const [autoListBusy, setAutoListBusy] = useState(false);
  const [pickId, setPickId] = useState('');
  const [pickMb, setPickMb] = useState(0);

  // 打开时同步后端配置
  useEffect(() => {
    let cancelled = false;
    api.getReminderConfig().then((c) => {
      if (!cancelled) { setCfg(c); setCfgErr(null); }
    }).catch((e) => {
      // 读失败 ≠ 加载中：记录原因，别让「加载中…」永远转圈（R79）。
      if (!cancelled) setCfgErr(String(e instanceof Error ? e.message : e));
    });
    return () => {
      cancelled = true;
    };
  }, []);

  // health-butler P2-1：读取授权规则集（带每条规则的生效状态，按真实 scaffolds 标注）。
  useEffect(() => {
    let cancelled = false;
    api.getAutoListConfigWithStatus(scaffolds.map((s) => s.id)).then((v) => {
      if (!cancelled) { setAutoList(v); setAutoListErr(null); }
    }).catch((e) => {
      // 读失败 ≠ 加载中：记录原因展示（R79 同一口径）。
      if (!cancelled) setAutoListErr(String(e instanceof Error ? e.message : e));
    });
    return () => { cancelled = true; };
    // scaffolds 变化（安装/卸载清理项后）时重取状态标注。
  }, [scaffolds]);

  const persistAutoList = async (next: { enabled: boolean; rules: AutoListRule[] }) => {
    setAutoListBusy(true);
    try {
      // 后端覆写语义：调用方持有的就是完整规则集。status 由下次带已知列表的加载补上。
      await api.setAutoListConfig(next);
      const v = await api.getAutoListConfigWithStatus(scaffolds.map((s) => s.id));
      setAutoList(v);
    } catch (e) {
      useStore.getState().toast(t('system.reminder.toastSaveFailed', { err: String(e instanceof Error ? e.message : e) }), 'err');
    } finally {
      setAutoListBusy(false);
    }
  };

  const addAutoListRule = async () => {
    if (!autoList || !pickId) return;
    // 只允许本机真实存在的清理项（下拉本来就只列这些；这里再挡一次防御）。
    if (!scaffolds.some((s) => s.id === pickId)) return;
    const next = withRule(autoList.rules, 'add', {
      scaffold_id: pickId,
      min_bytes: pickMb * 1024 * 1024,
    });
    await persistAutoList({ enabled: autoList.enabled, rules: next });
    setPickId('');
    setPickMb(0);
  };

  const removeAutoListRule = async (scaffoldId: string) => {
    if (!autoList) return;
    const next = withRule(autoList.rules, 'remove', { scaffold_id: scaffoldId });
    await persistAutoList({ enabled: autoList.enabled, rules: next });
  };

  const addable = scaffolds.filter((s) => !autoList?.rules.some((r) => r.scaffold_id === s.id));

  const save = async (patch: Partial<ReminderConfig>) => {
    setSaving(true);
    try {
      const next = await api.setReminderConfig(patch);
      setCfg(next);
    } catch (e) {
      useStore.getState().toast(t('system.reminder.toastSaveFailed', { err: String(e instanceof Error ? e.message : e) }), 'err');
    } finally {
      setSaving(false);
    }
  };

  const runNow = async () => {
    setBusy(true);
    try {
      const p = await api.runReminderCheck();
      setLastPayload(p);
      if (!p) {
        useStore.getState().toast(t('system.reminder.toastNoItems'), 'ok');
      }
    } catch (e) {
      useStore.getState().toast(t('system.reminder.toastCheckFailed', { err: String(e instanceof Error ? e.message : e) }), 'err');
    } finally {
      setBusy(false);
    }
  };

  // R100：立即检查各盘空间趋势（只读，外推已有采样，绝不触发扫描/清理）。
  const checkForecast = async () => {
    setForecastBusy(true);
    setForecastErr(null);
    try {
      const rows = await api.checkSpaceForecast();
      setForecast(rows);
    } catch (e) {
      // 读失败 ≠ 空列表：记录原因展示，不伪装成「没有趋势」。
      setForecast(null);
      setForecastErr(String(e instanceof Error ? e.message : e));
    } finally {
      setForecastBusy(false);
    }
  };

  if (cfgErr) {
    return (
      <div className="settings-pane">
        <div className="error small">{t('system.reminder.loadFailed', { err: cfgErr })}</div>
      </div>
    );
  }

  if (!cfg) {
    return <div className="settings-pane"><div className="muted">{t('system.reminder.loading')}</div></div>;
  }

  return (
    <div className="settings-pane">
      <div className="settings-group-title">{t('system.reminder.title')}</div>
      <div className="switch-row">
        <div>
          <div className="switch-label">{t('system.reminder.enabledLabel')}</div>
          <div className="switch-desc">
            {t('system.reminder.enabledDescA')}
            <b>{t('system.reminder.enabledDescB')}</b>{t('system.reminder.enabledDescC')}
          </div>
        </div>
        <input
          type="checkbox"
          className="switch"
          checked={cfg.enabled}
          onChange={(e) => save({ enabled: e.target.checked })}
        />
      </div>

      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.intervalTitle')}</div>
      <div className="seg seg-4 auto">
        {[6, 12, 24, 72].map((h) => (
          <button
            key={h}
            type="button"
            className={`seg-opt${cfg.interval_hours === h ? ' active' : ''}`}
            onClick={() => save({ interval_hours: h })}
          >
            {h === 72 ? t('system.reminder.days3') : t('system.reminder.hours', { n: h })}
          </button>
        ))}
      </div>
      <div className="muted small" style={{ marginTop: 6 }}>
        {t('system.reminder.intervalHint')}
      </div>

      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.thresholdTitle')}</div>
      <label className="field">
        <span>{t('system.reminder.thresholdLabel')}</span>
        <div className="seg seg-4 auto">
          {[50, 200, 500, 1024].map((mb) => (
            <button
              key={mb}
              type="button"
              className={`seg-opt${cfg.min_bytes === mb * 1024 * 1024 ? ' active' : ''}`}
              onClick={() => save({ min_bytes: mb * 1024 * 1024 })}
            >
              {mb >= 1024 ? t('system.reminder.gb', { gb: 1 }) : t('system.reminder.mb', { mb })}
            </button>
          ))}
        </div>
      </label>

      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.weeklyLabel')}</div>
      <div className="switch-row">
        <div>
          <div className="switch-label">{t('system.reminder.weeklyLabel')}</div>
          <div className="switch-desc">
            {t('system.reminder.weeklyDescA')}
            <b>{t('system.reminder.weeklyDescB')}</b>
          </div>
        </div>
        <input
          type="checkbox"
          className="switch"
          checked={cfg.weekly_report_enabled}
          onChange={(e) => save({ weekly_report_enabled: e.target.checked })}
        />
      </div>
      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.weeklyIntervalTitle')}</div>
      <div className="seg seg-4 auto">
        {[7, 14, 30, 90].map((d) => (
          <button
            key={d}
            type="button"
            className={`seg-opt${cfg.weekly_interval_days === d ? ' active' : ''}`}
            onClick={() => save({ weekly_interval_days: d })}
          >
            {t('system.reminder.weeklyDays', { n: d })}
          </button>
        ))}
      </div>
      <div className="muted small" style={{ marginTop: 6 }}>
        {t('system.reminder.weeklyIntervalHint')}
      </div>

      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.fullWarnLabel')}</div>
      <div className="switch-desc">
        {t('system.reminder.fullWarnDescA')}
        <b>{t('system.reminder.fullWarnDescB')}</b>
      </div>
      <div className="seg seg-5 auto" style={{ marginTop: 8 }}>
        {[0, 3, 7, 14, 30].map((d) => (
          <button
            key={d}
            type="button"
            className={`seg-opt${cfg.full_warn_days === d ? ' active' : ''}`}
            onClick={() => save({ full_warn_days: d })}
          >
            {d === 0 ? t('system.reminder.fullWarnOff') : t('system.reminder.fullWarnDays', { n: d })}
          </button>
        ))}
      </div>
      <div className="muted small" style={{ marginTop: 6 }}>
        {t('system.reminder.fullWarnHint')}
      </div>

      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.scanStaleLabel')}</div>
      <div className="switch-desc">
        {t('system.reminder.scanStaleDescA')}
        <b>{t('system.reminder.scanStaleDescB')}</b>
      </div>
      <div className="seg seg-5 auto" style={{ marginTop: 8 }}>
        {[0, 7, 14, 21, 30, 60].map((d) => (
          <button
            key={d}
            type="button"
            className={`seg-opt${cfg.scan_stale_days === d ? ' active' : ''}`}
            onClick={() => save({ scan_stale_days: d })}
          >
            {d === 0 ? t('system.reminder.fullWarnOff') : t('system.reminder.fullWarnDays', { n: d })}
          </button>
        ))}
      </div>
      <div className="muted small" style={{ marginTop: 6 }}>
        {t('system.reminder.scanStaleHint')}
      </div>

      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.forecastTitle')}</div>
      <button className="ghost" onClick={checkForecast} disabled={forecastBusy}>
        <TrendingUp size={13} /> {forecastBusy ? t('system.reminder.forecastChecking') : t('system.reminder.forecastCheck')}
      </button>
      {forecastErr && (
        <div className="error small" style={{ marginTop: 8 }}>
          {t('system.reminder.loadFailed', { err: forecastErr })}
        </div>
      )}
      {forecast && forecast.length === 0 && !forecastErr && (
        <div className="muted small" style={{ marginTop: 8 }}>{t('system.reminder.forecastEmpty')}</div>
      )}
      {forecast && forecast.length > 0 && (
        <div className="reminder-result" style={{ marginTop: 8 }}>
          {forecast.map((f) => {
            const status =
              f.reason === 'ok' && f.days !== null
                ? f.days === 0
                  ? t('system.reminder.forecastNow')
                  : t('system.reminder.forecastOk', { n: f.days })
                : f.reason === 'no_history'
                  ? t('system.reminder.forecastNoHistory')
                  : f.reason === 'not_enough'
                    ? t('system.reminder.forecastNotEnough')
                    : t('system.reminder.forecastReleasing');
            return (
              <div key={f.path} className="muted small">
                {f.path} · {status}
                {f.warn && t('system.reminder.forecastWarn')}
              </div>
            );
          })}
        </div>
      )}

      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.runNowTitle')}</div>
      <button className="ghost" onClick={runNow} disabled={busy || !cfg.enabled}>
        <Play size={13} /> {busy ? t('system.reminder.checking') : t('system.reminder.checkNow')}
      </button>
      {!cfg.enabled && (
        <div className="muted small" style={{ marginTop: 6 }}>
          <BellOff size={12} /> {t('system.reminder.disabledHint')}
        </div>
      )}
      {lastPayload && (
        <div className="reminder-result" style={{ marginTop: 10 }}>
          {t('system.reminder.lastCheck')} <b>{formatBytes(lastPayload.total_bytes)}</b>
          {lastPayload.drives.map((d) => (
            <div key={d.path} className="muted small">
              {d.path} · {formatBytes(d.bytes)}
            </div>
          ))}
          {cfg.enabled && t('system.reminder.notified')}
        </div>
      )}
      {saving && <div className="muted small" style={{ marginTop: 6 }}>{t('system.reminder.saving')}</div>}

      {/* health-butler P2-1 按项授权：给清理项设阈值规则，达标自动进总览清单 */}
      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.autoListTitle')}</div>
      <div className="muted small" style={{ marginTop: 2 }}>
        {t('system.reminder.autoListDescA')}
        <b> {t('system.reminder.autoListDescB')}</b>
      </div>

      {autoListErr ? (
        <div className="muted small err" style={{ marginTop: 8 }}>{autoListErr}</div>
      ) : !autoList ? (
        <div className="muted small" style={{ marginTop: 8 }}>{t('system.reminder.loading')}</div>
      ) : (
        <>
          <div className="switch-row" style={{ marginTop: 10 }}>
            <div>
              <div className="switch-label">{t('system.reminder.enabledLabel')}</div>
              <div className="switch-desc">{t('system.reminder.autoListDescB')}</div>
            </div>
            <input
              type="checkbox"
              className="switch"
              checked={autoList.enabled}
              disabled={autoListBusy}
              onChange={(e) => void persistAutoList({ enabled: e.target.checked, rules: autoList.rules.map((r) => ({ scaffold_id: r.scaffold_id, min_bytes: r.min_bytes })) })}
            />
          </div>

          {autoList.rules.length === 0 ? (
            <div className="muted small" style={{ marginTop: 8 }}>{t('system.reminder.autoListEmpty')}</div>
          ) : (
            <div style={{ marginTop: 8 }}>
              {autoList.rules.map((r) => {
                const sc = scaffolds.find((s) => s.id === r.scaffold_id);
                return (
                  <div key={r.scaffold_id} className="rule-row" style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 6 }}>
                    <AlertTriangle size={13} style={{ flexShrink: 0 }} color={r.status === 'unknown_scaffold' ? '#b45309' : undefined} />
                    <span style={{ flex: 1 }}>{sc ? sc.name : r.scaffold_id}</span>
                    <span className="muted small">
                      {r.min_bytes === 0 ? t('system.reminder.autoListAny') : formatBytes(r.min_bytes)}
                    </span>
                    {r.status === 'unknown_scaffold' && (
                      <span className="muted xsmall" style={{ color: '#b45309' }}>{t('system.reminder.autoListIdle')}</span>
                    )}
                    <button className="ghost icon" aria-label={t('system.reminder.autoListDelete')} disabled={autoListBusy} onClick={() => void removeAutoListRule(r.scaffold_id)}>
                      <Trash2 size={13} />
                    </button>
                  </div>
                );
              })}
            </div>
          )}

          {addable.length > 0 && (
            <div style={{ display: 'flex', gap: 8, marginTop: 10, flexWrap: 'wrap', alignItems: 'center' }}>
              <select
                className="field"
                value={pickId}
                onChange={(e) => setPickId(e.target.value)}
                style={{ maxWidth: 220 }}
              >
                <option value="">{t('system.reminder.autoListPick')}</option>
                {addable.map((s) => (
                  <option key={s.id} value={s.id}>{s.name}</option>
                ))}
              </select>
              <select
                className="field"
                value={pickMb}
                onChange={(e) => setPickMb(Number(e.target.value))}
                style={{ maxWidth: 150 }}
              >
                <option value={0}>{t('system.reminder.autoListAny')}</option>
                {[50, 100, 200, 500, 1024, 2048].map((mb) => (
                  <option key={mb} value={mb}>{formatBytes(mb * 1024 * 1024)}</option>
                ))}
              </select>
              <button className="ghost" disabled={autoListBusy || !pickId} onClick={() => void addAutoListRule()}>
                <ListPlus size={13} /> {t('system.reminder.autoListAdd')}
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
