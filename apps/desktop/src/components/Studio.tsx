import { useEffect, useMemo, useState } from 'react';
import { ChevronRight, ChevronDown, Sparkles, MessageSquare, Trash2, FolderOpen, Copy, ExternalLink, Gamepad2, Plus, X, RotateCcw, Download, ShieldAlert } from 'lucide-react';
import { useStore } from '../store';
import { formatBytes } from '../format';
import { t, useT } from '../i18n';
import { api, type RegistryScaffold } from '../api';
import { isPermEnabled } from '../permissions';
import type { Node, Scaffold } from '../types';
import { ErrorBoundary } from './ErrorBoundary';
import { ContextMenu, type ContextMenuState } from './ContextMenu';
import { ConfirmDialog } from './ConfirmDialog';
import { CleanupModal } from './CleanupModal';
import { SteamInspectorModal } from './SteamInspectorModal';
import { UndoPanel } from './UndoPanel';

const FEATURED_IDS = [
  'wechat-pc',
  'conda',
];

const ICONS: Record<string, string> = {
  'wechat-pc': '💬',
  'conda':     '🐍',
};

function fallbackByNameContains(root: Node | null, sc: Scaffold): Node | null {
  const fragments = (sc.match?.name_contains ?? []).map((s) => s.toLowerCase());
  if (fragments.length === 0 || !root) return null;
  const dfs = (n: Node | null): Node | null => {
    if (!n) return null;
    const lower = n.path.toLowerCase();
    if (fragments.some((f) => lower.includes(f))) return n;
    for (const c of n.children ?? []) {
      const f = dfs(c);
      if (f) return f;
    }
    return null;
  };
  return dfs(root);
}

interface CardData {
  scaffold: Scaffold;
  matches: Node[];
  totalSize: number;
  totalFiles: number;
}

export function Studio() {
  const t = useT();
  const root = useStore((s) => s.root);
  const scaffolds = useStore((s) => s.scaffolds);
  const setScaffolds = useStore((s) => s.setScaffolds);
  const toast = useStore((s) => s.toast);
  const requestStudio = useStore((s) => s.requestStudio);

  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [openTool, setOpenTool] = useState<null | 'steam-inspector'>(null);
  const [installOpen, setInstallOpen] = useState(false);
  const [communityOpen, setCommunityOpen] = useState(false);

  const hidden = (() => {
    try { return localStorage.getItem('diskpilot.hideStudio') === '1'; } catch { return false; }
  })();

  const allCards: CardData[] = useMemo(() => {
    if (hidden) return [];
    // 一次 DFS 按 scaffold_id 分组收集所有命中节点，替代原先"每个 scaffold
    // 各做一次全树遍历"（N 个 scaffold = N 倍全树开销）。命中即不再下钻，
    // 与旧 findAllMatchesByScaffold 语义一致：同一 scaffold 不会自递归重复收集。
    const byId = new Map<string, Node[]>();
    const collect = (n: Node) => {
      if (n.scaffold_id) {
        const arr = byId.get(n.scaffold_id);
        if (arr) arr.push(n);
        else byId.set(n.scaffold_id, [n]);
        return;
      }
      for (const c of n.children ?? []) collect(c);
    };
    if (root) collect(root);

    const items: CardData[] = scaffolds.map((sc) => {
      let matches = byId.get(sc.id) ?? [];
      if (matches.length === 0) {
        const fb = fallbackByNameContains(root, sc);
        if (fb) matches = [fb];
      }
      matches.sort((a, b) => b.size - a.size);
      const totalSize = matches.reduce((s, m) => s + m.size, 0);
      const totalFiles = matches.reduce((s, m) => s + m.file_count, 0);
      return { scaffold: sc, matches, totalSize, totalFiles };
    });
    items.sort((a, b) => {
      const aDet = a.matches.length > 0;
      const bDet = b.matches.length > 0;
      if (aDet && !bDet) return -1;
      if (!aDet && bDet) return 1;
      if (aDet && bDet) return b.totalSize - a.totalSize;
      return a.scaffold.name.localeCompare(b.scaffold.name);
    });
    return items;
  }, [scaffolds, root, hidden]);

  if (hidden) {
    return (
      <div className="studio">
        <div className="studio-head">
          <span>{t('studio.title')}</span>
          <span className="muted small">{t('studio.hidden')}</span>
        </div>
      </div>
    );
  }

  const featured = allCards.filter((c) => FEATURED_IDS.includes(c.scaffold.id));
  const others = allCards.filter((c) => !FEATURED_IDS.includes(c.scaffold.id));

  const toggle = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  return (
    <div className="studio">
      <div className="studio-head">
        <span>{t('studio.title')}</span>
        <span className="muted small">{t('studio.scriptCount', { n: allCards.length })}</span>
        <button
          className="ghost icon"
          title={t('studio.installTitle')}
          onClick={() => setInstallOpen(true)}
        >
          <Plus size={13} />
        </button>
      </div>

      <div className="studio-section-label">{t('studio.featured')}</div>
      <div className="studio-grid">
        <ErrorBoundary fallbackLabel={t('studio.fbSteamCard')}>
          <ToolCard
            icon={<Gamepad2 size={14} />}
            name={t('studio.steamName')}
            blurb={t('studio.steamBlurb')}
            onClick={() => setOpenTool('steam-inspector')}
          />
        </ErrorBoundary>
        {featured.map((c) => (
          <ErrorBoundary key={c.scaffold.id} fallbackLabel={t('studio.cardFailed', { name: c.scaffold.name })}>
            <Card card={c} expanded={expanded.has(c.scaffold.id)} onToggle={() => toggle(c.scaffold.id)} onAsk={() => requestStudio(c.scaffold.id)} />
          </ErrorBoundary>
        ))}
      </div>

      {others.length > 0 && (
        <>
          <div className="studio-section-label">{t('studio.more')}</div>
          <div className="studio-grid">
            {others.map((c) => (
              <ErrorBoundary key={c.scaffold.id} fallbackLabel={t('studio.cardFailed', { name: c.scaffold.name })}>
                <Card card={c} expanded={expanded.has(c.scaffold.id)} onToggle={() => toggle(c.scaffold.id)} onAsk={() => requestStudio(c.scaffold.id)} />
              </ErrorBoundary>
            ))}
          </div>
        </>
      )}

      <ErrorBoundary fallbackLabel={t('studio.fbUndoPanel')}>
        <UndoPanel />
      </ErrorBoundary>

      {openTool === 'steam-inspector' && (
        <SteamInspectorModal
          onClose={() => setOpenTool(null)}
          onRequestClose={() => setOpenTool(null)}
        />
      )}

      {installOpen && (
        <InstallScaffoldModal
          onClose={() => setInstallOpen(false)}
          onInstalled={(id) => {
            setInstallOpen(false);
            void refreshScaffolds(setScaffolds, toast, id);
          }}
        />
      )}

      <div className="studio-community">
        <button className="studio-community-head" onClick={() => setCommunityOpen((v) => !v)}>
          <Download size={13} />
          {t('studio.communityRepo')}
          <span className="muted small">{t('studio.communityHint')}</span>
          {communityOpen ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
        </button>
        {communityOpen && (
          <CommunityScaffolds
            onInstalled={(id) => {
              void refreshScaffolds(setScaffolds, toast, id);
            }}
          />
        )}
      </div>
    </div>
  );
}

// ── 社区脚本仓库（R8）：列表 + 一键安装（Ed25519 签名强制，后端校验）──
function CommunityScaffolds({ onInstalled }: { onInstalled: (id: string) => void }) {
  const t = useT();
  const toast = useStore((s) => s.toast);
  const [items, setItems] = useState<RegistryScaffold[] | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  const [confirmId, setConfirmId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .scaffoldRegistryList()
      .then((r) => {
        if (cancelled) return;
        setItems(r.items);
        setWarning(r.warning);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const install = async (id: string) => {
    if (!isPermEnabled('scaffold.manage')) {
      setError(t('studio.permMissing'));
      setConfirmId(null);
      return;
    }
    setInstalling(id);
    setError(null);
    try {
      const installed = await api.scaffoldRegistryInstall(id, true);
      setConfirmId(null);
      onInstalled(installed);
      toast(t('studio.installed', { id: installed }), 'ok');
    } catch (e) {
      setError(String(e));
    } finally {
      setInstalling(null);
    }
  };

  if (items === null) {
    return <div className="studio-community-list muted small">{t('studio.loadingRegistry')}</div>;
  }

  return (
    <div className="studio-community-list">
      {warning && <div className="muted small">{warning}</div>}
      {error && <div className="studio-community-err">{error}</div>}
      {items.length === 0 && <div className="muted small">{t('studio.registryEmpty')}</div>}
      {items.map((item) => {
        const ready = !!item.url.trim();
        return (
          <div key={item.id} className="studio-community-row">
            <div className="studio-community-info">
              <div className="studio-community-name">
                {item.name}
                {!ready && <span className="studio-community-soon">{t('studio.comingSoon')}</span>}
              </div>
              <div className="studio-community-desc muted small">{item.description}</div>
              <div className="studio-community-meta muted small">
                {t('studio.meta', { version: item.version, author: item.author, risk: item.risk })}
                {item.signature && item.signer && <span className="studio-community-signed">{t('studio.signed')}</span>}
              </div>
            </div>
            {confirmId === item.id ? (
              <div className="studio-community-confirm">
                <span className="muted small">{t('studio.confirmInstallTip')}</span>
                <button className="ghost small" onClick={() => setConfirmId(null)} disabled={installing === item.id}>{t('studio.cancel')}</button>
                <button className="primary small" onClick={() => void install(item.id)} disabled={installing === item.id}>
                  {installing === item.id ? t('studio.installing') : t('studio.confirmInstall')}
                </button>
              </div>
            ) : (
              <button
                className="ghost small"
                disabled={!ready}
                title={ready ? t('studio.installReadyTitle') : t('studio.installNoUrl')}
                onClick={() => {
                  setConfirmId(item.id);
                  setError(null);
                }}
              >
                {ready ? <Download size={12} /> : <ShieldAlert size={12} />} {t('studio.install')}
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}

async function refreshScaffolds(
  setScaffolds: (s: Scaffold[]) => void,
  toast: (t: string, k?: 'ok' | 'err') => void,
  installedId?: string,
) {
  try {
    const list = await api.listScaffolds();
    setScaffolds(list);
    if (installedId) toast(t('studio.installedScript', { id: installedId }), 'ok');
  } catch (e) {
    toast(t('studio.refreshFailed', { msg: String(e) }), 'err');
  }
}

function InstallScaffoldModal({
  onClose,
  onInstalled,
}: {
  onClose: () => void;
  onInstalled: (id: string) => void;
}) {
  const t = useT();
  const [toml, setToml] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const install = async () => {
    if (!toml.trim()) {
      setErr(t('studio.needToml'));
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      const id = await api.installScaffold(toml);
      onInstalled(id);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal scaffold-install-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <span>{t('studio.installHead')}</span>
          <button className="ghost icon" onClick={onClose} title={t('studio.close')} aria-label={t('studio.close')}><X size={14} /></button>
        </div>
        <div className="modal-body">
          <p className="muted small">
            {t('studio.installLead')} <code>*.toml</code> {t('studio.installTail')}
          </p>
          <textarea
            className="scaffold-toml-input"
            placeholder={t('studio.tomlSample')}
            value={toml}
            onChange={(e) => setToml(e.target.value)}
            spellCheck={false}
          />
          {err && <div className="install-err">{err}</div>}
        </div>
        <div className="modal-foot">
          <button className="ghost" onClick={onClose} disabled={busy}>{t('studio.cancel')}</button>
          <button className="primary" onClick={() => void install()} disabled={busy}>
            {busy ? t('studio.installing') : t('studio.install')}
          </button>
        </div>
      </div>
    </div>
  );
}

/// Tool cards live in Studio alongside scaffold cards but behave differently:
/// no inline expansion, no "X 个位置" detection meta — just a card-shaped
/// entry point that opens a dedicated modal. Steam Inspector is the first
/// tool; future "always-on" panels (Epic / GOG inspectors, library reports)
/// can reuse this shape.
function ToolCard({
  icon,
  name,
  blurb,
  onClick,
}: {
  icon: React.ReactNode;
  name: string;
  blurb: string;
  onClick: () => void;
}) {
  return (
    <div className="studio-card-wrap risk-low tool-card-wrap">
      <button className="studio-card tool-card" onClick={onClick} title={blurb}>
        <ExternalLink size={12} className="studio-caret tool-card-arrow" />
        <div className="studio-card-icon tool-card-icon">{icon}</div>
        <div className="studio-card-body">
          <div className="studio-card-name">{name}</div>
          <div className="studio-card-meta">{blurb}</div>
        </div>
      </button>
    </div>
  );
}

function Card({ card, expanded, onToggle, onAsk }: { card: CardData; expanded: boolean; onToggle: () => void; onAsk: () => void }) {
  const t = useT();
  const sc = card.scaffold;
  const matches = card.matches;
  const detected = matches.length > 0;
  const Caret = expanded ? ChevronDown : ChevronRight;
  const addReclaimed = useStore((s) => s.addReclaimed);
  const setScaffolds = useStore((s) => s.setScaffolds);
  const toast = useStore((s) => s.toast);

  const [showCleanup, setShowCleanup] = useState(false);
  const [showAllChildren, setShowAllChildren] = useState(false);
  const [ctx, setCtx] = useState<ContextMenuState | null>(null);

  const openCtx = (e: React.MouseEvent, p: string) => {
    e.preventDefault();
    setCtx({
      x: e.clientX,
      y: e.clientY,
      items: [
        {
          label: t('studio.reveal'),
          icon: <FolderOpen size={12} />,
          onClick: () => { api.revealInExplorer(p).catch(() => { /* path may have been deleted */ }); },
        },
        {
          label: t('studio.copyPath'),
          icon: <Copy size={12} />,
          onClick: () => { navigator.clipboard?.writeText(p).catch(() => { /* ignore */ }); },
        },
      ],
    });
  };

  const topChildrenAll = (() => {
    const all: Node[] = [];
    for (const m of matches) {
      for (const c of m.children ?? []) all.push(c);
    }
    all.sort((a, b) => b.size - a.size);
    return all.slice(0, 30);
  })();
  const topChildren = showAllChildren ? topChildrenAll : topChildrenAll.slice(0, 3);
  const hiddenChildrenCount = Math.max(0, topChildrenAll.length - topChildren.length);

  return (
    <div className={'studio-card-wrap risk-' + sc.risk + (detected ? ' detected' : '')}>
      <button
        className="studio-card"
        onClick={onToggle}
        title={sc.disclaimer}
      >
        <Caret size={14} className="studio-caret" />
        <div className="studio-card-icon">{ICONS[sc.id] ?? '🧹'}</div>
        <div className="studio-card-body">
          <div className="studio-card-name">{sc.name}</div>
          <div className="studio-card-meta">
            {detected
              ? <><Sparkles size={10} /> {formatBytes(card.totalSize)}{matches.length > 1 && <> · {t('studio.locations', { n: matches.length })}</>}</>
              : <>{t('studio.notDetected')}</>}
          </div>
        </div>
      </button>

      {expanded && (
        <div className="studio-card-expanded">
          {detected ? (
            <>
              <div className="studio-detail-row">
                <span className="studio-detail-label">{t('studio.pathLabel')}</span>
                <span style={{ display: 'flex', flexDirection: 'column', gap: 2, flex: 1, minWidth: 0 }}>
                  {matches.map((m) => (
                    <span
                      key={m.path}
                      className="studio-detail-path"
                      draggable
                      onDragStart={(e) => {
                        e.dataTransfer.setData('application/x-diskpilot-path', m.path);
                        e.dataTransfer.setData('application/x-diskpilot-name', m.name);
                        e.dataTransfer.effectAllowed = 'copy';
                      }}
                      onContextMenu={(e) => openCtx(e, m.path)}
                      title={t('studio.dragTip')}
                    >
                      {m.path}
                      {matches.length > 1 && (
                        <span className="muted small" style={{ marginLeft: 6 }}>
                          {formatBytes(m.size)}
                        </span>
                      )}
                    </span>
                  ))}
                </span>
              </div>
              <div className="studio-detail-row">
                <span className="studio-detail-label">{t('studio.sizeLabel')}</span>
                <span className="mono-num">
                  {t('studio.sizeMeta', { size: formatBytes(card.totalSize), files: card.totalFiles.toLocaleString() })}
                  {matches.length > 1 && <span className="muted small" style={{ marginLeft: 6 }}>{t('studio.acrossN', { n: matches.length })}</span>}
                </span>
              </div>

              {topChildrenAll.length > 0 && (
                <>
                  <div className="studio-detail-label" style={{ marginTop: 6, display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span>{t('studio.topChildren')}</span>
                    {topChildrenAll.length > 3 && (
                      <button
                        type="button"
                        className="ghost"
                        onClick={() => setShowAllChildren((v) => !v)}
                        style={{ fontSize: 10.5, padding: '0 6px' }}
                      >
                        {showAllChildren
                          ? t('studio.collapse')
                          : t('studio.expandAll', { n: hiddenChildrenCount })}
                      </button>
                    )}
                  </div>
                  <ul className="studio-children">
                    {topChildren.map((c) => (
                      <li
                        key={c.path}
                        draggable
                        onDragStart={(e) => {
                          e.dataTransfer.setData('application/x-diskpilot-path', c.path);
                          e.dataTransfer.setData('application/x-diskpilot-name', c.name);
                          e.dataTransfer.effectAllowed = 'copy';
                        }}
                        onContextMenu={(e) => openCtx(e, c.path)}
                        title={t('studio.childTip', { path: c.path })}
                      >
                        <span className="studio-child-name">{c.is_dir ? '📁' : '📄'} {c.name}</span>
                        <span className="mono-num">{formatBytes(c.size)}</span>
                      </li>
                    ))}
                  </ul>
                </>
              )}

              <div className="studio-card-actions">
                <button
                  className="primary studio-cleanup-btn"
                  onClick={() => setShowCleanup(true)}
                >
                  <Trash2 size={12} /> {t('studio.configureCleanup')}
                </button>
                <button className="secondary studio-ask-btn" onClick={onAsk}>
                  <MessageSquare size={12} /> {t('studio.askAI')}
                </button>
                <UninstallButton id={sc.id} onRemoved={() => void refreshScaffolds(setScaffolds, toast)} />
              </div>
            </>
          ) : (
            <>
              <div className="studio-detail-label">{t('studio.defaultPaths')}</div>
              <ul className="studio-children muted small">
                {sc.detect.slice(0, 4).map((p) => <li key={p}>{p}</li>)}
              </ul>
              <div className="studio-detail-label" style={{ marginTop: 6 }}>{t('studio.notesLabel')}</div>
              <p className="muted small" style={{ margin: '4px 0' }}>{sc.disclaimer}</p>
              <button className="primary studio-ask-btn" onClick={onAsk} style={{ marginTop: 6 }}>
                <MessageSquare size={12} /> {t('studio.askAIWhere')}
              </button>
            </>
          )}
        </div>
      )}

      {showCleanup && detected && (
        <CleanupModal
          scaffold={sc}
          matches={matches}
          onClose={() => setShowCleanup(false)}
          onCleaned={(bytes) => addReclaimed(bytes)}
        />
      )}
      <ContextMenu state={ctx} onClose={() => setCtx(null)} />
    </div>
  );
}

/// 用户安装的社区脚本显示「卸载」按钮（内嵌脚本不显示）。卸载只删用户
/// 目录里的 toml，内嵌脚本不受影响，删除后同名内嵌脚本自动回归生效。
function UninstallButton({ id, onRemoved }: { id: string; onRemoved: () => void }) {
  const t = useT();
  const [source, setSource] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [ask, setAsk] = useState(false);
  const toast = useStore((s) => s.toast);

  useEffect(() => {
    let alive = true;
    void api.scaffoldSource(id).then((s) => { if (alive) setSource(s); }).catch(() => { if (alive) setSource('embedded'); });
    return () => { alive = false; };
  }, [id]);

  if (source !== 'user') return null;

  const uninstall = async () => {
    setAsk(false);
    setBusy(true);
    try {
      await api.uninstallScaffold(id);
      toast(t('studio.uninstalled', { id }), 'ok');
      onRemoved();
    } catch (e) {
      toast(t('studio.uninstallFailed', { msg: String(e) }), 'err');
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <button
        className="ghost studio-uninstall-btn"
        onClick={() => setAsk(true)}
        disabled={busy}
        title={t('studio.uninstallTitle')}
      >
        <RotateCcw size={12} /> {busy ? t('studio.uninstalling') : t('studio.uninstall')}
      </button>
      {ask && (
        <ConfirmDialog
          title={t('studio.uninstall')}
          body={t('studio.uninstallConfirm', { id })}
          confirmLabel={t('studio.uninstall')}
          onConfirm={() => void uninstall()}
          onCancel={() => setAsk(false)}
        />
      )}
    </>
  );
}
