// 电源计划切换卡（P4-4）：枚举可选电源计划（只读），切换走 power.plan L2
// 权限（权限中心开关 + 执行时两步确认）。系统电源计划切换即时生效，
// 属于系统级写操作，绝不静默执行。
import { useEffect, useState } from 'react';
import { RefreshCw, Zap } from 'lucide-react';
import { api, type PowerPlanOut } from '../api';
import { isPermEnabled } from '../permissions';
import { useT } from '../i18n';

export function PowerPlanCard() {
  const t = useT();
  const [plans, setPlans] = useState<PowerPlanOut | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  // 两步确认：pending 待切换计划 id；主按钮常驻为列表单选。
  const [pending, setPending] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const perm = isPermEnabled('power.plan');

  // 标准 GUID → 可读名：toast 不再直显 8-4-4-4-12 长串。
  const GUID_NAMES: Record<string, string> = {
    '381b4222-f694-41f0-9685-ff5bb260df2e': t('optimizer.power.planBalanced'),
    '8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c': t('optimizer.power.planHighPerf'),
    'a1841308-3541-4fab-bc81-f71556f20b4a': t('optimizer.power.planSaver'),
  };
  const friendlyName = (scheme: string) => {
    const lower = scheme.toLowerCase();
    return GUID_NAMES[lower] ?? scheme;
  };

  const flashMsg = (s: string) => {
    setMsg(s);
    setTimeout(() => setMsg(null), 3500);
  };

  const load = async () => {
    setLoading(true);
    setErr(null);
    try {
      setPlans(await api.powerPlan());
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setLoading(false);
    }
  };

  const apply = async (scheme: string) => {
    try {
      setBusy(true);
      await api.powerPlan(scheme, true);
      flashMsg(t('optimizer.power.switched', { name: friendlyName(scheme) }));
      setPending(null);
      await load();
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (perm) load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [perm]);

  return (
    <>
      <div className="settings-group-title">{t('optimizer.power.title')}</div>
      <div className="hint" style={{ marginBottom: 8 }}>
        <span>
          {t('optimizer.power.hint')}
          {!perm && (
            <strong style={{ color: 'var(--ink-1)' }}> {t('optimizer.power.permOff')}</strong>
          )}
        </span>
      </div>
      {msg && <div className="ok">{msg}</div>}
      {err && <div className="error">{err}</div>}
      {loading && <div className="muted small">{t('system.startup.loading')}</div>}
      {plans && plans.schemes.length === 0 && !loading && (
        <div className="muted small">{t('optimizer.power.empty')}</div>
      )}
      {plans && plans.schemes.length > 0 && (
        <div className="sys-list">
          {plans.schemes.map((scheme) => {
            const active = plans.active === scheme;
            const confirming = pending === scheme;
            return (
              <div key={scheme} className={'sys-row' + (active ? ' sys-row-active' : '')}>
                <div className="sys-row-main">
                  <span className="sys-name">
                    {active ? <Zap size={13} style={{ verticalAlign: '-2px', marginRight: 4 }} /> : null}
                    {friendlyName(scheme)}
                    {active ? <span className="muted small"> · {t('optimizer.power.activeTag')}</span> : null}
                  </span>
                </div>
                <div className="sys-row-actions">
                  {confirming ? (
                    <span className="sys-confirm">
                      <span className="muted small">{t('optimizer.power.confirm')}</span>
                      <button type="button" className="primary small" onClick={() => apply(scheme)} disabled={busy}>
                        {t('optimizer.power.apply')}
                      </button>
                      <button type="button" className="ghost small" onClick={() => setPending(null)} disabled={busy}>
                        {t('system.common.cancel')}
                      </button>
                    </span>
                  ) : (
                    <button
                      type="button"
                      className="ghost small"
                      disabled={active || !perm}
                      onClick={() => setPending(scheme)}
                    >
                      {active ? t('optimizer.power.inUse') : t('optimizer.power.switchTo')}
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
      <button type="button" className="ghost small" onClick={load} disabled={loading || !perm}>
        <RefreshCw size={11} className={loading ? 'spin' : undefined} /> {t('system.startup.refresh')}
      </button>
    </>
  );
}