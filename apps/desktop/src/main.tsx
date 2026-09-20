import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { ErrorBoundary } from './components/ErrorBoundary';
import { initTheme } from './theme';
import { api } from './api';
import { loadPermConfig } from './permissions';
// 入口不在组件里，用模块级 t()：在渲染调用点求值，不在模块顶层烤死文案。
import { t } from './i18n';
import './styles/tokens.css';
import './styles/layout.css';
import './styles/chat.css';
import './styles/steam.css';
import './styles/settings.css';
import './styles/overview.css';
import './styles/toolwall.css';
import './styles/dark.css';
import './styles/disclaimer.css';

initTheme();

// 启动时把权限快照同步到后端：后端写命令（execute_ai_plan / toolbelt_run /
// plugin_*）需要据此做纵深校验，确保重启后权限状态不脱节。
{
  const cfg = loadPermConfig();
  const enabled = (Object.entries(cfg.enabled) as [string, boolean][])
    .filter(([, v]) => v)
    .map(([k]) => k);
  api.syncPerms(enabled).catch(() => { /* 非关键路径，失败忽略 */ });
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ErrorBoundary fallbackLabel={t('shell.boundary.appRender')}>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
