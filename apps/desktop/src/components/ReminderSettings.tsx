import { useEffect, useState } from 'react';
import { Play, BellOff, TrendingUp, ListPlus, Trash2, AlertTriangle } from 'lucide-react';
import { api } from '../api';
import type { ReminderConfig, ReminderPayload, SpaceForecast, AutoListConfigView, AutoListRule, StartupChanged } from '../api';
import { formatBytes } from '../format';
import { SCAN_STALE_WARN_DAYS, DEFAULT_THERMAL_SUSTAINED_C } from '../thresholds';
import { useStore } from '../store';
import { useT } from '../i18n';

/** R300：启动项对比结果的组织（纯函数，可单测）。
 *  后端 check_startup_delta_cmd 区分两种「没得比」：Err("no_history") = 真空
 *  （只有 0/1 份快照，首次快照已记）与真实读失败。UI 必须分开说——把「还没
 *  第二份快照」说成「读失败」，或把读失败说成「没有变化」，都是编造。
 *  delta 为 null = 读失败；added/gone 全空 = 一致。返回 (kind, text) 由调用方
 *  翻译文案，本函数只做判定与数字组织。 */
export type StartupDeltaKind = 'unread' | 'noHistory' | 'same' | 'changed';

export function formatStartupDelta(
  err: string | null | undefined,
  delta: { added: string[]; gone: string[]; gap_days: number; prev_at: number } | null | undefined,
): { kind: StartupDeltaKind; add: number; gone: number; gap: number } {
  if (err) {
    if (/no_history/i.test(err)) return { kind: 'noHistory', add: 0, gone: 0, gap: 0 };
    return { kind: 'unread', add: 0, gone: 0, gap: 0 };
  }
  if (!delta) return { kind: 'unread', add: 0, gone: 0, gap: 0 };
  const add = delta.added.length;
  const gone = delta.gone.length;
  if (add === 0 && gone === 0) return { kind: 'same', add: 0, gone: 0, gap: delta.gap_days };
  return { kind: 'changed', add, gone, gap: delta.gap_days };
}

/**
 * 按项授权（health-butler P2-1）的纯规则变换，与后端 auto_list.rs 同语义。
 * 覆写 = 调用方持有完整规则集；添加按 id 排序（多规则命中同 scaffold 取
 * 更低阈值，后端 should_auto_list 已保证）；删除按 scaffold_id 过滤。
 */
/** R302：把自动进清单的命中项组织成可渲染行（纯函数，可单测）。
 *  auto_list_hits 是后端 (scaffold_id, bytes) 对；scaffolds 是本机已安装清理项
 *  清单，用来把 id 翻译成可读名字。未知 id（规则指向已卸载项）不猜名字，
 *  原样显示 id——与规则列表 unknown_scaffold 的展示口径一致。
 *  返回 { key, label, bytes } 行，key 恒为 scaffold_id。 */
export function hitRows(
  hits: [string, number][] | null | undefined,
  scaffolds: { id: string; name: string }[],
): { key: string; label: string; bytes: number }[] {
  if (!hits || hits.length === 0) return [];
  return hits.map(([id, bytes]) => {
    const sc = scaffolds.find((s) => s.id === id);
    return { key: id, label: sc ? sc.name : id, bytes };
  });
}

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
 * R298：把后端返回的配置归一化成完整形状。
 * 旧版后端（R291/R292 之前）返回的 reminder.json 没有 thermal_* 与
 * startup_watch_* 字段
 * 字段——直接用 undefined 绑到复选框会让 React 把它当成
 * 非受控组件（切换一次就成了半受控，控制台告警且行为不可预测）。这里按字段
 * 补默认值，默认值取后端 ReminderConfig::default() 的同一口径。
 * 注意：只补缺字段，不覆盖后端已有的值——用户配过的就是配过的。
 */
export function normalizeReminderCfg(raw: Partial<ReminderConfig> | null | undefined): ReminderConfig | null {
  if (!raw) return null;
  return {
    enabled: raw.enabled ?? false,
    interval_hours: raw.interval_hours ?? 24,
    min_bytes: raw.min_bytes ?? 200 * 1024 * 1024,
    weekly_report_enabled: raw.weekly_report_enabled ?? false,
    weekly_interval_days: raw.weekly_interval_days ?? 7,
    full_warn_days: raw.full_warn_days ?? 0,
    scan_stale_days: raw.scan_stale_days ?? SCAN_STALE_WARN_DAYS,
    thermal_sample_enabled: raw.thermal_sample_enabled ?? false,
    thermal_sustained_c: raw.thermal_sustained_c ?? DEFAULT_THERMAL_SUSTAINED_C,
    thermal_sustained_samples: raw.thermal_sustained_samples ?? 5,
    startup_watch_enabled: raw.startup_watch_enabled ?? false,
    startup_watch_minutes: raw.startup_watch_minutes ?? 60,
  };
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
  // R300 启动项「立即对比」：独立忙碌/结果/失败态。Err 分两种（no_history 与真
  // 读失败），交给 formatStartupDelta 判定，UI 措辞分开。
  const [startupBusy, setStartupBusy] = useState(false);
  const [startupDelta, setStartupDelta] = useState<StartupChanged | null>(null);
  const [startupErr, setStartupErr] = useState<string | null>(null);
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
      // R298：归一化补缺字段（旧后端没有 thermal_*/startup_watch_*）。
      if (!cancelled) { setCfg(normalizeReminderCfg(c)); setCfgErr(null); }
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
  // R302：手动检查结果里的自动进清单命中项（一次算出，渲染时不再重复查找）。
  const lastHits = lastPayload ? hitRows(lastPayload.auto_list_hits, scaffolds) : [];

  const save = async (patch: Partial<ReminderConfig>) => {
    setSaving(true);
    try {
      const next = await api.setReminderConfig(patch);
      setCfg(normalizeReminderCfg(next));
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

  // R300：立即对比启动项（只读 startup-history.jsonl 最后两份快照，零枚举、零写入）。
  // 后端区分真空态（只有 0/1 份快照 → Err no_history）与真实读失败——
  // 分别给「首次快照」与「未读到」措辞，绝不用「没有变化」糊住任一种。
  const checkStartupDelta = async () => {
    setStartupBusy(true);
    setStartupErr(null);
    try {
      const d = await api.checkStartupDelta();
      setStartupDelta(d);
    } catch (e) {
      setStartupDelta(null);
      setStartupErr(String(e instanceof Error ? e.message : e));
    } finally {
      setStartupBusy(false);
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
      <div className="settings-pane" role="tabpanel" id="settings-pane-reminder" aria-labelledby="settings-tab-reminder">
        <div className="error small" role="alert">{t('system.reminder.loadFailed', { err: cfgErr })}</div>
      </div>
    );
  }

  if (!cfg) {
    return <div className="settings-pane" role="tabpanel" id="settings-pane-reminder" aria-labelledby="settings-tab-reminder"><div className="muted">{t('system.reminder.loading')}</div></div>;
  }

  return (
    <div className="settings-pane" role="tabpanel" id="settings-pane-reminder" aria-labelledby="settings-tab-reminder">
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
          aria-label={t('system.reminder.enabledLabel')}
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
            className={`seg-opt${cfg.interval_hours === h ? ' active' : ''}`} aria-pressed={cfg.interval_hours === h}
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
              className={`seg-opt${cfg.min_bytes === mb * 1024 * 1024 ? ' active' : ''}`} aria-pressed={cfg.min_bytes === mb * 1024 * 1024}
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
          aria-label={t('system.reminder.weeklyLabel')}
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
            className={`seg-opt${cfg.weekly_interval_days === d ? ' active' : ''}`} aria-pressed={cfg.weekly_interval_days === d}
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
            className={`seg-opt${cfg.full_warn_days === d ? ' active' : ''}`} aria-pressed={cfg.full_warn_days === d}
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
            className={`seg-opt${cfg.scan_stale_days === d ? ' active' : ''}`} aria-pressed={cfg.scan_stale_days === d}
            onClick={() => save({ scan_stale_days: d })}
          >
            {d === 0 ? t('system.reminder.fullWarnOff') : t('system.reminder.fullWarnDays', { n: d })}
          </button>
        ))}
      </div>
      <div className="muted small" style={{ marginTop: 6 }}>
        {t('system.reminder.scanStaleHint')}
      </div>

      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.thermalTitle')}</div>
      <div className="switch-row">
        <div>
          <div className="switch-label">{t('system.reminder.thermalTitle')}</div>
          <div className="switch-desc">
            {t('system.reminder.thermalDescA')}
            <b>{t('system.reminder.thermalDescB')}</b>
          </div>
        </div>
        <input
          type="checkbox"
          className="switch"
          aria-label={t('system.reminder.thermalTitle')}
          checked={cfg.thermal_sample_enabled}
          onChange={(e) => save({ thermal_sample_enabled: e.target.checked })}
        />
      </div>
      <label className="field" style={{ marginTop: 10 }}>
        <span>{t('system.reminder.thermalThreshold')}</span>
        <div className="seg seg-5 auto">
          {[75, 80, 85, 90, 95].map((c) => (
            <button
              key={c}
              type="button"
              className={`seg-opt${cfg.thermal_sustained_c === c ? ' active' : ''}`} aria-pressed={cfg.thermal_sustained_c === c}
              onClick={() => save({ thermal_sustained_c: c })}
            >
              {t('system.reminder.thermalC', { c })}
            </button>
          ))}
        </div>
      </label>
      <label className="field" style={{ marginTop: 10 }}>
        <span>{t('system.reminder.thermalSamplesLabel')}</span>
        <div className="seg seg-5 auto">
          {[1, 3, 5, 10, 15].map((n) => (
            <button
              key={n}
              type="button"
              className={`seg-opt${cfg.thermal_sustained_samples === n ? ' active' : ''}`} aria-pressed={cfg.thermal_sustained_samples === n}
              onClick={() => save({ thermal_sustained_samples: n })}
            >
              {t('system.reminder.thermalSamples', { n, min: n * 5 })}
            </button>
          ))}
        </div>
      </label>
      <div className="muted small" style={{ marginTop: 6 }}>
        {t('system.reminder.thermalHint')}
      </div>

      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.startupTitle')}</div>
      <div className="switch-row">
        <div>
          <div className="switch-label">{t('system.reminder.startupTitle')}</div>
          <div className="switch-desc">
            {t('system.reminder.startupDescA')}
            <b>{t('system.reminder.startupDescB')}</b>
          </div>
        </div>
        <input
          type="checkbox"
          className="switch"
          aria-label={t('system.reminder.startupTitle')}
          checked={cfg.startup_watch_enabled}
          onChange={(e) => save({ startup_watch_enabled: e.target.checked })}
        />
      </div>
      <label className="field" style={{ marginTop: 10 }}>
        <span>{t('system.reminder.startupIntervalLabel')}</span>
        <div className="seg seg-5 auto">
          {[15, 30, 60, 120, 240].map((m) => (
            <button
              key={m}
              type="button"
              className={`seg-opt${cfg.startup_watch_minutes === m ? ' active' : ''}`} aria-pressed={cfg.startup_watch_minutes === m}
              onClick={() => save({ startup_watch_minutes: m })}
            >
              {m === 60 ? t('system.reminder.startupHour') : t('system.reminder.startupMinutes', { n: m })}
            </button>
          ))}
        </div>
      </label>
      <div className="muted small" style={{ marginTop: 6 }}>
        {t('system.reminder.startupHint')}
      </div>
      <div className="reminder-result" style={{ marginTop: 10 }}>
        <button className="ghost" onClick={checkStartupDelta} disabled={startupBusy}>
          <TrendingUp size={13} /> {startupBusy ? t('system.reminder.startupChecking') : t('system.reminder.startupCheck')}
        </button>
        {(() => {
          const s = formatStartupDelta(startupErr, startupDelta);
          if (startupBusy) return null;
          if (s.kind === 'noHistory') {
            return <div className="muted small" style={{ marginTop: 8 }}>{t('system.reminder.startupNoHistory')}</div>;
          }
          if (s.kind === 'unread') {
            return <div className="error small" role="alert" style={{ marginTop: 8 }}>{t('system.reminder.loadFailed', { err: startupErr ?? '' })}</div>;
          }
          if (s.kind === 'same') {
            return <div className="muted small" style={{ marginTop: 8 }}>{t('system.reminder.startupSame')}</div>;
          }
          return (
            <div style={{ marginTop: 8 }}>
              <div className="muted small">{t('system.reminder.startupDelta', { days: s.gap, add: s.add, gone: s.gone })}</div>
              <div className="muted small" style={{ marginTop: 4 }}>{t('system.reminder.startupDeltaHint')}</div>
            </div>
          );
        })()}
      </div>
      <div className="settings-group-title" style={{ marginTop: 18 }}>{t('system.reminder.forecastTitle')}</div>
      <button className="ghost" onClick={checkForecast} disabled={forecastBusy}>
        <TrendingUp size={13} /> {forecastBusy ? t('system.reminder.forecastChecking') : t('system.reminder.forecastCheck')}
      </button>
      {forecastErr && (
        <div className="error small" role="alert" style={{ marginTop: 8 }}>
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
          {lastHits.length > 0 && (
            <div className="muted small" style={{ marginTop: 6 }}>
              {t('system.reminder.autoListHits', { items: lastHits.length })}
              {lastHits.map((row) => (
                <div key={row.key} style={{ marginTop: 2 }}>
                  {row.label} · {formatBytes(row.bytes)}
                </div>
              ))}
            </div>
          )}
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
        <div className="muted small err" role="alert" style={{ marginTop: 8 }}>{autoListErr}</div>
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
              aria-label={t('system.reminder.enabledLabel')}
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
              {/* R301：阈值下拉没标签，用户不知道这个数字是干什么的——
                  词条 autoListThreshold 写了没人用。 */}
              <span className="muted small">{t('system.reminder.autoListThreshold')}</span>
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
