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
import { type HwDiskHealth } from '../../api';
import { ensureHwLoaded, getHwCache } from '../../hwCache';
import { driveAliasesOf, resolveLetter, riskyDrives } from '../../diskHealth';
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
  // 要清的盘若健康可疑，确认前必须显式承认「我有备份」——与 AI 提案窗同一口径。
  // 健康数据只在预览窗打开时读一次（共享缓存有就直接用）；读不到不编造、也不拦。
  const [health, setHealth] = useState<HwDiskHealth | null>(null);
  const [ackRisky, setAckRisky] = useState(false);
  useEffect(() => {
    const cached = getHwCache().health;
    if (cached) {
      setHealth(cached);
      return;
    }
    let cancelled = false;
    void ensureHwLoaded()
      .then((c) => {
        if (!cancelled) setHealth(c.health);
      })
      .catch(() => {
        if (!cancelled) setHealth(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);
  // 映射盘（subst D: → J:\D）要先解析成真正承载它的盘，否则「D 没映射到物理盘」
  // 会放过一次对坏盘的批量清理。
  const aliases = driveAliasesOf(health);
  const risky = riskyDrives(health?.disks ?? []).filter((d) =>
    d.letters.some((l) =>
      preview.driveCounts.some((c) => resolveLetter(c.letter, aliases) === l),
    ),
  );
  const riskyBlocked = risky.length > 0 && !ackRisky;
  const armTimeoutRef = useRef<number | null>(null);
  useEffect(() => () => {
    if (armTimeoutRef.current !== null) window.clearTimeout(armTimeoutRef.current);
  }, []);
  const armClick = () => {
    if (running || armed || riskyBlocked) return;
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
    // 健康数据是异步到的：可能先预备、后才知道这块盘在坏。补一道闸。
    if (riskyBlocked) return;
    if (armTimeoutRef.current !== null) {
      window.clearTimeout(armTimeoutRef.current);
      armTimeoutRef.current = null;
    }
    await onConfirm();
  };
  // R140：键盘流——桌面应用的惯性预期，且比鼠标更防误触。
  //   Esc  → 直接关窗（任何时候都安全，永不会删东西）；
  //   Enter → 未预备则「进预备态」（与点按钮等价，不直接真删）；
  //           已预备则「真删」（等价于 5 秒内再点一次，仍是第二步确认）。
  // 用 ref 存最新闭包，避免 effect 依赖抖动导致监听反复重挂。
  const keyRef = useRef({ armed, running, riskyBlocked, armClick, runDelete, onCancel });
  keyRef.current = { armed, running, riskyBlocked, armClick, runDelete, onCancel };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
const k = keyRef.current;
      if (k.running) return;
      if (e.key === 'Escape') {
        e.preventDefault();
        k.onCancel();
      } else if (e.key === 'Enter') {
        e.preventDefault();
        if (k.riskyBlocked) return; // 未勾备份确认：Enter 也不放行
        if (k.armed) void k.runDelete();
        else k.armClick();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
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

        {preview.driveCounts.length > 0 && (
          <div className="cleanup-preview-drives muted small">
            {t('cleanup.previewDrivesLead')}
            {preview.driveCounts.map((d, i) => {
              const riskyDrive = risky.some((r) =>
                r.letters.includes(resolveLetter(d.letter, aliases)),
              );
              return (
                <span
                  key={d.letter}
                  className={riskyDrive ? 'cleanup-preview-drive risky' : 'cleanup-preview-drive'}
                >
                  {i > 0 ? ' · ' : ''}
                  {t('cleanup.previewDriveItem', {
                    letter: d.letter,
                    files: d.files.toLocaleString(),
                  })}
                </span>
              );
            })}
          </div>
        )}

        {risky.length > 0 && (
          <div className="cleanup-risky" role="alert">
            <div className="cleanup-risky-line">
              <AlertTriangle size={13} />
              <span>
                {risky.map((d) =>
                  t(
                    d.grade === 'bad' ? 'cleanup.riskyDriveBad' : 'cleanup.riskyDriveCaution',
                    { letters: d.letters.join('、'), model: d.friendly },
                  ),
                ).join(' ')}
              </span>
            </div>
            <label className="cleanup-risky-ack">
              <input
                type="checkbox"
                checked={ackRisky}
                onChange={(e) => setAckRisky(e.target.checked)}
              />
              {t('cleanup.riskyAck')}
            </label>
          </div>
        )}

        <div className="cleanup-preview-size-note muted small">
          {t('cleanup.previewSizeNote')}
        </div>

        <div className="cleanup-preview-list">
          {preview.sampleEntries.map((e) => (
            <div key={e.source} className="cleanup-preview-path" title={e.source}>
              <span className="cleanup-preview-path-name">{e.source}</span>
              {e.bytes_freed != null && (
                <span className="cleanup-preview-path-size">{formatBytes(e.bytes_freed)}</span>
              )}
            </div>
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
            {riskyBlocked
              ? t('cleanup.riskyBlockedHint')
              : armed
                ? t('cleanup.armAgainHint')
                : t('cleanup.twoStepHint')}
            <span className="cleanup-kbd-hint"> · {t('cleanup.kbdHint')}</span>
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
                disabled={running || riskyBlocked}
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
