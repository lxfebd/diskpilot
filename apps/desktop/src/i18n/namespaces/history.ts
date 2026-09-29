// 操作历史整页（P2c）的文案命名空间：过滤 / 汇总 / 分组 / 恢复反馈。
// 动作标签（回收站/隔离/删除）与恢复按钮文案复用 cleanup 命名空间的既有键，
// 两处口径不漂移。键前缀必须等于 ns（守卫测试强制）。
export const history = {
  ns: 'history',
  zh: {
    'history.title': '操作历史',
    'history.total': '共 {n} 条',
    'history.bytesFreed': '累计释放 {size}',
    'history.refresh': '刷新',
    'history.searchPlaceholder': '搜索路径或原因…',
    'history.filterAll': '全部',
    'history.today': '今天',
    'history.yesterday': '昨天',
    'history.unknownDay': '未知日期',
    'history.dayCount': '{n} 条',
    'history.loadFailed': '历史加载失败：{msg}',
    'history.empty': '还没有清理记录。执行清理后，每次操作（回收站 / 隔离 / 删除）都会记录在这里。',
    'history.emptyFiltered': '没有匹配的记录，换个条件试试。',
    'history.viewAll': '查看全部',
  },
  en: {
    'history.title': 'History',
    'history.total': '{n} entries',
    'history.bytesFreed': '{size} freed in total',
    'history.refresh': 'Refresh',
    'history.searchPlaceholder': 'Search path or reason…',
    'history.filterAll': 'All',
    'history.today': 'Today',
    'history.yesterday': 'Yesterday',
    'history.unknownDay': 'Unknown date',
    'history.dayCount': '{n} entries',
    'history.loadFailed': 'Failed to load history: {msg}',
    'history.empty': 'No cleanup history yet. Every operation (recycle / quarantine / delete) will be logged here.',
    'history.emptyFiltered': 'No matching entries — try different filters.',
    'history.viewAll': 'View all',
  },
};
