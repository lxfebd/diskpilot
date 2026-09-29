// 导航模型守卫：键名 / 分组结构 / 折叠三态纯函数。
// i18n 覆盖测试只校验源码里静态的 `t('字面量')`，而侧栏与页头是
// `t(nav.titleKey)` 这种动态取值——键名打错不会红，只会在界面上显示键名本身。
// 这里把「nav.ts 声明的每个键都能在文案表命中」钉成测试补上这个洞。
import { describe, it, expect } from 'vitest';
import {
  PAGE_IDS,
  PRIMARY_NAV_GROUPS,
  SECONDARY_NAV_ITEMS,
  SIDEBAR_EXPANDED_MIN_WIDTH_PX,
  createSidebarLayoutState,
  navItemById,
  resizeSidebarLayout,
  toggleSidebarLayout,
} from './nav';
import { zhKeys, enKeys } from './i18n';

describe('导航模型', () => {
  it('每个导航项与分组的文案键都在中英表里存在', () => {
    const zh = new Set(zhKeys());
    const en = new Set(enKeys());
    const keys = [
      ...PRIMARY_NAV_GROUPS.map((g) => g.titleKey),
      ...PRIMARY_NAV_GROUPS.flatMap((g) => g.items.flatMap((i) => [i.titleKey, i.subtitleKey])),
      ...SECONDARY_NAV_ITEMS.flatMap((i) => [i.titleKey, i.subtitleKey]),
    ].filter((k): k is string => typeof k === 'string');

    expect(keys.filter((k) => !zh.has(k)), '中文表缺键').toEqual([]);
    expect(keys.filter((k) => !en.has(k)), '英文表缺键').toEqual([]);
  });

  it('页面 ID 唯一，且 navItemById 能查到每个一级页面', () => {
    const ids = [
      ...PRIMARY_NAV_GROUPS.flatMap((g) => g.items.map((i) => i.id)),
      ...SECONDARY_NAV_ITEMS.map((i) => i.id),
    ];
    expect(new Set(ids).size, '有重复的页面 ID').toBe(ids.length);
    for (const id of Object.values(PAGE_IDS)) {
      expect(navItemById(id)?.id, `${id} 查不到导航项`).toBe(id);
    }
  });

  it('未知 ID 返回 undefined 而不是抛错', () => {
    expect(navItemById('nope' as never)).toBeUndefined();
  });
});

describe('侧边栏折叠三态', () => {
  it('宽视口初始展开，窄视口初始折叠', () => {
    expect(createSidebarLayoutState(SIDEBAR_EXPANDED_MIN_WIDTH_PX).expanded).toBe(true);
    expect(createSidebarLayoutState(SIDEBAR_EXPANDED_MIN_WIDTH_PX - 1).expanded).toBe(false);
  });

  it('缩窄到断点以下自动折叠，恢复宽度回到用户上次的选择', () => {
    // 用户在宽视口下显式收起
    let s = toggleSidebarLayout(createSidebarLayoutState(1400));
    expect(s.expanded).toBe(false);
    expect(s.preferredExpanded).toBe(false);

    // 缩窄：保持折叠
    s = resizeSidebarLayout(s, 900);
    expect(s.expanded).toBe(false);

    // 恢复宽度：回到「用户想收起」而不是被强制展开
    s = resizeSidebarLayout(s, 1400);
    expect(s.expanded).toBe(false);
    expect(s.preferredExpanded).toBe(false);
  });

  it('宽度未跨断点时返回同一引用（避免无谓重渲）', () => {
    const s = createSidebarLayoutState(1400);
    expect(resizeSidebarLayout(s, 1300)).toBe(s);
  });

  it('展开/收起切换会更新用户偏好', () => {
    const s = createSidebarLayoutState(1400);
    const collapsed = toggleSidebarLayout(s);
    expect(collapsed.preferredExpanded).toBe(false);
    const expanded = toggleSidebarLayout(collapsed);
    expect(expanded.expanded).toBe(true);
    expect(expanded.preferredExpanded).toBe(true);
  });

  it('持久化的「收起」偏好在启动时立即生效，不被默认展开覆盖', () => {
    const s = createSidebarLayoutState(1400, false);
    expect(s.expanded).toBe(false);
    expect(s.preferredExpanded).toBe(false);
    // 之后再缩放也不会被强制展开
    expect(resizeSidebarLayout(s, 900).expanded).toBe(false);
    expect(resizeSidebarLayout(s, 1400).expanded).toBe(false);
  });

  it('偏好为展开时，窄视口仍按响应式规则折叠', () => {
    const s = createSidebarLayoutState(900, true);
    expect(s.expanded).toBe(false);
    expect(s.preferredExpanded).toBe(true);
  });
});
