import { X } from 'lucide-react';
import { useT } from '../i18n';
import { useEscapeKey } from '../escapeKey';
import { useDialogFocus } from '../dialogFocus';

// R494：键盘快捷键帮助面板——? 键全局唤出（监听挂在 App.tsx）。应用里散布着
// Esc 关窗、Shift+F10 右键菜单、搜索框 Esc 清空、方向键走行/走卡、空格翻页等
// 快捷键，但此前没有一处集中可查，键盘用户只能靠摸索发现。这里集中列出，
// 也给读屏用户一份完整文字清单（弹窗本身接入 R349 的 Esc 栈与焦点管理）。
export function KeyboardHelp({ open, onClose }: { open: boolean; onClose: () => void }) {
  const t = useT();
  useEscapeKey(onClose, open);
  useDialogFocus(open);
  if (!open) return null;
  const groups: { title: string; keys: string[] }[] = [
    { title: t('shell.help.g.global'), keys: [t('shell.help.k.esc'), t('shell.help.k.help')] },
    { title: t('shell.help.g.browse'), keys: [t('shell.help.k.arrows'), t('shell.help.k.enter')] },
    { title: t('shell.help.g.ctx'), keys: [t('shell.help.k.ctxkey'), t('shell.help.k.searchEsc')] },
    { title: t('shell.help.g.tools'), keys: [t('shell.help.k.shiftEnter')] },
    { title: t('shell.help.g.tabs'), keys: [t('shell.help.k.tabs')] },
    { title: t('shell.help.g.dead'), keys: [t('shell.help.k.dead')] },
    { title: t('shell.help.g.chat'), keys: [t('shell.help.k.chatEnter')] },
  ];
  return (
    <div className="modal-bg" role="dialog" aria-modal="true" aria-label={t('shell.help.title')} onClick={onClose}>
      <div className="modal help-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>{t('shell.help.title')}</div>
          <button className="ghost icon" onClick={onClose} title={t('shell.help.close')} aria-label={t('shell.help.close')}>
            <X size={16} />
          </button>
        </div>
        <div className="help-hint small muted">{t('shell.help.hint')}</div>
        {groups.map((g) => (
          <div className="help-group" key={g.title}>
            <div className="help-group-title">{g.title}</div>
            <ul className="help-keys">
              {g.keys.map((k) => <li key={k}>{k}</li>)}
            </ul>
          </div>
        ))}
      </div>
    </div>
  );
}
