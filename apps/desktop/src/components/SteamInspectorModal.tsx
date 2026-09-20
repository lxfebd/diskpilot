import { X } from 'lucide-react';
import { SteamInspector } from './SteamInspector';
import { ErrorBoundary } from './ErrorBoundary';
import { useT } from '../i18n';

/// Modal wrapper for the Steam Inspector. Click backdrop to close.
/// The Inspector owns its keyboard handling (including closing the detail
/// rail on Esc); a second window-level Esc here would fire on the same key
/// press and close the whole modal whenever the detail rail was open — so
/// closing this modal on Esc is delegated to a callback the Inspector only
/// invokes when it has nothing else to dismiss first.
export function SteamInspectorModal({
  onClose,
  onRequestClose,
}: {
  onClose: () => void;
  /** Inspector 在 Esc 无其它可关层级（详情栏/子模态/搜索框）时调用。 */
  onRequestClose?: () => void;
}) {
  const t = useT();
  return (
    <div
      className="steam-modal-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="steam-modal-dialog" role="dialog" aria-modal="true" aria-label={t('steam.modalAriaLabel')}>
        <div className="steam-modal-head">
          <div className="steam-modal-title">🎮 {t('steam.modalTitle')}</div>
          <div className="steam-modal-subtitle">{t('steam.modalSubtitle')}</div>
          <button className="steam-modal-close" onClick={onRequestClose ?? onClose} title={t('steam.close')} aria-label={t('steam.close')}>
            <X size={16} />
          </button>
        </div>
        <div className="steam-modal-body">
          <ErrorBoundary fallbackLabel={t('steam.renderFailed')}>
            <SteamInspector
              onDismissBack={() => (onRequestClose ?? onClose)()}
            />
          </ErrorBoundary>
        </div>
      </div>
    </div>
  );
}
