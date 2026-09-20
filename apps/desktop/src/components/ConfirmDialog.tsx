import type { ReactNode } from 'react';
import { AlertTriangle } from 'lucide-react';
import { t } from '../i18n';

// 破坏性命令的两步确认面板（铁律 6：禁 window.confirm，必须界面内二次确认）。
// 轻量实现：遮罩 + 卡片，复用 chat 的 cli-confirm 样式。默认文案走 common 表，
// 调用方可传自定义标题/正文/按钮。确认后由调用方执行（recycle / uninstall）。
type Props = {
  title: string;
  body: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
};

export function ConfirmDialog({ title, body, confirmLabel, cancelLabel, onConfirm, onCancel }: Props) {
  return (
    <div className="confirm-dialog-mask" role="alertdialog" aria-modal="true" aria-label={title}>
      <div className="cli-confirm">
        <div className="cli-confirm-head"><AlertTriangle size={14} /> {title}</div>
        <div className="cli-confirm-body">{body}</div>
        <div className="cli-confirm-actions">
          <div className="grow" />
          <button className="ghost" onClick={onCancel}>{cancelLabel ?? t('common.confirm.cancel')}</button>
          <button className="primary" onClick={onConfirm}>{confirmLabel ?? t('common.confirm.ok')}</button>
        </div>
      </div>
    </div>
  );
}
