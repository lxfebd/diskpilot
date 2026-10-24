// 启动项管理面板（P4-4 抽取）：自包含状态 + 两步确认。
// 被「系统工具（设置）」与「系统优化页」两处复用，样式沿用 sys-* 系列。
import { useEffect, useState } from 'react';
import { Trash2, RefreshCw, Search } from 'lucide-react';
import { api, type StartupItem } from '../api';
import { isPermEnabled } from '../permissions';
import { useT } from '../i18n';
import { useFlashText } from '../flash';

// 启动项来源标签：switch 比较的是后端返回的标识符（不译），文案在渲染处求值——
// 模块顶层求值会把中文烤死在首次 import，切语言不再生效。未知来源原样显示。
function startupLocationLabel(t: (key: string) => string, location: string): string {
  switch (location) {
    case 'registry_hkcu':
      return t('system.startup.locHkcu');
    case 'registry_hklm':
      return t('system.startup.locHklm');
    case 'folder_user':
      return t('system.startup.locFolderUser');
    case 'folder_machine':
      return t('system.startup.locFolderMachine');
    default:
      return location;
  }
}

/** R164：启动项筛选——几十项时逐行找太累；搜索匹配 name/command/target，
 *  状态筛 all/enabled/disabled。纯函数：空词 + all 原样返回。 */
export type StartupFilter = 'all' | 'enabled' | 'disabled';

export function filterStartupItems(
  items: readonly StartupItem[],
  query: string,
  filter: StartupFilter,
): StartupItem[] {
  const q = query.trim().toLowerCase();
  return items.filter((it) => {
    if (filter === 'enabled' && !it.enabled) return false;
    if (filter === 'disabled' && it.enabled) return false;
    if (!q) return true;
    return (
      it.name.toLowerCase().includes(q) ||
      (it.command ?? '').toLowerCase().includes(q) ||
      (it.target ?? '').toLowerCase().includes(q)
    );
  });
}

/** R164：启用/停用计数——顶部一句「开机自启 N 项 · 已停用 M 项」。 */
export function startupCounts(items: readonly StartupItem[]): { on: number; off: number } {
  let on = 0;
  for (const it of items) if (it.enabled) on += 1;
  return { on, off: items.length - on };
}

/** R165：按来源分组的展示顺序——用户级（HKCU / 用户启动夹）在前，机器级
 *  （HKLM / 公共启动夹）在后：后者影响全机、改动风险更高，放后面让用户先
 *  处理自己有把握的。未知来源排在最后（不丢项）。 */
export const STARTUP_LOCATION_ORDER = [
  'registry_hkcu',
  'folder_user',
  'registry_hklm',
  'folder_machine',
] as const;

/** R165：把启动项按来源分组（组内保持原顺序）；未知来源归到 'other' 组。
 *  纯函数：不改入参，空输入 → 空数组。 */
export function groupStartupByLocation(
  items: readonly StartupItem[],
): { location: string; items: StartupItem[] }[] {
  const buckets = new Map<string, StartupItem[]>();
  for (const it of items) {
    const known = (STARTUP_LOCATION_ORDER as readonly string[]).includes(it.location);
    const key = known ? it.location : 'other';
    const arr = buckets.get(key) ?? [];
    arr.push(it);
    buckets.set(key, arr);
  }
  const order = [...STARTUP_LOCATION_ORDER, 'other'];
  const out: { location: string; items: StartupItem[] }[] = [];
  for (const loc of order) {
    const arr = buckets.get(loc);
    if (arr && arr.length > 0) out.push({ location: loc, items: arr });
  }
  return out;
}

export function StartupPanel() {
  const t = useT();
  const [startup, setStartup] = useState<StartupItem[] | null>(null);
  const [startupErr, setStartupErr] = useState<string | null>(null);
  const [startupLoading, setStartupLoading] = useState(false);
  // 两步确认：pendingId 待确认项（点了「禁用/删除」先变「确认？」按钮）。
  const [pendingToggle, setPendingToggle] = useState<string | null>(null);
  const [pendingDel, setPendingDel] = useState<string | null>(null);
  // R358：提示文案统一走 useFlashText（见 flash.ts）。
  const [msg, flashMsg] = useFlashText();
  // R164：搜索 + 状态筛选（几十项启动项时逐行找太累）。
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<StartupFilter>('all');

  const startupPerm = isPermEnabled('startup.manage');

  const loadStartup = async () => {
    setStartupLoading(true);
    setStartupErr(null);
    try {
      setStartup(await api.listStartupItems());
    } catch (e) {
      setStartupErr(String(e instanceof Error ? e.message : e));
    } finally {
      setStartupLoading(false);
    }
  };

  const toggleStartup = async (item: StartupItem, enable: boolean) => {
    // R261：执行时权限门（不只靠 UI 禁用）——startup.manage 是 L2 高危，
    // 关闭时任何入口（按钮/快捷键/脚本）都不许改启动项。
    if (!isPermEnabled('startup.manage')) {
      setStartupErr(t('system.startup.permOff'));
      return;
    }
    try {
      await api.setStartupItem(item.id, enable, true);
      flashMsg(t('system.startup.toastToggled', {
        action: enable ? t('system.startup.enabled') : t('system.startup.disabled'),
        name: item.name,
      }));
      await loadStartup();
    } catch (e) {
      setStartupErr(String(e instanceof Error ? e.message : e));
    }
  };

  const removeStartup = async (item: StartupItem) => {
    // R261：执行时权限门（删除启动项是 L2 高危，UI 禁用之外执行函数也拦）。
    if (!isPermEnabled('startup.manage')) {
      setStartupErr(t('system.startup.permOff'));
      return;
    }
    try {
      const backup = await api.removeStartupItem(item.id, true);
      // 回收站接住的（路径为空）走系统回收站还原；没接住 / 注册表值走应用自己的备份，
      // 路径必须报给用户——那是唯一的退路。
      flashMsg(backup
        ? (item.location.startsWith('registry')
            ? t('system.startup.toastDeletedReg', { name: item.name, path: backup })
            : t('system.startup.toastDeletedFile', { name: item.name, path: backup }))
        : t('system.startup.toastDeleted', { name: item.name }));
      await loadStartup();
    } catch (e) {
      setStartupErr(String(e instanceof Error ? e.message : e));
    }
  };

  useEffect(() => {
    if (startupPerm) loadStartup();
  }, [startupPerm]);

  // R164：统计与筛选结果（纯函数计算，渲染只读）。
  const counts = startupCounts(startup ?? []);
  const visible = filterStartupItems(startup ?? [], query, filter);
  // R165：筛选结果按来源分组（用户级在前、机器级在后）。
  const groups = groupStartupByLocation(visible);

  return (
    <>
      <div className="settings-group-title">{t('system.startup.title')}</div>
      <div className="hint" style={{ marginBottom: 8 }}>
        <span>
          {t('system.startup.hintA')} <code>.disabled</code>{' '}
          {t('system.startup.hintB')}
          {!startupPerm && (
            <strong style={{ color: 'var(--risk-caution)' }}> {t('system.startup.permOff')}</strong>
          )}
        </span>
      </div>
      {msg && <div className="ok" role="status">{msg}</div>}
      {startupErr && <div className="error">{startupErr}</div>}
      {startupLoading && <div className="muted small">{t('system.startup.loading')}</div>}
      {startup && startup.length === 0 && !startupLoading && (
        <div className="muted small">{t('system.startup.empty')}</div>
      )}
      {startup && startup.length > 0 && (
        <>
          {/* R164：统计 + 筛选——「开机自启 N 项 · 已停用 M 项」一眼看清现状 */}
          <div className="sys-filter-bar">
            <span className="muted small">
              {t('system.startup.counts', { on: counts.on, off: counts.off })}
            </span>
            <span className="sys-filter-chips">
              {(['all', 'enabled', 'disabled'] as StartupFilter[]).map((f) => (
                <button
                  key={f}
                  type="button"
                  className={'chip' + (filter === f ? ' active' : '')}
                  onClick={() => setFilter(f)}
                >
                  {f === 'all' ? t('system.startup.filterAll')
                    : f === 'enabled' ? t('system.startup.filterOn')
                    : t('system.startup.filterOff')}
                </button>
              ))}
            </span>
            <label className="sys-filter-search">
              <Search size={13} />
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t('system.startup.searchPlaceholder')}
              />
            </label>
          </div>
          {visible.length === 0 && (
            <div className="muted small">{t('system.startup.emptyFiltered')}</div>
          )}
        </>
      )}
      {startup && startup.length > 0 && visible.length > 0 && (
        <div className="sys-list">
          {groups.map((g) => (
            <div key={g.location} className="sys-group">
              {/* R165：分组标题——用户级在前、机器级在后（后者影响全机） */}
              <div className="sys-group-head muted small">
                {g.location === 'other' ? t('system.startup.locOther') : startupLocationLabel(t, g.location)}
                <span className="sys-group-n">{g.items.length}</span>
              </div>
              {g.items.map((item) => (
            <div key={item.id} className={'sys-row' + (item.enabled ? '' : ' sys-row-off')}>
              <div className="sys-row-main">
                <span className="sys-name">{item.name}</span>
                <span className="sys-sub muted">
                  {startupLocationLabel(t, item.location)}
                  {item.command ? ` · ${item.command.length > 70 ? item.command.slice(0, 70) + '…' : item.command}` : ''}
                </span>
              </div>
              <div className="sys-row-actions">
                {item.location.startsWith('registry') && (
                  <button
                    type="button"
                    className="ghost icon"
                    title={t('system.startup.delRegistryTitle')}
                    onClick={() => setPendingDel(pendingDel === item.id ? null : item.id)}
                  >
                    <Trash2 size={13} />
                  </button>
                )}
                {item.location.startsWith('folder') && (
                  <button
                    type="button"
                    className="ghost icon"
                    title={t('system.startup.delFolderTitle')}
                    onClick={() => setPendingDel(pendingDel === item.id ? null : item.id)}
                  >
                    <Trash2 size={13} />
                  </button>
                )}
                {pendingDel === item.id ? (
                  <span className="sys-confirm">
                    <span className="muted small">{t('system.common.confirmDelete')}</span>
                    <button type="button" className="danger small" onClick={() => removeStartup(item)}>{t('system.common.delete')}</button>
                    <button type="button" className="ghost small" onClick={() => setPendingDel(null)}>{t('system.common.cancel')}</button>
                  </span>
                ) : pendingToggle === item.id ? (
                  <span className="sys-confirm">
                    <span className="muted small">{item.enabled ? t('system.startup.confirmDisable') : t('system.startup.confirmRestore')}</span>
                    <button
                      type="button"
                      className={item.enabled ? 'danger small' : 'primary small'}
                      onClick={() => toggleStartup(item, !item.enabled)}
                    >
                      {item.enabled ? t('system.common.disable') : t('system.common.restore')}
                    </button>
                    <button type="button" className="ghost small" onClick={() => setPendingToggle(null)}>{t('system.common.cancel')}</button>
                  </span>
                ) : (
                  <button
                    type="button"
                    className={'switch' + (item.enabled ? ' on' : '')}
                    aria-checked={item.enabled}
                    aria-label={t('system.startup.ariaToggle', { action: item.enabled ? t('system.common.disable') : t('system.common.restore'), name: item.name })}
                    title={item.enabled ? t('system.startup.switchOffTitle') : t('system.startup.switchOnTitle')}
                    onClick={() => setPendingToggle(item.id)}
                  />
                )}
              </div>
            </div>
              ))}
            </div>
          ))}
        </div>
      )}
      <button type="button" className="ghost small" onClick={loadStartup} disabled={startupLoading || !startupPerm}>
        <RefreshCw size={11} className={startupLoading ? 'spin' : undefined} /> {t('system.startup.refresh')}
      </button>
    </>
  );
}
