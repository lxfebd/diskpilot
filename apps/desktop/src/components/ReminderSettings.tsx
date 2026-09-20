import { useEffect, useState } from 'react';
import { Play, BellOff } from 'lucide-react';
import { api } from '../api';
import type { ReminderConfig, ReminderPayload } from '../api';
import { formatBytes } from '../format';
import { useStore } from '../store';
import { useT } from '../i18n';

/**
 * 清理提醒设置（R3）：开关 + 间隔 + 阈值 + 手动检查一次。
 * 铁律：只出清单 + 通知，绝不自动清理——检查按钮也只跑引擎真算并发通知，
 * 清理仍走总览页确认窗（execute_ai_plan user_confirmed）。
 */
export function ReminderSettings() {
  const t = useT();
  const [cfg, setCfg] = useState<ReminderConfig | null>(null);
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const [lastPayload, setLastPayload] = useState<ReminderPayload | null>(null);

  // 打开时同步后端配置
  useEffect(() => {
    let cancelled = false;
    api.getReminderConfig().then((c) => {
      if (!cancelled) setCfg(c);
    });
    return () => {
      cancelled = true;
    };
  }, []);

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
    </div>
  );
}
