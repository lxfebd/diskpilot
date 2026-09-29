// dry-run 预览窗：第一步「清理」点完弹出，列出真删会动到的路径（前 80 条），
// 用户确认后才真删。这是 AGENTS 铁律①「先出清单再确认」的具象落地——
// 不许被任何「一键清理」跳过。
//
// ⚠️ runDelete 内部不得 setArmed(false)：armed → false 会让本组件重渲染
// 进未预备分支，把正在跑的 ProgressButton 卸载掉、丢失进度态。只清自动
// 回退定时器即可（慢清理 >5s 时不会被误退）。
import { useEffect, useRef, useState } from 'react';
import { X, Trash2, Loader2, AlertTriangle } from 'lucide-react';
import { ProgressButton } from '../ProgressButton';
import { formatBytes } from '../../format';
import { useT } from '../../i18n';
import type { DryRunPreview } from '../../cleanup/model';
import type { RecycleGranularity } from '../../types';

interface Props {
  preview: DryRunPreview;
  running: boolean;
  onConfirm: () => Promise<void>;
  onCancel: () => void;
  estimatedCount: number;
  granularity: RecycleGranularity;
}

export function DryRunPreviewDialog({
  preview,
  running,
  onConfirm,
  onCancel,
  estimatedCount,
  granularity,
}: Props) {
  const t = useT();
  const [armed, setArmed] = useState(false);
  const armTimeoutRef = useRef<number | null>(null);
  useEffect(() => () => {
    if (armTimeoutRef.current !== null) window.clearTimeout(armTimeoutRef.current);
  }, []);
  const armClick = () => {
    if (running || armed) return;
    setArmed(true);
    if (armTimeoutRef.current !== null) window.clearTimeout(armTimeoutRef.current);
    armTimeoutRef.current = window.setTimeout(() => {
      armTimeoutRef.current = null;
      setArmed(false);
    }, 5000);
  };
  // Hand onConfirm directly to ProgressButton. Do NOT setArmed(false) here:
  // armed → false would re-render this dialog into the unarmed branch,
  // unmounting ProgressButton mid-flight and losing its progress state.
  // Cancel the auto-disarm timer though, so a slow clean (>5s) can't trip it.
  const runDelete = async () => {
    if (armTimeoutRef.current !== null) {
      window.clearTimeout(armTimeoutRef.current);
      armTimeoutRef.current = null;
    }
    await onConfirm();
  };
  return (
    <div className="modal-bg" onClick={onCancel} style={{ zIndex: 60 }}>
      <div className="modal cleanup-preview-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>{t('cleanup.previewHead')}</div>
          <button className="ghost icon" onClick={onCancel} disabled={running}><X size={16} /></button>
        </div>

        <div className="cleanup-preview-summary">
          <strong>{preview.totalFiles.toLocaleString()}</strong> {t('cleanup.previewSummaryMid')} <strong>{formatBytes(preview.totalBytes)}</strong>
          <span className="muted small" style={{ marginLeft: 10 }}>
            {t('cleanup.previewRecycle')}
          </span>
        </div>

        <div className="cleanup-preview-list">
          {preview.samplePaths.map((p) => (
            <div key={p} className="cleanup-preview-path" title={p}>{p}</div>
          ))}
          {preview.truncated && (
            <div className="cleanup-preview-more muted small">
              {t('cleanup.previewMore', { n: (preview.totalFiles - preview.samplePaths.length).toLocaleString() })}
            </div>
          )}
        </div>

        <p className="cleanup-disclaimer">
          <AlertTriangle size={12} /> {t('cleanup.previewDisclaimer')}
        </p>

        <div className="cleanup-footer">
          <div className="cleanup-summary muted small">
            {armed ? t('cleanup.armAgainHint') : t('cleanup.twoStepHint')}
          </div>
          <div className="cleanup-actions">
            <button className="ghost" onClick={onCancel} disabled={running}>{t('cleanup.back')}</button>
            {armed ? (
              <ProgressButton
                className="primary cleanup-execute armed"
                estimatedCount={estimatedCount}
                granularity={granularity}
                mode="recycle"
                onAction={runDelete}
                idleContent={<><Trash2 size={13} /> {t('cleanup.reclickDelete')}</>}
              />
            ) : (
              <button
                type="button"
                className="primary cleanup-execute"
                onClick={armClick}
                disabled={running}
              >
                {running
                  ? <><Loader2 size={13} className="spin" /> {t('cleanup.cleaning')}</>
                  : <><Trash2 size={13} /> {t('cleanup.confirmDelete')}</>}
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
