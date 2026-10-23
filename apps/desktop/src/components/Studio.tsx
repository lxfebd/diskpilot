import { useEffect, useMemo, useState } from 'react';
import { ChevronRight, ChevronDown, Sparkles, MessageSquare, Trash2, FolderOpen, Copy, ExternalLink, Gamepad2, Plus, X, RotateCcw, Download, ShieldAlert } from 'lucide-react';
import { useStore } from '../store';
import { formatBytes } from '../format';
import { t, useT } from '../i18n';
import { api, type RegistryScaffold, type Winapp2PreviewItem } from '../api';
import { isPermEnabled } from '../permissions';
import type { Node, Scaffold } from '../types';
import { ErrorBoundary } from './ErrorBoundary';
import { ContextMenu, type ContextMenuState } from './ContextMenu';
import { ConfirmDialog } from './ConfirmDialog';
import { collectScaffoldCards, type ScaffoldCard } from '../cleanup/matches';
import { SteamInspectorModal } from './SteamInspectorModal';
import { UndoPanel } from './UndoPanel';
import { copyPathWithToast } from '../clipboard';
import { revealWithToast } from '../reveal';

const FEATURED_IDS = [
  'wechat-pc',
  'conda',
];

const ICONS: Record<string, string> = {
  'wechat-pc': '💬',
  'conda':     '🐍',
};

type CardData = ScaffoldCard;

export function Studio({ onOpenCleanup, onOpenHistory }: { onOpenCleanup: (scaffoldId: string) => void; onOpenHistory?: () => void }) {
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
    return collectScaffoldCards(root, scaffolds);
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
            <Card card={c} expanded={expanded.has(c.scaffold.id)} onToggle={() => toggle(c.scaffold.id)} onAsk={() => requestStudio(c.scaffold.id)} onOpenCleanup={onOpenCleanup} />
          </ErrorBoundary>
        ))}
      </div>

      {others.length > 0 && (
        <>
          <div className="studio-section-label">{t('studio.more')}</div>
          <div className="studio-grid">
            {others.map((c) => (
              <ErrorBoundary key={c.scaffold.id} fallbackLabel={t('studio.cardFailed', { name: c.scaffold.name })}>
                <Card card={c} expanded={expanded.has(c.scaffold.id)} onToggle={() => toggle(c.scaffold.id)} onAsk={() => requestStudio(c.scaffold.id)} onOpenCleanup={onOpenCleanup} />
              </ErrorBoundary>
            ))}
          </div>
        </>
      )}

      <ErrorBoundary fallbackLabel={t('studio.fbUndoPanel')}>
        <UndoPanel onOpenHistory={onOpenHistory} />
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
  const [tab, setTab] = useState<'toml' | 'winapp2'>('toml');
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
        <div className="modal-tabs">
          <button className={'modal-tab' + (tab === 'toml' ? ' active' : '')} onClick={() => setTab('toml')}>
            {t('studio.tabToml')}
          </button>
          <button className={'modal-tab' + (tab === 'winapp2' ? ' active' : '')} onClick={() => setTab('winapp2')}>
            {t('studio.tabWinapp2')}
          </button>
        </div>
        {tab === 'toml' ? (
          <>
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
          </>
        ) : (
          <Winapp2ImportPanel onClose={onClose} onInstalled={onInstalled} />
        )}
      </div>
    </div>
  );
}

/// Winapp2.ini 社区规则导入：粘贴 → 解析预览（危险条目红线标注）→ 勾选入库。
/// 全程只读预览 + installScaffold（后端红线闸二次校验），不直接执行任何清理。
function Winapp2ImportPanel({
  onClose,
  onInstalled,
}: {
  onClose: () => void;
  onInstalled: (id: string) => void;
}) {
  const t = useT();
  const toast = useStore((s) => s.toast);
  const [ini, setIni] = useState('');
  const [items, setItems] = useState<Winapp2PreviewItem[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const parse = async () => {
    if (!ini.trim()) return;
    setBusy(true);
    setErr(null);
    try {
      const list = await api.parseWinapp2(ini);
      setItems(list);
      setSelected(new Set(list.filter((i) => !i.red_line).map((i) => i.id)));
    } catch (e) {
      // R309：此前直接丢原始异常文本（英文），这里用专用键包一层
      setErr(t('studio.winapp2ParseFailed', { msg: String(e) }));
    } finally {
      setBusy(false);
    }
  };

  const toggle = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleAll = () => {
    if (!items) return;
    const safe = items.filter((i) => !i.red_line).map((i) => i.id);
    setSelected((prev) => (prev.size === safe.length && safe.every((id) => prev.has(id)) ? new Set() : new Set(safe)));
  };

  const installSelected = async () => {
    if (!items) return;
    const chosen = items.filter((i) => selected.has(i.id) && i.toml);
    if (chosen.length === 0) {
      setErr(t('studio.winapp2NoneSelected'));
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      for (const item of chosen) {
        const id = await api.installScaffold(item.toml!);
        toast(t('studio.winapp2Installed', { id }), 'ok');
        onInstalled(id);
      }
    } catch (e) {
      setErr(t('studio.winapp2InstallFailed', { msg: String(e) }));
    } finally {
      setBusy(false);
    }
  };

  const safeCount = items?.filter((i) => !i.red_line).length ?? 0;
  const blockedCount = items ? items.length - safeCount : 0;

  return (
    <>
      <div className="modal-body">
        <p className="muted small">{t('studio.winapp2Lead')}</p>
        <textarea
          className="scaffold-toml-input"
          placeholder={t('studio.winapp2Placeholder')}
          value={ini}
          onChange={(e) => setIni(e.target.value)}
          spellCheck={false}
        />
        {err && <div className="install-err">{err}</div>}
        {items !== null && items.length > 0 && (
          <div className="winapp2-preview">
            <div className="winapp2-preview-head">
              <span className="muted small">
                {safeCount === items.length
                  ? t('studio.winapp2AllSafe')
                  : t('studio.winapp2SomeBlocked', { n: blockedCount })}
              </span>
              <button className="ghost small" onClick={toggleAll}>{selected.size === safeCount && safeCount > 0 ? t('studio.winapp2Clear') : t('studio.winapp2SelectAll')}</button>
            </div>
            <ul className="winapp2-list">
              {items.map((item) => (
                <li key={item.id} className={'winapp2-item' + (item.red_line ? ' blocked' : '')}>
                  <label className="winapp2-item-main">
                    <input
                      type="checkbox"
                      checked={selected.has(item.id)}
                      disabled={!!item.red_line}
                      onChange={() => toggle(item.id)}
                    />
                    <span className="winapp2-item-name">{item.name}</span>
                    <span className="muted small">{t('studio.winapp2Scopes', { n: item.scope_count })}</span>
                  </label>
                  {item.red_line && <div className="winapp2-redline small">{t('studio.winapp2RedLine', { msg: item.red_line })}</div>}
                  <div className="winapp2-item-disclaimer muted small">{item.disclaimer}</div>
                </li>
              ))}
            </ul>
          </div>
        )}
        {/* R309：解析出 0 条（只含注册表项/内容为空）时，不要拿「全部安全」唱空结论——
            那是 0 条的数据，不是健康结论。此前该键写了没人渲染。 */}
        {items !== null && items.length === 0 && (
          <div className="winapp2-preview muted small">{t('studio.winapp2Empty')}</div>
        )}
      </div>
      <div className="modal-foot">
        <button className="ghost" onClick={onClose} disabled={busy}>{t('studio.cancel')}</button>
        {/* R309：没有安全可装的条目（如全被红线拒绝）时按钮禁用，不让用户点了才吃一个错误提示 */}
        {items === null ? (
          <button className="primary" onClick={() => void parse()} disabled={busy || !ini.trim()}>
            {busy ? t('studio.winapp2Parsing') : t('studio.winapp2Parse')}
          </button>
        ) : (
          <button className="primary" onClick={() => void installSelected()} disabled={busy || selected.size === 0}>
            {busy ? t('studio.winapp2Installing') : t('studio.winapp2InstallSelected', { n: selected.size })}
          </button>
        )}
      </div>
    </>
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

function Card({ card, expanded, onToggle, onAsk, onOpenCleanup }: { card: CardData; expanded: boolean; onToggle: () => void; onAsk: () => void; onOpenCleanup: (scaffoldId: string) => void }) {
  const t = useT();
  const sc = card.scaffold;
  const matches = card.matches;
  const detected = matches.length > 0;
  const Caret = expanded ? ChevronDown : ChevronRight;
  const setScaffolds = useStore((s) => s.setScaffolds);
  const toast = useStore((s) => s.toast);

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
          onClick: () => { void revealWithToast(p); },
        },
        {
          label: t('studio.copyPath'),
          icon: <Copy size={12} />,
          // R316：复制要给回音（成功/失败都 toast），不再静默吞掉。
          onClick: () => { void copyPathWithToast(p); },
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
                  onClick={() => onOpenCleanup(sc.id)}
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
