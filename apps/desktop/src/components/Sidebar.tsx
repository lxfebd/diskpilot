import { History, LayoutGrid, ListTree, Loader2, PanelLeftClose, PanelLeftOpen, Settings as SettingsIcon, Trash2, Wrench } from 'lucide-react';
import { useT } from '../i18n';
import { PRIMARY_NAV_GROUPS, SECONDARY_NAV_ITEMS, type NavItem, type PageId } from '../nav';
import { Logo } from './Logo';

// 图标表按页面 ID 索引：新增页面时在 nav.ts 登记 + 在这里补一枚图标。
const ICONS: Record<PageId, typeof LayoutGrid> = {
  overview: LayoutGrid,
  workspace: ListTree,
  cleanup: Trash2,
  history: History,
  tools: Wrench,
  settings: SettingsIcon,
};

interface Props {
  /** 当前页（高亮项）；设置是模态入口，不会成为 current */
  current: PageId;
  onNavigate: (id: PageId) => void;
  expanded: boolean;
  onToggle: () => void;
  /** 后台忙的页面：图标位换成转圈（如扫描中的工作台） */
  busy?: PageId | null;
  /** 设置项右上角提示点（AI 已配置） */
  settingsDot?: boolean;
  /** 设置项的悬浮提示（AI 绑定状态）；折叠态被标签 tooltip 覆盖 */
  settingsHint?: string;
}

export function Sidebar({ current, onNavigate, expanded, onToggle, busy, settingsDot, settingsHint }: Props) {
  const t = useT();

  const item = (nav: NavItem) => {
    const Icon = ICONS[nav.id];
    const active = current === nav.id;
    const spinning = busy === nav.id;
    const label = t(nav.titleKey);
    // 折叠态只露图标，靠原生 tooltip 补全名称；展开态名称就在按钮上，
    // 仅设置项用 hint 覆盖（AI 绑定状态比重复标签更有信息量）。
    const hint = nav.id === 'settings' && settingsHint ? settingsHint : label;
    return (
      <button
        key={nav.id}
        className={'side-item' + (active ? ' active' : '')}
        onClick={() => onNavigate(nav.id)}
        title={expanded && nav.id !== 'settings' ? undefined : hint}
        aria-current={active ? 'page' : undefined}
        aria-busy={spinning || undefined}
      >
        <span className="side-item-ic">
          {spinning ? <Loader2 size={16} className="spin" /> : <Icon size={16} />}
          {nav.id === 'settings' && settingsDot && <span className="side-item-dot" />}
        </span>
        {expanded && <span className="side-item-label">{label}</span>}
      </button>
    );
  };

  return (
    <nav className={'app-sidebar' + (expanded ? '' : ' collapsed')} aria-label={t('shell.sidebar.ariaLabel')}>
      <div className="side-brand">
        <Logo size={20} />
        {expanded && <span>DiskPilot</span>}
      </div>

      <div className="side-nav">
        {PRIMARY_NAV_GROUPS.map((group) => (
          <div className="side-group" key={group.id}>
            {expanded && <div className="side-group-label">{t(group.titleKey)}</div>}
            {group.items.map(item)}
          </div>
        ))}
      </div>

      <div className="side-foot">
        {SECONDARY_NAV_ITEMS.map(item)}
        <button
          className="side-toggle"
          onClick={onToggle}
          title={expanded ? t('shell.sidebar.collapse') : t('shell.sidebar.expand')}
          aria-expanded={expanded}
        >
          {expanded ? <PanelLeftClose size={16} /> : <PanelLeftOpen size={16} />}
          {expanded && <span className="side-item-label">{t('shell.sidebar.collapse')}</span>}
        </button>
      </div>
    </nav>
  );
}
