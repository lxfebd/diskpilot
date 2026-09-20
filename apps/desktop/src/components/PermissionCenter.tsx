import { useState } from 'react';
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
import { zhText, useT } from '../i18n';

// 「即将上线」历史上是写在权限说明里的标记，判定只看中文原文（与显示语言无关）。
// 这是数据标记不是文案，故不参与翻译。
const COMING_SOON_MARK = '（即将上线）'; // @i18n-keep

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

  // 权限快照同步到后端（纵深防御：后端写命令也要校验权限）。
  const pushToBackend = (next: typeof cfg) => {
    const enabled = (Object.entries(next.enabled) as [string, boolean][])
      .filter(([, v]) => v)
      .map(([k]) => k);
    api.syncPerms(enabled).catch(() => { /* 非关键路径，同步失败忽略 */ });
  };

  const toggle = (id: string) => {
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
        <button className="ghost small" onClick={reset} title={t('perm.center.resetTitle')}>
          <RotateCcw size={11} /> {t('perm.center.reset')}
        </button>
      </div>

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
                const comingSoon = zhText(p.descKey).includes(COMING_SOON_MARK);
                return (
                  <div key={p.id} className={'perm-row' + (comingSoon ? ' coming-soon' : '')}>
                    <div className="perm-row-main">
                      <span className="perm-name">{t(p.labelKey)}</span>
                      <span className="perm-desc">{t(p.descKey)}</span>
                    </div>
                    {lv === 'L3' ? (
                      <Lock size={14} className="perm-lock" />
                    ) : lv === 'L0' ? (
                      // L0 始终开启不可关：禁用态开关 + 常开视觉，点了也不会有「没反应」的错觉
                      <button type="button" className="switch on" disabled aria-label={t(p.labelKey)} title={t('perm.center.l0Title')} />
                    ) : comingSoon ? (
                      <span className="perm-coming">{t('perm.center.comingSoon')}</span>
                    ) : (
                      <button
                        type="button"
                        className={'switch' + (on ? ' on' : '')}
                        onClick={() => toggle(p.id)}
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