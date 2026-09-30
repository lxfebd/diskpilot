// 启动项管理面板（P4-4 抽取）：自包含状态 + 两步确认。
// 被「系统工具（设置）」与「系统优化页」两处复用，样式沿用 sys-* 系列。
import { useEffect, useState } from 'react';
import { Trash2, RefreshCw } from 'lucide-react';
import { api, type StartupItem } from '../api';
import { isPermEnabled } from '../permissions';
import { useT } from '../i18n';

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

export function StartupPanel() {
  const t = useT();
  const [startup, setStartup] = useState<StartupItem[] | null>(null);
  const [startupErr, setStartupErr] = useState<string | null>(null);
  const [startupLoading, setStartupLoading] = useState(false);
  // 两步确认：pendingId 待确认项（点了「禁用/删除」先变「确认？」按钮）。
  const [pendingToggle, setPendingToggle] = useState<string | null>(null);
  const [pendingDel, setPendingDel] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const startupPerm = isPermEnabled('startup.manage');

  const flashMsg = (s: string) => {
    setMsg(s);
    setTimeout(() => setMsg(null), 3500);
  };

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
    try {
      await api.removeStartupItem(item.id, true);
      flashMsg(t('system.startup.toastDeleted', { name: item.name }));
      await loadStartup();
    } catch (e) {
      setStartupErr(String(e instanceof Error ? e.message : e));
    }
  };

  useEffect(() => {
    if (startupPerm) loadStartup();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [startupPerm]);

  return (
    <>
      <div className="settings-group-title">{t('system.startup.title')}</div>
      <div className="hint" style={{ marginBottom: 8 }}>
        <span>
          {t('system.startup.hintA')} <code>.disabled</code>{' '}
          {t('system.startup.hintB')}
          {!startupPerm && (
            <strong style={{ color: 'var(--ink-1)' }}> {t('system.startup.permOff')}</strong>
          )}
        </span>
      </div>
      {msg && <div className="ok">{msg}</div>}
      {startupErr && <div className="error">{startupErr}</div>}
      {startupLoading && <div className="muted small">{t('system.startup.loading')}</div>}
      {startup && startup.length === 0 && !startupLoading && (
        <div className="muted small">{t('system.startup.empty')}</div>
      )}
      {startup && startup.length > 0 && (
        <div className="sys-list">
          {startup.map((item) => (
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
      )}
      <button type="button" className="ghost small" onClick={loadStartup} disabled={startupLoading || !startupPerm}>
        <RefreshCw size={11} className={startupLoading ? 'spin' : undefined} /> {t('system.startup.refresh')}
      </button>
    </>
  );
}
