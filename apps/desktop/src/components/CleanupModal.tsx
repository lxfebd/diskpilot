import { useEffect, useMemo, useRef, useState } from 'react';
import { X, Trash2, Loader2, AlertTriangle } from 'lucide-react';
import { api } from '../api';
import type { ExecuteScopeOpts } from '../api';
import { formatBytes } from '../format';
import { t, useT } from '../i18n';
import type { Node, Scaffold, Scope, CondaEnv } from '../types';
import { ProgressButton } from './ProgressButton';

interface ScopeSize {
  scope_id: string;
  /** Bytes that match scope glob AND are older than the requested retention. */
  bytes: number;
  file_count: number;
  /** Bytes inside the scope regardless of retention — UI uses this so users
   *  can see "12 GB total · 0 GB older than 90d will be cleaned" instead of
   *  the misleading "空" that used to render when retention spared everything. */
  total_bytes: number;
  total_files: number;
}

interface DryRunPreview {
  scopeIds: string[];
  totalBytes: number;
  totalFiles: number;
  /** First N paths that would be deleted. Capped to keep the dialog usable. */
  samplePaths: string[];
  /** True when more paths exist than samplePaths shows. */
  truncated: boolean;
}

const DRY_RUN_SAMPLE_CAP = 80;

const SCOPE_DAYS_STORAGE_KEY = 'diskpilot.scopeDays';

function readScopeDaysAll(): Record<string, Record<string, number>> {
  try {
    const raw = localStorage.getItem(SCOPE_DAYS_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    return {};
  }
}

function useScopeDays(scaffoldId: string, defaults: Record<string, number>) {
  const defaultsKey = JSON.stringify(defaults);
  const [days, setDays] = useState<Record<string, number>>(() => {
    const persisted = readScopeDaysAll()[scaffoldId] ?? {};
    return { ...defaults, ...persisted };
  });

  useEffect(() => {
    const persisted = readScopeDaysAll()[scaffoldId] ?? {};
    setDays({ ...defaults, ...persisted });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scaffoldId, defaultsKey]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      const all = readScopeDaysAll();
      const overrides: Record<string, number> = {};
      for (const [k, v] of Object.entries(days)) {
        if (defaults[k] !== v) overrides[k] = v;
      }
      if (Object.keys(overrides).length === 0) {
        delete all[scaffoldId];
      } else {
        all[scaffoldId] = overrides;
      }
      try { localStorage.setItem(SCOPE_DAYS_STORAGE_KEY, JSON.stringify(all)); } catch { /* quota / private mode */ }
    }, 300);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scaffoldId, JSON.stringify(days), defaultsKey]);

  return [days, setDays] as const;
}

function detectVariants(matches: Node[]): Set<string> {
  const out = new Set<string>();
  for (const m of matches) {
    const p = m.path.replace(/\\/g, '/').toLowerCase();
    if (p.includes('xwechat_files') || p.includes('tencent/xwechat')) out.add('4.x');
    if (p.includes('wechat files') || p.includes('tencent/wechat')) out.add('3.x');
  }
  return out;
}

function aggregateScopeSizes(rowsList: ScopeSize[][]): ScopeSize[] {
  const merged = new Map<string, ScopeSize>();
  for (const rows of rowsList) {
    for (const r of rows) {
      const prev = merged.get(r.scope_id);
      if (prev) {
        prev.bytes += r.bytes;
        prev.file_count += r.file_count;
        prev.total_bytes += r.total_bytes;
        prev.total_files += r.total_files;
      } else {
        merged.set(r.scope_id, {
          scope_id: r.scope_id,
          bytes: r.bytes,
          file_count: r.file_count,
          total_bytes: r.total_bytes,
          total_files: r.total_files,
        });
      }
    }
  }
  return [...merged.values()];
}

function formatLastActive(ts: number | null): string {
  if (ts === null) return t('cleanup.lastActiveNever');
  const now = Math.floor(Date.now() / 1000);
  const diffSecs = Math.max(0, now - ts);
  const days = Math.floor(diffSecs / 86400);
  if (days < 1) return t('cleanup.lastActiveToday');
  if (days < 30) return t('cleanup.lastActiveDays', { n: days });
  if (days < 365) return t('cleanup.lastActiveMonths', { n: Math.floor(days / 30) });
  return t('cleanup.lastActiveYears', { n: Math.floor(days / 365) });
}

interface Props {
  scaffold: Scaffold;
  matches: Node[];
  onClose: () => void;
  onCleaned: (bytes: number) => void;
}

export function CleanupModal({ scaffold: sc, matches, onClose, onCleaned }: Props) {
  const t = useT();
  const isConda = sc.id === 'conda';

  // ── Days inputs (per-scope, persisted across sessions) ──
  const defaultDays = useMemo<Record<string, number>>(() => {
    const out: Record<string, number> = {};
    for (const scope of sc.scopes ?? []) {
      if (scope.prompt?.kind === 'days') out[scope.id] = scope.prompt.default;
    }
    return out;
  }, [sc.scopes]);
  const [daysByScope, setDaysByScope] = useScopeDays(sc.id, defaultDays);

  // ── WeChat: per-account (wxid_*) filter ──
  const wxids = useMemo<string[]>(() => {
    const out = new Set<string>();
    for (const m of matches) {
      for (const c of m.children ?? []) {
        if (c.is_dir && c.name.startsWith('wxid_')) out.add(c.name);
      }
    }
    return [...out].sort();
  }, [matches]);
  const [selectedWxids, setSelectedWxids] = useState<Set<string>>(() => new Set(wxids));
  useEffect(() => { setSelectedWxids(new Set(wxids)); }, [wxids.join('|')]);
  const wxidFilterArg = wxids.length > 0 && selectedWxids.size < wxids.length
    ? [...selectedWxids]
    : undefined;
  const wxidKey = wxidFilterArg ? wxidFilterArg.slice().sort().join('|') : '';

  // ── Conda: env picker ──
  const [condaEnvs, setCondaEnvs] = useState<CondaEnv[] | null>(null);
  const [condaEnvsLoading, setCondaEnvsLoading] = useState(false);
  const [selectedEnvs, setSelectedEnvs] = useState<Set<string>>(new Set());

  // ── Scope sizes (live preview, refetched when filters change) ──
  const [scopeSizes, setScopeSizes] = useState<ScopeSize[] | null>(null);
  const [scopeLoading, setScopeLoading] = useState(false);

  // ── Which scopes are checked for cleanup ──
  const detectedVariants = useMemo(() => detectVariants(matches), [matches]);
  const visibleScopes = useMemo(
    () => (sc.scopes ?? []).filter((s) => !s.variant || detectedVariants.has(s.variant)),
    [sc.scopes, detectedVariants],
  );
  const [selectedScopes, setSelectedScopes] = useState<Set<string>>(() => new Set());

  // ── Execute state ──
  const [running, setRunning] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [preview, setPreview] = useState<DryRunPreview | null>(null);
  const [armed, setArmed] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  // 二次确认的 5s 自动取消定时器登记到 ref：组件提前卸载（切页/ESC）时清掉，
  // 避免迟到回调在已卸载的组件上 setState。预览对话框里有独立的同款定时器。
  const armTimeoutRef = useRef<number | null>(null);
  useEffect(() => () => {
    if (armTimeoutRef.current !== null) window.clearTimeout(armTimeoutRef.current);
  }, []);

  const matchKey = matches.map((m) => m.path).sort().join('|');
  const daysKey = JSON.stringify(daysByScope);

  // Load conda envs.
  useEffect(() => {
    if (!isConda || matches.length === 0) return;
    let cancelled = false;
    setCondaEnvsLoading(true);
    api
      .listCondaEnvs(matches[0].path)
      .then((envs) => {
        if (cancelled) return;
        setCondaEnvs(envs);
        setSelectedEnvs(new Set(envs.filter((e) => e.default_checked).map((e) => e.name)));
      })
      .catch((e) => { if (!cancelled) setErr(t('cleanup.errCondaEnvs', { msg: String(e) })); })
      .finally(() => { if (!cancelled) setCondaEnvsLoading(false); });
    return () => { cancelled = true; };
  }, [isConda, matchKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // Fetch scope sizes (debounced).
  useEffect(() => {
    if (matches.length === 0 || (sc.scopes ?? []).length === 0) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      if (cancelled) return;
      setScopeLoading(true);
      api
        .scopeSizesBatch(sc.id, matches.map((m) => m.path), daysByScope, wxidFilterArg)
        .then((batch) => {
          if (cancelled) return;
          setScopeSizes(aggregateScopeSizes(batch.map((b) => b.sizes)));
        })
        .catch((e) => { if (!cancelled) setErr(t('cleanup.errScopeSizes', { msg: String(e) })); })
        .finally(() => { if (!cancelled) setScopeLoading(false); });
    }, 300);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [isConda, matchKey, sc.id, (sc.scopes ?? []).length, daysKey, wxidKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // Bytes / files for a scope from current sizes. `bytes` and `filesForScope`
  // honor the days filter (i.e. they describe what would actually be cleaned).
  // `totalBytes` / `totalFiles` ignore the days filter — used to show users
  // "you have 12 GB of videos · 0 GB exceed retention" instead of just "空".
  const bytesForScope = (id: string) => scopeSizes?.find((r) => r.scope_id === id)?.bytes ?? 0;
  const filesForScope = (id: string) => scopeSizes?.find((r) => r.scope_id === id)?.file_count ?? 0;
  const totalBytesForScope = (id: string) => scopeSizes?.find((r) => r.scope_id === id)?.total_bytes ?? 0;
  const totalFilesForScope = (id: string) => scopeSizes?.find((r) => r.scope_id === id)?.total_files ?? 0;

  const sortedMediaScopes = useMemo(() => {
    const list = visibleScopes.filter((s) => s.category === 'media');
    return [...list].sort((a, b) => bytesForScope(b.id) - bytesForScope(a.id));
  }, [visibleScopes, scopeSizes]);

  const cacheScopes = visibleScopes.filter((s) => (s.category ?? 'cache') === 'cache');
  const backupScopes = visibleScopes.filter((s) => s.category === 'backup');

  const toggleScope = (id: string) => {
    setSelectedScopes((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleScopeGroup = (group: Scope[]) => {
    const ids = group.map((s) => s.id);
    const allOn = ids.every((id) => selectedScopes.has(id));
    setSelectedScopes((prev) => {
      const next = new Set(prev);
      if (allOn) ids.forEach((id) => next.delete(id));
      else ids.forEach((id) => { if (bytesForScope(id) > 0) next.add(id); });
      return next;
    });
  };

  // ── Total bytes for the cleanup button label ──
  const totalSelected = useMemo(() => {
    if (isConda) {
      let count = 0;
      let bytes = 0;
      if (condaEnvs) {
        for (const e of condaEnvs) {
          if (selectedEnvs.has(e.name)) { count += 1; bytes += e.size_bytes; }
        }
      }
      // conda 包缓存 scope（tarballs / unused-packages）计入合计。
      for (const id of ['tarballs', 'unused-packages']) {
        const b = bytesForScope(id);
        if (b > 0 && selectedScopes.has(id)) { count += 1; bytes += b; }
      }
      return { count, bytes };
    }
    let count = 0;
    let bytes = 0;
    for (const id of selectedScopes) {
      const b = bytesForScope(id);
      if (b > 0) { count += 1; bytes += b; }
    }
    return { count, bytes };
  }, [isConda, selectedEnvs, condaEnvs, selectedScopes, scopeSizes]);

  // ── Execute ──
  const execute = async () => {
    if (running) return;
    if (!armed) {
      setArmed(true);
      setMsg(null);
      if (armTimeoutRef.current !== null) window.clearTimeout(armTimeoutRef.current);
      armTimeoutRef.current = window.setTimeout(() => {
        armTimeoutRef.current = null;
        setArmed(false);
      }, 5000);
      return;
    }
    setArmed(false);
    if (armTimeoutRef.current !== null) {
      window.clearTimeout(armTimeoutRef.current);
      armTimeoutRef.current = null;
    }
    if (preview === null) {
      // Two-step: first click after arming runs dry-run + opens preview
      // dialog. The user must explicitly confirm IN the dialog to actually
      // delete. Without this users were silent-deleting based on a misleading
      // size pill ("空" used to mean "everything was wiped" — actually meant
      // "nothing exceeds retention").
      await runDryRun();
      return;
    }
    await runRealDelete();
  };

  const runDryRun = async () => {
    setPreviewing(true);
    setMsg(null);
    setErr(null);
    try {
      const samplePaths: string[] = [];
      let totalBytes = 0;
      let totalFiles = 0;
      let scopeIds: string[] = [];

      if (isConda) {
        if (matches.length === 0) {
          setErr(t('cleanup.errNoCondaRoot'));
          return;
        }
        scopeIds = [];
        if (selectedEnvs.size > 0) {
          scopeIds.push('envs-stale');
        }
        // 包缓存 scope：tarballs（pkgs/cache）+ unused-packages（pkgs/*，30 天）
        for (const id of ['tarballs', 'unused-packages']) {
          if (selectedScopes.has(id) && bytesForScope(id) > 0) scopeIds.push(id);
        }
        if (scopeIds.length === 0) {
          setErr(t('cleanup.errNoCondaItems'));
          return;
        }
        const tasks: Promise<{ source: string }[]>[] = [];
        for (const scopeId of scopeIds) {
          if (scopeId === 'envs-stale') {
            tasks.push(
              api.executeScope(sc.id, scopeId, matches[0].path, true, {
                envFilter: [...selectedEnvs],
              }).catch((e) => { console.warn(`[diskpilot] dry-run ${sc.id}/${scopeId} failed:`, e); return [] as { source: string }[]; }),
            );
          } else {
            const days = daysByScope[scopeId];
            for (const m of matches) {
              tasks.push(
                api.executeScope(sc.id, scopeId, m.path, true, { olderThanDays: days }).catch((e) => {
                  console.warn(`[diskpilot] dry-run ${sc.id}/${scopeId} on ${m.path} failed:`, e);
                  return [] as { source: string }[];
                }),
              );
            }
          }
        }
        const lists = await Promise.all(tasks);
        for (const list of lists) {
          for (const e of list) {
            totalFiles += 1;
            if (samplePaths.length < DRY_RUN_SAMPLE_CAP) samplePaths.push(e.source);
          }
        }
        totalBytes = totalSelected.bytes;
      } else {
        scopeIds = [...selectedScopes].filter((id) => bytesForScope(id) > 0);
        if (scopeIds.length === 0) {
          setErr(t('cleanup.errNoScopes'));
          return;
        }
        totalBytes = scopeIds.reduce((s, id) => s + bytesForScope(id), 0);
        const tasks: Promise<{ source: string }[]>[] = [];
        for (const scopeId of scopeIds) {
          const days = daysByScope[scopeId];
          for (const m of matches) {
            tasks.push(
              api.executeScope(sc.id, scopeId, m.path, true, { olderThanDays: days, wxidFilter: wxidFilterArg }).catch((e) => {
                console.warn(`[diskpilot] dry-run ${sc.id}/${scopeId} on ${m.path} failed:`, e);
                return [] as { source: string }[];
              }),
            );
          }
        }
        const lists = await Promise.all(tasks);
        for (const list of lists) {
          for (const e of list) {
            totalFiles += 1;
            if (samplePaths.length < DRY_RUN_SAMPLE_CAP) samplePaths.push(e.source);
          }
        }
      }

      if (totalFiles === 0) {
        setErr(t('cleanup.errPreviewEmpty'));
        return;
      }

      setPreview({
        scopeIds,
        totalBytes,
        totalFiles,
        samplePaths,
        truncated: totalFiles > samplePaths.length,
      });
    } catch (e) {
      setErr(t('cleanup.errPreview', { msg: String(e) }));
    } finally {
      setPreviewing(false);
    }
  };

  const cancelPreview = () => {
    setPreview(null);
    setArmed(false);
    if (armTimeoutRef.current !== null) {
      window.clearTimeout(armTimeoutRef.current);
      armTimeoutRef.current = null;
    }
  };

  const runRealDelete = async () => {
    if (preview === null) return;
    setRunning(true);
    setMsg(null);
    setErr(null);
    try {
      let totalEntries = 0;
      // 真实执行的失败必须看得见：每个 executeScope 的 reject 记进 failures，
      // 结束时按「全成功 / 部分失败 / 全失败」三种口径出信息。绝不能因为
      // catch 里只 console.warn 就把「一个都没删掉」报成「已清理 N 字节」。
      const failures: string[] = [];
      const runReal = (scopeId: string, path: string, opts: ExecuteScopeOpts) =>
        api
          .executeScope(sc.id, scopeId, path, false, { ...opts, confirmed: true })
          .then(
            (entries) => { totalEntries += entries.length; },
            (e) => {
              // 诊断串（scope @ 路径: 后端原文），全 ASCII 分隔符，便于直接拼进文案表。
              failures.push(`${scopeId} @ ${path}: ${String(e)}`);
              console.warn(`[diskpilot] executeScope ${sc.id}/${scopeId} on ${path} failed:`, e);
            },
          );

      if (isConda) {
        const envFilterArg = [...selectedEnvs];
        const tasks: Promise<unknown>[] = [];
        for (const scopeId of preview.scopeIds) {
          if (scopeId === 'envs-stale') {
            tasks.push(runReal(scopeId, matches[0].path, { envFilter: envFilterArg }));
          } else {
            const days = daysByScope[scopeId];
            for (const m of matches) {
              tasks.push(runReal(scopeId, m.path, { olderThanDays: days }));
            }
          }
        }
        await Promise.all(tasks);
        const refreshed = await api.listCondaEnvs(matches[0].path).catch(() => [] as CondaEnv[]);
        setCondaEnvs(refreshed);
        setSelectedEnvs(new Set(refreshed.filter((e) => e.default_checked).map((e) => e.name)));
        if (preview.scopeIds.some((s) => s !== 'envs-stale')) {
          const batch = await api.scopeSizesBatch(
            sc.id,
            matches.map((m) => m.path),
            daysByScope,
            undefined,
          );
          setScopeSizes(aggregateScopeSizes(batch.map((b) => b.sizes)));
        }
      } else {
        const tasks: Promise<unknown>[] = [];
        for (const scopeId of preview.scopeIds) {
          const days = daysByScope[scopeId];
          for (const m of matches) {
            tasks.push(runReal(scopeId, m.path, { olderThanDays: days, wxidFilter: wxidFilterArg }));
          }
        }
        await Promise.all(tasks);
        const batch = await api.scopeSizesBatch(
          sc.id,
          matches.map((m) => m.path),
          daysByScope,
          wxidFilterArg,
        );
        setScopeSizes(aggregateScopeSizes(batch.map((b) => b.sizes)));
        setSelectedScopes(new Set());
      }

      // 只有真清掉东西才累加「已回收」计数；字节数一律标注为预估（UndoEntry 不带
      // 每项实际字节，实测口径留待后端补字段）。
      const allFailed = failures.length > 0 && totalEntries === 0;
      if (!allFailed) onCleaned(preview.totalBytes);
      if (allFailed) {
        setErr(
          t('cleanup.allFailed', { n: failures.length, msg: failures[failures.length - 1] }),
        );
        setPreview(null);
      } else {
        setMsg(
          t('cleanup.done', { n: totalEntries, size: formatBytes(preview.totalBytes) })
            + (failures.length > 0
              ? t('cleanup.partialFail', { n: failures.length, msg: failures[failures.length - 1] })
              : ''),
        );
        setPreview(null);
      }
    } catch (e) {
      setErr(t('cleanup.errCleanup', { msg: String(e) }));
      throw e;
    } finally {
      setRunning(false);
    }
  };

  const renderScopeRow = (scope: Scope) => {
    const bytes = bytesForScope(scope.id);
    const fileCount = filesForScope(scope.id);
    const totalBytes = totalBytesForScope(scope.id);
    const totalFiles = totalFilesForScope(scope.id);
    const eligibleEmpty = scopeSizes !== null && bytes === 0;
    const trulyEmpty = scopeSizes !== null && totalBytes === 0;
    const allWithinRetention = eligibleEmpty && !trulyEmpty;
    const checked = selectedScopes.has(scope.id) && !eligibleEmpty;
    const days = daysByScope[scope.id] ?? (scope.prompt?.kind === 'days' ? scope.prompt.default : undefined);
    const meta = (() => {
      if (scopeSizes === null) return t('cleanup.scanning');
      if (trulyEmpty) return t('cleanup.empty');
      if (allWithinRetention) {
        return t('cleanup.withinRetention', {
          size: formatBytes(totalBytes),
          files: totalFiles.toLocaleString(),
        });
      }
      // Mixed: some files would be cleaned, some kept.
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
        key={scope.id}
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
            disabled={eligibleEmpty || running}
            onChange={() => toggleScope(scope.id)}
          />
          <div className="cleanup-row-text">
            <span className="cleanup-row-label">{scope.label}</span>
            <span className="cleanup-row-meta">{meta}</span>
          </div>
        </label>
        {scope.prompt?.kind === 'days' && (
          <div className="cleanup-row-days">
            <span>{t('cleanup.keepRecent')}</span>
            <input
              type="number"
              min={0}
              value={days ?? 0}
              onChange={(e) => {
                const v = Number(e.target.value);
                setDaysByScope((prev) => ({ ...prev, [scope.id]: Number.isFinite(v) && v >= 0 ? v : 0 }));
              }}
              disabled={running}
            />
            <span>{t('cleanup.daysUnit')}</span>
          </div>
        )}
      </li>
    );
  };

  const renderScopeGroup = (label: string, group: Scope[]) => {
    if (group.length === 0) return null;
    const allOn = group.every((s) => selectedScopes.has(s.id));
    const someOn = !allOn && group.some((s) => selectedScopes.has(s.id));
    return (
      <section className="cleanup-section">
        <div className="cleanup-section-head">
          <span>{label}</span>
          <button
            type="button"
            className="ghost cleanup-toggle-all"
            onClick={() => toggleScopeGroup(group)}
            disabled={running}
          >
            {allOn ? t('cleanup.selectNone') : someOn ? t('cleanup.selectAll') : t('cleanup.selectAll')}
          </button>
        </div>
        <ul className="cleanup-rows">
          {group.map((s) => renderScopeRow(s))}
        </ul>
      </section>
    );
  };

  const userEnvs = (condaEnvs ?? []).filter((e) => !e.is_base);
  const baseEnv = (condaEnvs ?? []).find((e) => e.is_base);

  const canExecute = !running && !previewing && !preview && totalSelected.count > 0;

  // ── Coverage breakdown: helps users understand "13 GB total but only 4 GB
  // in scopes" — the rest is red-line content (chat DBs, favorites, account
  // state) that scaffolds INTENTIONALLY don't touch. Only meaningful for
  // non-conda scaffolds where matches[] is the actual disk root.
  const coverageBreakdown = useMemo(() => {
    if (isConda) return null;
    const folderTotal = matches.reduce((s, m) => s + m.size, 0);
    const inScope = (scopeSizes ?? []).reduce((s, r) => s + r.total_bytes, 0);
    if (folderTotal === 0) return null;
    const outsideScope = Math.max(0, folderTotal - inScope);
    return { folderTotal, inScope, outsideScope };
  }, [isConda, matches, scopeSizes]);

  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal cleanup-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>
            {t('cleanup.title', { name: sc.name })}
            {scopeLoading && <Loader2 size={13} className="spin" style={{ marginLeft: 8, verticalAlign: 'middle' }} />}
          </div>
          <button className="ghost icon" onClick={onClose}><X size={16} /></button>
        </div>

        <div className="cleanup-paths">
          {matches.map((m) => (
            <div key={m.path} className="cleanup-path-row" title={m.path}>
              <span className="cleanup-path">{m.path}</span>
              <span className="muted small">{formatBytes(m.size)}</span>
            </div>
          ))}
        </div>

        {coverageBreakdown && coverageBreakdown.outsideScope > 0 && (
          <div className="cleanup-coverage" title={t('cleanup.coverageTip')}>
            <div className="cleanup-coverage-row">
              <span>{t('cleanup.coverageTotal')}</span>
              <strong>{formatBytes(coverageBreakdown.folderTotal)}</strong>
            </div>
            <div className="cleanup-coverage-row">
              <span>{t('cleanup.coverageScope')}</span>
              <strong>{formatBytes(coverageBreakdown.inScope)}</strong>
            </div>
            <div className="cleanup-coverage-row protected">
              <span>{t('cleanup.coverageProtected')}</span>
              <strong>{formatBytes(coverageBreakdown.outsideScope)}</strong>
            </div>
          </div>
        )}

        {/* WeChat: account (wxid) filter */}
        {wxids.length > 0 && (
          <section className="cleanup-section">
            <div className="cleanup-section-head">
              <span>{t('cleanup.accounts')}</span>
              <span className="muted small">{t('cleanup.accountsHint')}</span>
            </div>
            <div className="cleanup-wxid-grid">
              {wxids.map((w) => {
                const checked = selectedWxids.has(w);
                return (
                  <label key={w} className="cleanup-wxid-chip">
                    <input
                      type="checkbox"
                      checked={checked}
                      disabled={running}
                      onChange={() => {
                        setSelectedWxids((prev) => {
                          const next = new Set(prev);
                          if (next.has(w)) next.delete(w);
                          else next.add(w);
                          return next;
                        });
                      }}
                    />
                    <span className={checked ? '' : 'muted'}>{w}</span>
                  </label>
                );
              })}
            </div>
          </section>
        )}

        {/* Conda: env picker */}
        {isConda && (
          <section className="cleanup-section">
            <div className="cleanup-section-head">
              <span>{t('cleanup.envList')}</span>
              {condaEnvsLoading && <Loader2 size={11} className="spin" />}
            </div>
            {condaEnvs === null && !condaEnvsLoading && (
              <div className="muted small">{t('cleanup.readFailed')}</div>
            )}
            {condaEnvs && (baseEnv || userEnvs.length > 0) && (
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
                  const checked = selectedEnvs.has(e.name);
                  return (
                    <li key={e.name} className={'cleanup-row' + (checked ? ' checked' : '')} title={e.path}>
                      <label className="cleanup-row-main">
                        <input
                          type="checkbox"
                          checked={checked}
                          disabled={running}
                          onChange={() => {
                            setSelectedEnvs((prev) => {
                              const next = new Set(prev);
                              if (next.has(e.name)) next.delete(e.name);
                              else next.add(e.name);
                              return next;
                            });
                          }}
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
            {condaEnvs && userEnvs.length === 0 && (
              <div className="muted small">{t('cleanup.noUserEnvs')}</div>
            )}
          </section>
        )}

        {/* Scope groups (non-conda scaffolds) */}
        {!isConda && (
          <>
            {renderScopeGroup(t('cleanup.groupMedia'), sortedMediaScopes)}
            {renderScopeGroup(t('cleanup.groupCache'), cacheScopes)}
            {renderScopeGroup(t('cleanup.groupBackup'), backupScopes)}
          </>
        )}
        {/* Conda: 包缓存 scope（tarballs / unused-packages）——与 env picker 分开渲染，
            两者共用 totalSelected 合计；天数字段只出现在 unused-packages 上 */}
        {isConda && renderScopeGroup(t('cleanup.groupPkgs'), cacheScopes)}

        <p className="cleanup-disclaimer">
          <AlertTriangle size={12} /> {sc.disclaimer}
        </p>

        {msg && <div className="ok">{msg}</div>}
        {err && <div className="error">{err}</div>}

        <div className="cleanup-footer">
          <div className="cleanup-summary">
            {totalSelected.count > 0
              ? <>{t('cleanup.summaryLead')} <strong>{totalSelected.count}</strong> {t('cleanup.summaryMid')} <strong>{formatBytes(totalSelected.bytes)}</strong> {t('cleanup.summaryTail')}</>
              : <span className="muted">{t('cleanup.pickItems')}</span>}
          </div>
          <div className="cleanup-actions">
            <button className="ghost" onClick={onClose} disabled={running}>{t('cleanup.cancel')}</button>
            <button
              className={'primary cleanup-execute' + (armed ? ' armed' : '')}
              onClick={execute}
              disabled={!canExecute}
              title={armed ? t('cleanup.armTitle') : t('cleanup.firstClickTitle')}
            >
              {previewing
                ? <><Loader2 size={13} className="spin" /> {t('cleanup.previewing')}</>
                : running
                  ? <><Loader2 size={13} className="spin" /> {t('cleanup.cleaning')}</>
                  : armed
                    ? <><Trash2 size={13} /> {t('cleanup.reclickPreview')}</>
                    : <><Trash2 size={13} /> {t('cleanup.previewFiles')}</>}
            </button>
          </div>
        </div>

        {preview && (
          <DryRunPreviewDialog
            preview={preview}
            running={running}
            onConfirm={runRealDelete}
            onCancel={cancelPreview}
            estimatedCount={preview.totalFiles}
            granularity={isConda ? 'directory' : 'file'}
          />
        )}
      </div>
    </div>
  );
}

interface PreviewDialogProps {
  preview: DryRunPreview;
  running: boolean;
  onConfirm: () => Promise<void>;
  onCancel: () => void;
  estimatedCount: number;
  granularity: 'file' | 'directory';
}

function DryRunPreviewDialog({
  preview,
  running,
  onConfirm,
  onCancel,
  estimatedCount,
  granularity,
}: PreviewDialogProps) {
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
