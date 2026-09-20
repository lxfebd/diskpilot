// 报错与失败提示：数据层（store 的 toast、api/* 的前端拒绝）在这一层出的文案。
// 后端 error string 原文（本来就是中文）一律透传显示，不在这里译。
// 确认门相关的四条（confirm*）是「铁律」提示语：只搬文案，判定条件与抛错时机都在调用处。
export const errors = {
  ns: 'errors',
  zh: {
    // 浏览器（非 Tauri）环境下调用桌面端命令
    'errors.tauriOnly': '{cmd} 仅在 Tauri 环境可用',
    'errors.agentDesktopOnly': 'agent-server 工具集仅在桌面端可用',

    // 清理确认门（前端第一道闸的拒绝语）
    'errors.confirmExecuteScope': '真实清理必须带 confirmed=true（用户已确认）',
    'errors.confirmAiPlan': '需要用户确认后才可执行清理',
    'errors.confirmRecyclePaths': '移入回收站必须经用户确认（confirmed=true）',
    'errors.confirmToolbeltRecycle': '回收工具目录必须经用户确认（confirmed=true）',
    // 插件/系统写操作确认门（批次8：API 层不再烤死 confirmed=true）
    'errors.confirmPluginWrite': '插件写操作必须经用户确认（confirmed=true）',
    'errors.confirmSystemWrite': '系统写操作必须经用户确认（confirmed=true）',

    // 执行失败 / 未授权
    'errors.recycleFailed': '{n} 项回收失败（可能被占用或需要管理员权限）',
    'errors.permExecuteMissing': '清理执行未授权：请前往「设置 → AI 权限中心」开启「执行清理计划」后再试',
    'errors.pluginManageMissing': '未开启「管理插件」权限（plugin.manage）。请先在权限中心开启后重试。',
  },
  en: {
    'errors.tauriOnly': '{cmd} is only available in the desktop (Tauri) runtime',
    'errors.agentDesktopOnly': 'The agent-server toolset is only available in the desktop app',

    'errors.confirmExecuteScope': 'A real cleanup run requires confirmed=true (user has confirmed)',
    'errors.confirmAiPlan': 'Cleanup can only be executed after you confirm it',
    'errors.confirmRecyclePaths': 'Moving items to the Recycle Bin requires your confirmation (confirmed=true)',
    'errors.confirmToolbeltRecycle': 'Recycling a tool directory requires your confirmation (confirmed=true)',
    'errors.confirmPluginWrite': 'Plugin write operations require user confirmation (confirmed=true)',
    'errors.confirmSystemWrite': 'System write operations require user confirmation (confirmed=true)',

    'errors.recycleFailed': '{n} items failed to recycle (possibly in use or needs admin rights)',
    'errors.permExecuteMissing': 'Cleanup execution is not authorized: enable "Execute cleanup plan" under Settings → AI Permissions, then try again.',
    'errors.pluginManageMissing': 'The "Manage plugins" permission (plugin.manage) is off. Enable it in the Permission Center first.',
  },
};
