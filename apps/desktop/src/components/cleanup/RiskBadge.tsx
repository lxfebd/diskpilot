// 风险徽标：把两套风险等级体系统一成一处渲染——
//   Risk       = 'low'|'medium'|'high'   ——scaffold 级（types.ts，脚本整体风险）
//   CleanupRisk= 'safe'|'caution'|'danger'——AI 提案项级（store.ts，单条路径风险）
// 三段色映射到既有 token：safe/low=绿、caution/medium=黄、danger/high=红。
// CleanupProposalDialog 原来手写 RISK_META 表，现在改从这里取，保持文案/图标/cls 一致。
import { ShieldCheck, AlertTriangle, ShieldAlert, type LucideIcon } from 'lucide-react';
import { useT } from '../../i18n';
import type { Risk } from '../../types';
import type { CleanupRisk } from '../../store';

export type AnyRisk = Risk | CleanupRisk;

interface RiskMeta {
  labelKey: string;
  icon: LucideIcon;
  cls: string;
}

export const RISK_META: Record<AnyRisk, RiskMeta> = {
  // AI 提案项级（cleanup 命名空间，已有键）
  safe: { labelKey: 'cleanup.riskSafe', icon: ShieldCheck, cls: 'risk-safe' },
  caution: { labelKey: 'cleanup.riskCaution', icon: AlertTriangle, cls: 'risk-caution' },
  danger: { labelKey: 'cleanup.riskDanger', icon: ShieldAlert, cls: 'risk-danger' },
  // scaffold 级（新键 cleanup.riskLow/Medium/High）
  low: { labelKey: 'cleanup.riskLow', icon: ShieldCheck, cls: 'risk-safe' },
  medium: { labelKey: 'cleanup.riskMedium', icon: AlertTriangle, cls: 'risk-caution' },
  high: { labelKey: 'cleanup.riskHigh', icon: ShieldAlert, cls: 'risk-danger' },
};

export const RISK_ORDER: CleanupRisk[] = ['safe', 'caution', 'danger'];

interface Props {
  risk: AnyRisk;
  /** 默认带文字标签；列表里只要色块时传 false。 */
  showLabel?: boolean;
}

export function RiskBadge({ risk, showLabel = true }: Props) {
  const t = useT();
  const meta = RISK_META[risk];
  const Icon = meta.icon;
  return (
    <span className={`risk-badge ${meta.cls}`} title={t(meta.labelKey)}>
      <Icon size={11} />
      {showLabel && <span>{t(meta.labelKey)}</span>}
    </span>
  );
}
