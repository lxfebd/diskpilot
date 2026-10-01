// 系统优化页（P4-4）：启动项管理 + 电源计划切换。
// 启动项面板与「设置 → 系统工具」共用 StartupPanel（同一份 L2 确认逻辑），
// 电源计划是独立卡片（power.plan L2）。两块都是权限中心可开关 + 两步确认。
import { useT } from '../i18n';
import { ErrorBoundary } from './ErrorBoundary';
import { StartupPanel } from './StartupPanel';
import { PowerPlanCard } from './PowerPlanCard';

export function OptimizerPage() {
  const t = useT();
  return (
    <div className="optimizer-page">
      <ErrorBoundary fallbackLabel={t('shell.boundary.optimizerFailed')}>
        <StartupPanel />
        <PowerPlanCard />
      </ErrorBoundary>
    </div>
  );
}