import { useRef, useState } from 'react';
import { ShieldCheck, ShieldAlert, ShieldX, Lock, Unlock, RotateCcw } from 'lucide-react';
import { api } from '../api';
import {
  PERMS,
  loadPermConfig,
  savePermConfig,
  resetPermConfig,
  isPermEnabled,
  type PermDef,
} from '../permissions';
import { LEVEL_COLORS } from '../colors';
import { useT } from '../i18n';
import { useStore } from '../store';

// 存文案键而非中文：顶层常量在定义处求值会把中文烤死在首次 import，切语言不生效。
const LEVEL_META: Record<'L0' | 'L1' | 'L2' | 'L3', { labelKey: string; icon: typeof ShieldCheck; color: string; descKey: string }> = {
  L0: { labelKey: 'perm.level.l0.label', icon: ShieldCheck, color: LEVEL_COLORS.L0, descKey: 'perm.level.l0.desc' },
  L1: { labelKey: 'perm.level.l1.label', icon: ShieldAlert, color: LEVEL_COLORS.L1, descKey: 'perm.level.l1.desc' },
  L2: { labelKey: 'perm.level.l2.label', icon: ShieldX, color: LEVEL_COLORS.L2, descKey: 'perm.level.l2.desc' },
  L3: { labelKey: 'perm.level.l3.label', icon: Lock, color: LEVEL_COLORS.L3, descKey: 'perm.level.l3.desc' },
};

export function PermissionCenter() {
  const t = useT();
  const [cfg, setCfg] = useState(loadPermConfig());
  // R474：恢复默认是双向状态改写（会关掉用户手动打开的、也会重新打开出厂默认开启的），
  // 一键生效太危险——先给确认带说明影响再执行。
  const [confirmReset, setConfirmReset] = useState(false);
  const resetBtnRef = useRef<HTMLButtonElement>(null);
  const toast = useStore((s) => s.toast);

  // 权限快照同步到后端（纵深防御：后端写命令也要校验权限）。
  const pushToBackend = (next: typeof cfg) => {
    const enabled = (Object.entries(next.enabled) as [string, boolean][])
      .filter(([, v]) => v)
      .map(([k]) => k);
    api.syncPerms(enabled).catch(() => { /* 非关键路径，同步失败忽略 */ });
  };

  const toggle = (id: string) => {
    setConfirmReset(false);
    const next: typeof cfg = {
      enabled: { ...cfg.enabled, [id as keyof typeof cfg.enabled]: !cfg.enabled[id as keyof typeof cfg.enabled] },
    };
    savePermConfig(next);
    setCfg(next);
    pushToBackend(next);
  };

  const reset = () => {
    const next = resetPermConfig();
    setCfg(next);
    pushToBackend(next);
    setConfirmReset(false);
    toast(t('perm.center.resetDone'), 'ok');
    // 确认带卸载后焦点会掉到 body：还给「恢复默认」按钮。
    resetBtnRef.current?.focus();
  };

  const cancelReset = () => {
    setConfirmReset(false);
    resetBtnRef.current?.focus();
  };

  const groups: Array<'L0' | 'L1' | 'L2' | 'L3'> = ['L0', 'L1', 'L2', 'L3'];
  const rows = (level: 'L0' | 'L1' | 'L2' | 'L3') =>
    PERMS.filter((p) => p.level === level);

  return (
    <div className="perm-center">
      <div className="perm-legend">
        <span className="muted small">
          {t('perm.center.legend')}
        </span>
        <button ref={resetBtnRef} className="ghost small" onClick={() => setConfirmReset(true)} title={t('perm.center.resetTitle')}>
          <RotateCcw size={11} /> {t('perm.center.reset')}
        </button>
      </div>

      {confirmReset && (
        <div className="perm-reset-confirm" role="alert">
          <span className="perm-reset-text">
            {t('perm.center.resetConfirm')}
          </span>
          <div className="perm-reset-confirm-actions">
            <button type="button" className="ghost small" onClick={cancelReset} autoFocus>
              {t('perm.center.resetCancel')}
            </button>
            <button type="button" className="danger small" onClick={reset}>
              {t('perm.center.resetYes')}
            </button>
          </div>
        </div>
      )}

      {groups.map((lv) => {
        const meta = LEVEL_META[lv];
        const Icon = meta.icon;
        return (
          <div key={lv} className="perm-level">
            <div className="perm-level-head" style={{ color: meta.color }}>
              <Icon size={14} />
              <span>{t(meta.labelKey)}</span>
            </div>
            <div className="perm-level-desc muted small">{t(meta.descKey)}</div>
            <div className="perm-list">
              {rows(lv).map((p: PermDef) => {
                const on = isPermEnabled(p.id, cfg);
                return (
                  <div key={p.id} className="perm-row">
                    <div className="perm-row-main">
                      <span className="perm-name">{t(p.labelKey)}</span>
                      <span className="perm-desc">{t(p.descKey)}</span>
                    </div>
                    {lv === 'L3' ? (
                      <Lock size={14} className="perm-lock" />
                    ) : lv === 'L0' ? (
                      // L0 始终开启不可关：禁用态开关 + 常开视觉，点了也不会有「没反应」的错觉
                      <button type="button" role="switch" aria-checked={true} className="switch on" disabled aria-label={t(p.labelKey)} title={t('perm.center.l0Title')} />
                    ) : (
                      <button
                        type="button"
                        className={'switch' + (on ? ' on' : '')}
                        onClick={() => toggle(p.id)}
                        role="switch"
                        aria-checked={on}
                        aria-label={t(p.labelKey)}
                      />
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        );
      })}

      <div className="perm-foot">
        <Unlock size={12} /> {t('perm.center.foot')}
      </div>
    </div>
  );
}