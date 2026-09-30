// 系统优化页（P4-4）：电源计划切换的界面文案。
// 启动项管理文案与「设置 → 系统工具」共用 system.startup.*（同一面板）。
// 口径：后端返回的电源计划 guid 原值不译（schemes 是系统标识）；
// 但已知三个标准 guid 前端映射为标准名（optimizer.power.planBalanced 等），
// 列表与 toast 不再直显 36 位长串。
export const optimizer = {
  ns: 'optimizer',
  zh: {
    // ── 电源计划切换 ──
    'optimizer.power.title': '电源计划',
    'optimizer.power.hint': '枚举系统电源计划并切换当前生效计划。切换立即生效，耗电与性能随之变化。',
    'optimizer.power.permOff': '当前未开启「电源计划」权限 — 请先到 AI 权限中心打开开关。',
    'optimizer.power.empty': '没有检测到可用的电源计划。',
    'optimizer.power.activeTag': '使用中',
    'optimizer.power.inUse': '使用中',
    'optimizer.power.switchTo': '切换',
    'optimizer.power.confirm': '确认切换该电源计划？',
    'optimizer.power.apply': '切换',
    'optimizer.power.switched': '已切换到电源计划：{name}',
    'optimizer.power.planBalanced': '平衡',
    'optimizer.power.planHighPerf': '高性能',
    'optimizer.power.planSaver': '节能',
  },
  en: {
    'optimizer.power.title': 'Power plan',
    'optimizer.power.hint': 'List system power plans and switch the active one. Applies immediately, changing power draw and performance.',
    'optimizer.power.permOff': 'Power-plan permission is off — enable it under the permission hub first.',
    'optimizer.power.empty': 'No power plans detected.',
    'optimizer.power.activeTag': 'Active',
    'optimizer.power.inUse': 'Active',
    'optimizer.power.switchTo': 'Switch',
    'optimizer.power.confirm': 'Switch to this power plan?',
    'optimizer.power.apply': 'Switch',
    'optimizer.power.switched': 'Switched to power plan: {name}',
    'optimizer.power.planBalanced': 'Balanced',
    'optimizer.power.planHighPerf': 'High performance',
    'optimizer.power.planSaver': 'Power saver',
  },
};