// 清理页主体：原 CleanupModal 的 modal 体去掉 modal-bg 外壳与 header，
// 直接渲染到整页的右侧主区。状态全部从 useCleanupStore 读，不再有本地 useState。
//
// 三段执行语义（预览 → 预备 → 真删）由 store.execute 驱动，本组件只画按钮；
// DryRunPreviewDialog 在 preview !== null 时挂在 body 末尾（zIndex:60 盖住行动条）。
import { Loader2, Trash2, AlertTriangle, History } from 'lucide-react';
import { useCleanupStore, scopeBytes, computeSessionTotal, computeSessionCoverage, computeAllCleanableBytes } from '../../useCleanupStore';
import { formatBytes } from '../../format';
import { applySelectionMode, cleanRatio, cleanedHitStats, formatLastActive, sortScopesByBytes } from '../../cleanup/model';
import { SelectionModePicker, useSelectionOptions } from './SelectionMode';
import { SelectionActionBar } from './SelectionActionBar';
import { DryRunPreviewDialog } from './DryRunPreviewDialog';
import { useT } from '../../i18n';
import type { Scope } from '../../types';

function ScopeRow({ scope }: { scope: Scope }) {
  const t = useT();
  const s = useCleanupStore((st) => st.session)!;
  const toggleScope = useCleanupStore((st) => st.toggleScope);
  const setDays = useCleanupStore((st) => st.setDays);
  const row = s.scopeSizes?.find((r) => r.scope_id === scope.id) ?? null;
  const bytes = row?.bytes ?? 0;
  const fileCount = row?.file_count ?? 0;
  const totalBytes = row?.total_bytes ?? 0;
  const totalFiles = row?.total_files ?? 0;
  const eligibleEmpty = s.scopeSizes !== null && bytes === 0;
  const trulyEmpty = s.scopeSizes !== null && totalBytes === 0;
  const allWithinRetention = eligibleEmpty && !trulyEmpty;
  const checked = s.selectedScopes.has(scope.id) && !eligibleEmpty;
  const days = s.daysByScope[scope.id] ?? (scope.prompt?.kind === 'days' ? scope.prompt.default : undefined);
  // R156：这个 scope 涉及的目录里，最近一次被清理的时刻（取最新）——只按
  // 目录前缀匹配（glob 展开只有后端能做，前端不猜）；台账没读到 → null。
  // R157：同时报覆盖度——多命中目录时只清过其中几个要说清，否则「上次清理
  // 3 天前」会让人以为整个 scope 都清过了。
  const hitStats = cleanedHitStats(s.lastCleanByScope, s.matches.map((m) => m.path));
  const lastCleanTs = hitStats.latestTs;
  // R174：可清理占比（totalBytes 非正 → null，不画条）。
  const ratio = cleanRatio(bytes, totalBytes);
  const meta = (() => {
    if (s.scopeSizes === null) return t('cleanup.scanning');
    if (trulyEmpty) return t('cleanup.empty');
    if (allWithinRetention) {
      return t('cleanup.withinRetention', {
        size: formatBytes(totalBytes),
        files: totalFiles.toLocaleString(),
      });
    }
    const kept = totalBytes - bytes;
    return (
      t('cleanup.metaTotal', {
        size: formatBytes(totalBytes),
        files: totalFiles ? t('cleanup.metaFiles', { files: totalFiles.toLocaleString() }) : '',
      }) +
      t('cleanup.metaToClean', {
        size: formatBytes(bytes),
        count: fileCount ? t('cleanup.metaCount', { count: fileCount.toLocaleString() }) : '',
      }) +
      (kept > 0 ? t('cleanup.metaKept', { size: formatBytes(kept) }) : '')
    );
  })();
  return (
    <li
      className={
        'cleanup-row' +
        (eligibleEmpty ? ' empty' : '') +
        (allWithinRetention ? ' within-retention' : '') +
        (checked ? ' checked' : '')
      }
    >
      <label className="cleanup-row-main">
        <input
          type="checkbox"
          checked={checked}
          disabled={eligibleEmpty || s.running}
          onChange={() => toggleScope(scope.id)}
        />
        <div className="cleanup-row-text">
          <span className="cleanup-row-label" title={scope.label}>{scope.label}</span>
          {/* R174：可清理占比条——三个数字要读一遍才知道「能清掉几成」，
              细条让比例一眼可见（总大小作分母，可清部分填色）。 */}
          {ratio !== null && ratio > 0 && (
            <span className="cleanup-ratio" title={t('cleanup.ratioTip', { pct: Math.round(ratio * 100) })}>
              <span className="cleanup-ratio-fill" aria-hidden style={{ width: `${(ratio * 100).toFixed(1)}%` }} />
            </span>
          )}
          <span className="cleanup-row-meta">{meta}</span>
          {/* R156：该目录上次清理时刻——勾选前就知道「刚清过 / 很久没清」。
              台账没读到（null）或这个目录没清过时不显示，不编造。 */}
          {lastCleanTs !== null && (
            <span className="cleanup-row-last muted small">
              {t('cleanup.lastCleaned', { age: formatLastActive(lastCleanTs) })}
              {hitStats.total > 1 && hitStats.cleaned < hitStats.total && (
                <> · {t('cleanup.lastCleanedPartial', { n: hitStats.cleaned, total: hitStats.total })}</>
              )}
            </span>
          )}
        </div>
      </label>
      {scope.prompt?.kind === 'days' && (
        <div className="cleanup-row-days">
          <span>{t('cleanup.keepRecent')}</span>
          <input
            type="number"
            aria-label={t('cleanup.keepRecentAria')}
            min={0}
            value={days ?? 0}
            onChange={(e) => setDays(scope.id, Number(e.target.value))}
            disabled={s.running}
          />
          <span>{t('cleanup.daysUnit')}</span>
        </div>
      )}
    </li>
  );
}

function ScopeGroup({ label, group }: { label: string; group: Scope[] }) {
  const t = useT();
  const s = useCleanupStore((st) => st.session)!;
  const selectScopes = useCleanupStore((st) => st.selectScopes);
  const clearScopes = useCleanupStore((st) => st.clearScopes);
  const opts = useSelectionOptions();
  if (group.length === 0) return null;
  // R177：分类组统一按可清大小降序（与 media 组口径对齐，不再按目录顺序乱跳）。
  const ordered = sortScopesByBytes(group, (sc) => scopeBytes(s, sc.id));
  const ids = ordered.map((g) => g.id);
  const allOn = ids.every((id) => s.selectedScopes.has(id));
  // 分组语义：全选=本组替换（只挑 bytes>0）；全不选=摘本组。用 applySelectionMode
  // 算出要勾的集合，再与外层 selectedScopes 合并：all → 并入，none → 摘掉。
  const onMode = (mode: 'all' | 'safe' | 'none') => {
    if (mode === 'none') {
      clearScopes(ids);
      return;
    }
    const eligible = group.filter((sc) => scopeBytes(s, sc.id) > 0);
    const keys = applySelectionMode(mode, eligible, () => true, (sc) => sc.id);
    selectScopes([...keys]);
  };
  return (
    <section className="cleanup-section">
      <div className="cleanup-section-head">
        <span>{label}</span>
        <SelectionModePicker
          options={allOn ? [{ mode: 'none', label: t('cleanup.selectNone') }] : opts.scopeGroup}
          onMode={onMode}
          disabled={s.running}
          btnClass="cleanup-toggle-all"
        />
      </div>
      <ul className="cleanup-rows">
        {ordered.map((sc) => <ScopeRow key={sc.id} scope={sc} />)}
      </ul>
    </section>
  );
}

export function CleanupBody({ onOpenHistory }: { onOpenHistory?: () => void }) {
  const t = useT();
  const s = useCleanupStore((st) => st.session);
  const execute = useCleanupStore((st) => st.execute);
  const runRealDelete = useCleanupStore((st) => st.runRealDelete);
  const cancelPreview = useCleanupStore((st) => st.cancelPreview);
  const cancelCleanup = useCleanupStore((st) => st.cancelCleanup);
  if (!s) return null;

  const sc = s.scaffold;
  const total = computeSessionTotal(s);
  const coverage = computeSessionCoverage(s);
  // R175：全盘可清总量 + 本次选择覆盖比例——用户知道自己选了多少成。
  const allCleanable = computeAllCleanableBytes(s);
  const pickRatio = cleanRatio(total.bytes, allCleanable);
  const canExecute = !s.running && !s.previewing && !s.preview && total.count > 0;
  const userEnvs = (s.condaEnvs ?? []).filter((e) => !e.is_base);
  const baseEnv = (s.condaEnvs ?? []).find((e) => e.is_base);

  // R177：三组不再各自内联排序——统一交给 ScopeGroup 按可清大小降序渲染，
  // 避免 media 排过、cache/backup 漏排导致顺序口径不一致。
  // R206：privacy 组（浏览器历史 / 表单 / Recent / 缩略图 / INetCache）此前
  // 落不进任何组导致勾选行不渲染——补第四组，让「隐私清理」页可勾可清。
  const mediaScopes = s.visibleScopes.filter((x) => x.category === 'media');
  const cacheScopes = s.visibleScopes.filter((x) => (x.category ?? 'cache') === 'cache');
  const backupScopes = s.visibleScopes.filter((x) => x.category === 'backup');
  const privacyScopes = s.visibleScopes.filter((x) => x.category === 'privacy');

  const summary =
    total.count > 0
      ? (
        <>
          {t('cleanup.summaryLead')} <strong>{total.count}</strong> {t('cleanup.summaryMid')} <strong>{formatBytes(total.bytes)}</strong> {t('cleanup.summaryTail')}
          {pickRatio !== null && allCleanable > 0 && (
            <span className="muted small">{t('cleanup.summaryShare', { pct: Math.round(pickRatio * 100) })}</span>
          )}
          {s.scopeLoading && <Loader2 size={11} className="spin" style={{ marginLeft: 6 }} />}
        </>
      )
      : <span className="muted">{t('cleanup.pickItems')}</span>;

  return (
    <div className="cp-scroll">
      {s.matches.length === 0 ? (
        <div className="cp-empty">{t('cleanup.noTargets')}</div>
      ) : (
        <>
          <div className="cleanup-paths">
            {s.matches.map((m) => (
              <div key={m.path} className="cleanup-path-row" title={m.path}>
                <span className="cleanup-path">{m.path}</span>
                <span className="muted small">{formatBytes(m.size)}</span>
              </div>
            ))}
          </div>

          {coverage && coverage.outsideScope > 0 && (
            <div className="cleanup-coverage" title={t('cleanup.coverageTip')}>
              <div className="cleanup-coverage-row">
                <span>{t('cleanup.coverageTotal')}</span>
                <strong>{formatBytes(coverage.folderTotal)}</strong>
              </div>
              <div className="cleanup-coverage-row">
                <span>{t('cleanup.coverageScope')}</span>
                <strong>{formatBytes(coverage.inScope)}</strong>
              </div>
              <div className="cleanup-coverage-row protected">
                <span>{t('cleanup.coverageProtected')}</span>
                <strong>{formatBytes(coverage.outsideScope)}</strong>
              </div>
            </div>
          )}

          {s.wxids.length > 0 && (
            <section className="cleanup-section">
              <div className="cleanup-section-head">
                <span>{t('cleanup.accounts')}</span>
                <span className="muted small">{t('cleanup.accountsHint')}</span>
              </div>
              <div className="cleanup-wxid-grid">
                {s.wxids.map((w) => {
                  const checked = s.selectedWxids.has(w);
                  return (
                    <label key={w} className="cleanup-wxid-chip">
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={s.running}
                        onChange={() => useCleanupStore.getState().toggleWxid(w)}
                      />
                      <span className={checked ? '' : 'muted'}>{w}</span>
                    </label>
                  );
                })}
              </div>
            </section>
          )}

          {s.isConda && (
            <section className="cleanup-section">
              <div className="cleanup-section-head">
                <span>{t('cleanup.envList')}</span>
                {s.condaLoading && <Loader2 size={11} className="spin" />}
              </div>
              {s.condaEnvs === null && !s.condaLoading && (
                <div className="muted small">{t('cleanup.readFailed')}</div>
              )}
              {s.condaEnvs && (baseEnv || userEnvs.length > 0) && (
                <ul className="cleanup-rows">
                  {baseEnv && (
                    <li className="cleanup-row empty" title={t('cleanup.baseTip')}>
                      <label className="cleanup-row-main">
                        <input type="checkbox" disabled checked={false} />
                        <div className="cleanup-row-text">
                          <span className="cleanup-row-label muted">{t('cleanup.baseUncleanable')}</span>
                          <span className="cleanup-row-meta">{formatBytes(baseEnv.size_bytes)} · {formatLastActive(baseEnv.last_active_ts)}</span>
                        </div>
                      </label>
                    </li>
                  )}
                  {userEnvs.map((e) => {
                    const checked = s.selectedEnvs.has(e.name);
                    return (
                      <li key={e.name} className={'cleanup-row' + (checked ? ' checked' : '')} title={e.path}>
                        <label className="cleanup-row-main">
                          <input
                            type="checkbox"
                            checked={checked}
                            disabled={s.running}
                            onChange={() => useCleanupStore.getState().toggleEnv(e.name)}
                          />
                          <div className="cleanup-row-text">
                            <span className="cleanup-row-label">{e.name}</span>
                            <span className="cleanup-row-meta">{formatBytes(e.size_bytes)} · {formatLastActive(e.last_active_ts)}{e.default_checked ? t('cleanup.stale90') : ''}</span>
                          </div>
                        </label>
                      </li>
                    );
                  })}
                </ul>
              )}
              {s.condaEnvs && userEnvs.length === 0 && (
                <div className="muted small">{t('cleanup.noUserEnvs')}</div>
              )}
            </section>
          )}

          {!s.isConda && (
            <>
              <ScopeGroup label={t('cleanup.groupMedia')} group={mediaScopes} />
              <ScopeGroup label={t('cleanup.groupCache')} group={cacheScopes} />
              <ScopeGroup label={t('cleanup.groupBackup')} group={backupScopes} />
              <ScopeGroup label={t('cleanup.groupPrivacy')} group={privacyScopes} />
            </>
          )}
          {s.isConda && <ScopeGroup label={t('cleanup.groupPkgs')} group={cacheScopes} />}

          <p className="cleanup-disclaimer">
            <AlertTriangle size={12} /> {sc.disclaimer}
          </p>

          {s.msg && (
            <div className="ok cleanup-msg-with-link">
              <span>{s.msg}</span>
              {onOpenHistory && (
                <button
                  type="button"
                  className="ghost cleanup-goto-history"
                  onClick={onOpenHistory}
                  title={t('cleanup.gotoHistory')}
                >
                  <History size={12} /> {t('cleanup.gotoHistory')}
                </button>
              )}
            </div>
          )}
          {s.err && <div className="error" role="alert">{s.err}</div>}
        </>
      )}

      <SelectionActionBar summary={summary}>
        <button
          className={'primary cleanup-execute' + (s.armed ? ' armed' : '')}
          onClick={execute}
          disabled={!canExecute}
          title={s.armed ? t('cleanup.armTitle') : t('cleanup.firstClickTitle')}
        >
          {s.previewing
            ? <><Loader2 size={13} className="spin" /> {t('cleanup.previewing')}</>
            : s.running
              ? <><Loader2 size={13} className="spin" /> {t('cleanup.cleaning')}</>
              : s.armed
                ? <><Trash2 size={13} /> {t('cleanup.reclickPreview')}</>
                : <><Trash2 size={13} /> {t('cleanup.previewFiles')}</>}
        </button>
      </SelectionActionBar>

      {s.preview && (
        <DryRunPreviewDialog
          preview={s.preview}
          running={s.running}
          onConfirm={runRealDelete}
          onCancel={cancelPreview}
          estimatedCount={s.preview.totalFiles}
          granularity={s.isConda ? 'directory' : 'file'}
          progress={s.progress}
          cancelRequested={s.cancelRequested}
          onCancelCleanup={cancelCleanup}
        />
      )}
    </div>
  );
}
