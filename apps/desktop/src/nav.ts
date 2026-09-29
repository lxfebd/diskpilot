// ── 导航模型 ────────────────────────────────────────────────────────
// 页面 ID 是稳定标识：localStorage 持久化、事件跳转、busy/notice 广播都用它，
// 不随语言变化。标题文案走 shell 命名空间，这里只存键名，渲染时求值。
//
// 分组对标成熟桌面工具的信息架构：一级按「空间 / 系统工具」分域，
// 二级（设置）固定在侧边栏底部。新增页面时先在这里登记，再在 App 里接线。

export const PAGE_IDS = {
  overview: 'overview',
  workspace: 'workspace',
  cleanup: 'cleanup',
  history: 'history',
  tools: 'tools',
  settings: 'settings',
} as const;

export type PageId = (typeof PAGE_IDS)[keyof typeof PAGE_IDS];

export interface NavItem {
  id: PageId;
  /** 侧边栏标签与页头标题共用的文案键 */
  titleKey: string;
  /** 页头副标题文案键（可选） */
  subtitleKey?: string;
}

export interface NavGroup {
  id: string;
  /** 分组小标题（仅展开态可见） */
  titleKey: string;
  items: NavItem[];
}

export const PRIMARY_NAV_GROUPS: NavGroup[] = [
  {
    id: 'space',
    titleKey: 'shell.sidebar.groups.space',
    items: [
      { id: PAGE_IDS.overview, titleKey: 'shell.nav.overview', subtitleKey: 'shell.nav.overviewTitle' },
      { id: PAGE_IDS.workspace, titleKey: 'shell.nav.workspace', subtitleKey: 'shell.nav.workspaceTitle' },
      { id: PAGE_IDS.cleanup, titleKey: 'shell.nav.cleanup', subtitleKey: 'shell.nav.cleanupTitle' },
      { id: PAGE_IDS.history, titleKey: 'shell.nav.history', subtitleKey: 'shell.nav.historyTitle' },
    ],
  },
  {
    id: 'system',
    titleKey: 'shell.sidebar.groups.system',
    items: [{ id: PAGE_IDS.tools, titleKey: 'shell.nav.tools', subtitleKey: 'shell.nav.toolsTitle' }],
  },
];

/** 底部固定区：设置等全局入口 */
export const SECONDARY_NAV_ITEMS: NavItem[] = [
  { id: PAGE_IDS.settings, titleKey: 'shell.nav.settings', subtitleKey: 'shell.nav.settingsTitle' },
];

/** 按 ID 取导航项（页头标题/副标题查表用） */
export function navItemById(id: PageId): NavItem | undefined {
  const all = [...PRIMARY_NAV_GROUPS.flatMap((g) => g.items), ...SECONDARY_NAV_ITEMS];
  return all.find((it) => it.id === id);
}

// ── 侧边栏折叠三态 ──────────────────────────────────────────────────
// 显式的用户选择（preferredExpanded）与响应式断点判断（wideViewport）分开记：
// 窗口缩窄时自动折叠保护内容区，但窗口恢复宽度时回到用户上次的选择，
// 而不是每次都强制展开——否则用户「收起侧边栏」的意图会被一次缩放吞掉。
export const SIDEBAR_EXPANDED_MIN_WIDTH_PX = 1100;

// 侧栏宽度：与 styles/tokens.css 的 --layout-sidebar-*-width 成对存在
// （CSS 负责渲染、这里负责内容区宽度预算，两处必须同步改）。
export const SIDEBAR_EXPANDED_WIDTH_PX = 208;
export const SIDEBAR_COLLAPSED_WIDTH_PX = 56;

export interface SidebarLayoutState {
  expanded: boolean;
  preferredExpanded: boolean;
  wideViewport: boolean;
}

export function isWideViewport(viewportWidth: number): boolean {
  return viewportWidth >= SIDEBAR_EXPANDED_MIN_WIDTH_PX;
}

/**
 * 初始折叠态。`preferredExpanded` 由调用方从持久化偏好传入（缺省展开）：
 * 「用户上次是收起还是展开」要跨会话保留，否则每次启动都被强制展开一次。
 */
export function createSidebarLayoutState(viewportWidth: number, preferredExpanded = true): SidebarLayoutState {
  const wideViewport = isWideViewport(viewportWidth);
  return { expanded: wideViewport && preferredExpanded, preferredExpanded, wideViewport };
}

export function resizeSidebarLayout(state: SidebarLayoutState, viewportWidth: number): SidebarLayoutState {
  const wideViewport = isWideViewport(viewportWidth);
  if (wideViewport === state.wideViewport) return state;
  return {
    expanded: wideViewport ? state.preferredExpanded : false,
    preferredExpanded: state.preferredExpanded,
    wideViewport,
  };
}

export function toggleSidebarLayout(state: SidebarLayoutState): SidebarLayoutState {
  const expanded = !state.expanded;
  return { expanded, preferredExpanded: expanded, wideViewport: state.wideViewport };
}
