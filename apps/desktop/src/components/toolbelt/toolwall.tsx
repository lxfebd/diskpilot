// ── 工具墙主体：硬件工具箱（图吧工具箱官方 CLI 文档驱动）+ W2 工具墙 ──
// HardwarePanel（CLI 视图）/ CatalogWall（12 分类图标墙）/ ToolCard / ToolDetailModal / HwAggregatePanel
import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  BookOpen, Bot, ChevronDown, ChevronRight, ExternalLink, Gauge,
  Info, LayoutGrid, Lock, Package, Play, RefreshCw, Search,
  ShieldAlert, ShieldCheck, Trash2, Wrench, X,
} from 'lucide-react';
import { api } from '../../api';
import { save } from '@tauri-apps/plugin-dialog';
import type {
  AgentToolMeta, ToolbeltCatalog, ToolbeltCatalogCategory, ToolbeltCatalogItem,
  ToolbeltRunReport, ToolbeltStatus, ToolbeltToolSpec, ToolbeltUsage, ToolManifest,
} from '../../api';
import { RISK_COLORS, LEVEL_COLORS } from '../../colors';
import { isTauri } from '../../env';
import { useT } from '../../i18n';
import { useStore } from '../../store';
import { getHwCache, subscribeHwCache, ensureHwLoaded } from '../../hwCache';
import type { SettingsTab } from '../Settings';
import { HwReportCard, HwHistorySection } from '../HwPanels';
import { splitArgs } from './services';
import {
  CATEGORY_COLOR, CATEGORY_ICON, RISK_ICON, SafeToolIcon, adNoticeFor, catDisplayName,
  plainDescriptionFor, riskLabelDisplay, toolColorFor, toolIconFor,
} from './catalog';
import type { ToolContext } from './shared';

export function HardwarePanel({ ctx }: { ctx: ToolContext }) {
  const t = useT();
  const toast = useStore((s) => s.toast);
  const { onOpenSettings } = ctx;
  const [status, setStatus] = useState<ToolbeltStatus | null>(null);
  const [catalog, setCatalog] = useState<ToolbeltCatalog | null>(null);
  const [view, setView] = useState<'cli' | 'all'>('cli');
  const [usages, setUsages] = useState<Record<string, ToolbeltUsage>>({});
  const [usageOpen, setUsageOpen] = useState<Record<string, boolean>>({});
  const [argsMap, setArgsMap] = useState<Record<string, string>>({});
  const [running, setRunning] = useState<string | null>(null);
  const [reports, setReports] = useState<Record<string, ToolbeltRunReport>>({});
  const [pending, setPending] = useState<string | null>(null);
  const [rootDraft, setRootDraft] = useState('');

  const refresh = useCallback(() => {
    if (!isTauri) return;
    api.toolbeltStatus().then((s) => {
      setStatus(s);
      setRootDraft(s.tools_root ?? '');
    });
    api.toolbeltCatalog().then(setCatalog);
  }, []);
  useEffect(refresh, [refresh]);

  const groups = useMemo(() => {
    const m = new Map<string, ToolbeltToolSpec[]>();
    for (const tool of status?.tools ?? []) {
      const arr = m.get(tool.category) ?? [];
      arr.push(tool);
      m.set(tool.category, arr);
    }
    return [...m.entries()];
  }, [status]);

  const toggleUsage = (name: string) => {
    const next = !usageOpen[name];
    setUsageOpen((m) => ({ ...m, [name]: next }));
    if (next && !usages[name]) {
      api.toolbeltUsage(name)
        .then((u) => setUsages((m) => ({ ...m, [name]: u })))
        .catch((e) => toast(String(e instanceof Error ? e.message : e), 'err'));
    }
  };

  const runTool = async (tool: ToolbeltToolSpec, confirmed = false) => {
    const args = splitArgs(argsMap[tool.name] ?? '');
    if (tool.risk !== 'low' && !confirmed) {
      setPending(tool.name);
      return;
    }
    setPending(null);
    setRunning(tool.name);
    try {
      const r = await api.toolbeltRun(tool.name, args, undefined, confirmed);
      setReports((m) => ({ ...m, [tool.name]: r }));
    } catch (e) {
      toast(String(e instanceof Error ? e.message : e), 'err');
    } finally {
      setRunning(null);
    }
  };

  if (!isTauri) {
    return (
      <div className="toolbelt-pane">
        <h2 className="toolbelt-h"><Wrench size={16} /> {t('toolbelt.cli.title')}</h2>
        <div className="tb-empty"><div>{t('toolbelt.cli.desktopOnly')}</div></div>
      </div>
    );
  }

  const missingRoot = status && !status.tools_root;

  // 全目录工具墙分组（catalog 为空时退化为 CLI 视图的统计）
  const cliInstalled = status ? status.tools.filter((tool) => tool.installed).length : 0;

  return (
    <div className="toolbelt-pane">
      <h2 className="toolbelt-h">
        <Wrench size={16} /> {t('toolbelt.cli.title')}
        <button className="ghost icon" title={t('toolbelt.cli.redetect')} aria-label={t('toolbelt.cli.redetect')} onClick={refresh}><RefreshCw size={13} /></button>
      </h2>
      <div className="toolbelt-tabs">
        <button className={'tb-tab' + (view === 'cli' ? ' active' : '')} onClick={() => setView('cli')}>
          {t('toolbelt.cli.tabCli')}
          {status && <span className="tb-count">{t('toolbelt.cli.readyCount', { installed: cliInstalled, total: status.tools.length })}</span>}
        </button>
        <button className={'tb-tab' + (view === 'all' ? ' active' : '')} onClick={() => setView('all')}>
          {t('toolbelt.cli.tabAll')}
          {catalog && <span className="tb-count">{t('toolbelt.cli.totalCount', { n: catalog.total })}</span>}
        </button>
      </div>

      {missingRoot && (
        <div className="tb-tools-root">
          <div className="tb-ext-desc">
            {t('toolbelt.cli.noRootA')}<strong>{t('toolbelt.cli.noRootStrong')}</strong>{t('toolbelt.cli.noRootB')}
          </div>
          <div className="toolbelt-actions">
            <input
              className="tb-args"
              placeholder={t('toolbelt.cli.rootPlaceholder')}
              value={rootDraft}
              onChange={(e) => setRootDraft(e.target.value)}
            />
            <button
              className="primary"
              onClick={() => api.setToolsRoot(rootDraft.trim()).then(refresh).catch(() => toast(t('toolbelt.cli.saveFail'), 'err'))}
            >
              {t('toolbelt.cli.savePath')}
            </button>
          </div>
        </div>
      )}

      {view === 'cli' ? (
        groups.map(([cat, tools]) => {
          const CatIcon = CATEGORY_ICON[cat];
          const catColor = CATEGORY_COLOR[cat] ?? '#64748b';
          return (
            <div key={cat} className="tb-ext-cat">
              <div className="tb-ext-cat-title">
                {CatIcon && <CatIcon size={14} color={catColor} />} {cat}
              </div>
              <div className="tb-ext-grid">
                {tools.map((tool) => {
                  const rep = reports[tool.name];
                  const Icon = toolIconFor(tool);
                  const color = toolColorFor(tool);
                  return (
                    <div key={tool.name} className={'tb-ext-card' + (tool.installed ? '' : ' tb-ext-missing')}>
                      <div className="tb-ext-icon" style={{ background: color + '1a', color }}>
                        <Icon size={20} color={color} strokeWidth={1.8} />
                        {!tool.installed && <span className="tb-ext-badge">{t('toolbelt.cli.missing')}</span>}
                      </div>
                      <div className="tb-ext-body">
                        <div className="tb-ext-head">
                          <span className="tb-ext-name" title={tool.name}>{tool.name}</span>
                          <span
                            className={'tb-risk tb-risk-' + tool.risk}
                            title={tool.risk === 'low' ? t('toolbelt.risk.lowTip') : tool.risk === 'medium' ? t('toolbelt.risk.mediumTip') : t('toolbelt.risk.highTip')}
                          >{riskLabelDisplay(tool.risk) ?? tool.risk}</span>
                        </div>
                        <div className="tb-ext-desc">{tool.description}</div>
                        {tool.installed ? (
                          <>
                            <div className="toolbelt-actions">
                              <button className="ghost icon" title={t('toolbelt.cli.usageTitle')} onClick={() => toggleUsage(tool.name)}>
                                {usageOpen[tool.name] ? <ChevronDown size={14} /> : <ChevronRight size={14} />}{t('toolbelt.cli.usage')}
                              </button>
                              <input
                                className="tb-args"
                                placeholder={t('toolbelt.cli.argsPlaceholder')}
                                value={argsMap[tool.name] ?? ''}
                                onChange={(e) => setArgsMap((m) => ({ ...m, [tool.name]: e.target.value }))}
                                disabled={running !== null}
                              />
                              <button className="primary" disabled={running !== null} onClick={() => runTool(tool)}>
                                {running === tool.name ? t('toolbelt.cli.running') : t('toolbelt.cli.run')}
                              </button>
                            </div>
                            {usageOpen[tool.name] && (
                              <pre className="tb-ext-usage">{usages[tool.name]?.usage ?? t('toolbelt.cli.usageLoading')}</pre>
                            )}
                            {pending === tool.name && (
                              <div className="toolbelt-actions">
                                <span className="tb-ext-desc">
                                  {t('toolbelt.cli.confirmRun', {
                                    risk: riskLabelDisplay(tool.risk) ?? tool.risk,
                                    cmd: `${tool.name} ${splitArgs(argsMap[tool.name] ?? '').join(' ').trim()}`,
                                  })}
                                </span>
                                <button className="primary" onClick={() => runTool(tool, true)}>{t('toolbelt.common.confirm')}</button>
                                <button className="ghost" onClick={() => setPending(null)}>{t('toolbelt.common.cancel')}</button>
                              </div>
                            )}
                            {rep && (
                              <pre className="tb-ext-output">
                                {`$ ${rep.command_line}\n${rep.outcome.timed_out ? t('toolbelt.cli.timedOut') : t('toolbelt.cli.exitCode', { code: rep.outcome.exit_code ?? t('toolbelt.common.unknown') })}\n`}
                                {rep.outcome.stdout}
                                {rep.outcome.stderr && `\n[stderr]\n${rep.outcome.stderr}`}
                              </pre>
                            )}
                          </>
                        ) : (
                          <div className="tb-ext-desc">{t('toolbelt.cli.notInDir')}</div>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })
      ) : (
        <CatalogWall catalog={catalog} onOpenSettings={onOpenSettings} />
      )}
    </div>
  );
}

// ════════════════════════════════════════════════════════════════════════
// W2 工具墙：原版 12 分类导航 + 图标网格 + 单击详情 / 双击启动 + 硬件聚合页
// 分类/风险/图标/推广助手已收敛在 ./catalog（CATEGORY_SLUG/catDisplayName/RISK_ICON/
// adNoticeFor/SafeToolIcon 等，见该文件头注）。
// ════════════════════════════════════════════════════════════════════════

// 硬件信息聚合页：HwReportCard 复用 + 硬盘健康 + 刷新按钮。
// 走 hwCache 模块级共享缓存：首次读取后切页不再重跑 PowerShell，
// 手动「刷新」才强制重读。
function HwAggregatePanel() {
  const t = useT();
  const toast = useStore((s) => s.toast);
  const [cache, setCache] = useState(getHwCache());

  useEffect(() => {
    const unsub = subscribeHwCache(() => setCache(getHwCache()));
    // 有缓存直接复用；无缓存才发起读取（并发请求合并为一次）。
    void ensureHwLoaded().catch((e) => toast(String(e instanceof Error ? e.message : e), 'err'));
    return unsub;
  }, [toast]);

  const refresh = () => {
    void ensureHwLoaded(true).catch((e) => toast(String(e instanceof Error ? e.message : e), 'err'));
  };

  return (
    <div className="hw-aggregate">
      <div className="hw-aggregate-head">
        <span className="muted small">
          {t('toolbelt.hwagg.summary')}
          {cache.at != null && <>{t('toolbelt.hwagg.updatedAt', { time: new Date(cache.at).toLocaleTimeString() })}</>}
        </span>
        <button className="ghost small" onClick={refresh} disabled={cache.loading} title={t('toolbelt.hwagg.refreshTitle')}>
          <RefreshCw size={12} className={cache.loading ? 'spin' : ''} /> {cache.loading ? t('toolbelt.hwagg.reading') : t('toolbelt.common.refresh')}
        </button>
      </div>
      {cache.error && <p className="hw-error">{cache.error}</p>}
      <HwReportCard info={cache.info} health={cache.health} loading={cache.loading} />
      <HwHistorySection />
    </div>
  );
}

// 工具详情卡（单击卡片弹出）：说明 / 参数表 / 示例 / 风险 / 权限级别 / 权限中心跳转 / 启动
function ToolDetailModal({
  tool,
  manifest,
  onClose,
  onLaunch,
  onOpenPerms,
}: {
  tool: ToolbeltCatalogItem;
  manifest: ToolManifest | undefined;
  onClose: () => void;
  onLaunch: () => void;
  onOpenPerms: () => void;
}) {
  const t = useT();
  const color = CATEGORY_COLOR[tool.category] ?? '#64748b';
  const showIcon = <SafeToolIcon tool={tool} size={18} color={color} />;

  const permLabel: Record<string, string> = {
    L0: t('toolbelt.detail.permL0'),
    L1: t('toolbelt.detail.permL1'),
    L2: t('toolbelt.detail.permL2'),
    L3: t('toolbelt.detail.permL3'),
  };
  const permColor = LEVEL_COLORS;
  const adNotice = adNoticeFor(tool.name);
  const [exporting, setExporting] = useState(false);
  const [exportMsg, setExportMsg] = useState<string | null>(null);
  const [recycleAsk, setRecycleAsk] = useState(false);
  const [recycling, setRecycling] = useState(false);
  const [recycleMsg, setRecycleMsg] = useState<string | null>(null);

  // 移除此工具：整个工具目录进回收站（可恢复），写 undo 日志。
  // 二次确认弹窗里明示目录路径，杜绝误删；和插件卸载走同一条撤销链路。
  const recycleTool = async () => {
    setRecycling(true);
    setRecycleMsg(null);
    try {
      const r = await api.toolbeltRecycle(tool.name, true);
      setRecycleMsg(t('toolbelt.detail.recycled', { name: r.name, dir: r.dir_rel }));
      setRecycleAsk(false);
    } catch (e) {
      setRecycleMsg(String(e instanceof Error ? e.message : e));
    } finally {
      setRecycling(false);
    }
  };

  // 分发：把已安装插件打包成 zip。save 对话框让用户选保存位置，
  // 后端按插件 id 定位目录（校验 tool.plugin.json 存在）后整包压缩。
  // 写操作：选完路径再弹一次确认（confirmed 在确认后才置位）。
  const [exportAsk, setExportAsk] = useState<{ outZip: string } | null>(null);
  const exportPlugin = async () => {
    const pluginId = tool.plugin?.id;
    if (!pluginId) return;
    setExportMsg(null);
    try {
      const picked = await save({ defaultPath: `${pluginId}-v${tool.plugin?.version ?? '0'}.zip`, filters: [{ name: t('toolbelt.detail.pluginPackFilter'), extensions: ['zip'] }] });
      if (!picked) return; // 用户取消
      setExportAsk({ outZip: picked });
    } catch (e) {
      setExportMsg(t('toolbelt.detail.exportFailed', { err: String(e) }));
    }
  };
  const doExport = async (outZip: string) => {
    const pluginId = tool.plugin?.id;
    if (!pluginId) return;
    setExportAsk(null);
    setExporting(true);
    setExportMsg(null);
    try {
      const r = await api.pluginExport(pluginId, outZip, true);
      setExportMsg(t('toolbelt.detail.exported', { zip: r.zip }));
    } catch (e) {
      setExportMsg(t('toolbelt.detail.exportFailed', { err: String(e) }));
    } finally {
      setExporting(false);
    }
  };

  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal tool-detail-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <span className="tdm-title">
            <span className="tdm-icon" style={{ background: color + '1a', color }}>{showIcon}</span>
            {tool.name}
            <span
              className={'tb-risk tb-risk-' + tool.risk}
              title={tool.risk === 'low' ? t('toolbelt.risk.lowTip') : tool.risk === 'medium' ? t('toolbelt.risk.mediumTip') : t('toolbelt.risk.highTip')}
            >{riskLabelDisplay(tool.risk) ?? tool.risk}</span>
            {manifest && (
              <span className="tdm-perm" style={{ color: permColor[manifest.permission_level] ?? LEVEL_COLORS.L1 }}>
                {permLabel[manifest.permission_level] ?? manifest.permission_level}
              </span>
            )}
          </span>
          <button className="ghost icon" onClick={onClose} title={t('toolbelt.common.close')} aria-label={t('toolbelt.common.close')}><X size={15} /></button>
        </div>

        <div className="modal-body">
          {adNotice && <p className="ad-notice">⚠️ {adNotice}</p>}
          {(plainDescriptionFor(tool.name) ?? tool.description) && <p className="tdm-desc">{plainDescriptionFor(tool.name) ?? tool.description}</p>}

          <div className="tdm-meta">
            {tool.category && <span className="tdm-meta-item">{t('toolbelt.detail.category')}<b>{catDisplayName(tool.category)}</b></span>}
            {tool.linked_from.length > 0 && (
              <span className="tdm-meta-item">{t('toolbelt.detail.linkedFrom')}<b>{tool.linked_from.map(catDisplayName).join(' / ')}</b></span>
            )}
            {tool.arch && <span className="tdm-meta-item">{t('toolbelt.detail.arch')}<b>{tool.arch}</b></span>}
            {(tool.publisher || manifest?.publisher) && (
              <span className="tdm-meta-item">{t('toolbelt.detail.publisher')}<b>{tool.publisher || manifest?.publisher}</b></span>
            )}
            {tool.version && <span className="tdm-meta-item">{t('toolbelt.detail.version')}<b>{tool.version}</b></span>}
            {tool.plugin && (
              <span className="tdm-meta-item">
                {t('toolbelt.detail.plugin')}<b>v{tool.plugin.version}</b>
                {tool.plugin.author ? ` · ${tool.plugin.author}` : ''}
                {tool.plugin.permissions.length > 0 ? t('toolbelt.detail.permissionsSuffix', { perms: tool.plugin.permissions.join('/') }) : ''}
              </span>
            )}
          </div>

          {(tool.tags.length > 0 || (manifest && manifest.tags.length > 0)) && (
            <div className="tb-ext-tags" style={{ marginTop: 8 }}>
              {(tool.tags.length > 0 ? tool.tags : manifest!.tags).map((tag) => <span key={tag} className="tb-ext-tag">{tag}</span>)}
            </div>
          )}

          {manifest && (
            <div className="tdm-manifest">
              <div className="tdm-section-title"><BookOpen size={13} /> {t('toolbelt.detail.manifestTitle')}</div>

              {manifest.purpose && <p className="tdm-line"><b>{t('toolbelt.detail.purpose')}</b>{manifest.purpose}</p>}
              {manifest.when_to_use && <p className="tdm-line"><b>{t('toolbelt.detail.whenToUse')}</b>{manifest.when_to_use}</p>}
              {manifest.when_not_to_use && <p className="tdm-line"><b>{t('toolbelt.detail.whenNotToUse')}</b>{manifest.when_not_to_use}</p>}

              {manifest.invocation.mode === 'cli' && (
                <div className="tdm-cli">
                  <div className="tdm-section-title"><ChevronRight size={13} /> {t('toolbelt.detail.cliSection')}</div>
                  {manifest.invocation.params.length > 0 && (
                    <table className="tdm-params">
                      <thead><tr><th>{t('toolbelt.detail.thParam')}</th><th>{t('toolbelt.detail.thType')}</th><th>{t('toolbelt.detail.thRequired')}</th><th>{t('toolbelt.detail.thDesc')}</th></tr></thead>
                      <tbody>
                        {manifest.invocation.params.map((p) => (
                          <tr key={p.name}>
                            <td><code>{p.flag || p.name}</code></td>
                            <td>{p.type}</td>
                            <td>{p.required ? t('toolbelt.common.yes') : t('toolbelt.common.no')}{p.default != null ? t('toolbelt.detail.defaultSuffix', { v: p.default }) : ''}</td>
                            <td>{p.desc}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}
                  {manifest.examples.length > 0 && (
                    <div className="tdm-examples">
                      {manifest.examples.map((ex, i) => (
                        <div key={i} className="tdm-example">
                          <code className="tdm-example-args">{ex.args}</code>
                          <span className="tdm-example-desc">{ex.desc}</span>
                          {ex.expect && <span className="tdm-example-expect muted small">→ {ex.expect}</span>}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {manifest.side_effects && <p className="tdm-line"><b>{t('toolbelt.detail.sideEffects')}</b>{manifest.side_effects}</p>}

              {(tool.tutorial_url || tool.download_url || manifest?.tutorial_url || manifest?.download_hint) && (
                <div className="tdm-links">
                  {(tool.tutorial_url || manifest?.tutorial_url) && (
                    <a className="ghost small" href={tool.tutorial_url || manifest?.tutorial_url} target="_blank" rel="noreferrer">
                      <ExternalLink size={12} /> {t('toolbelt.detail.tutorial')}
                    </a>
                  )}
                  {(tool.download_url || manifest?.download_hint) && (
                    <a className="ghost small" href={tool.download_url || manifest?.download_hint} target="_blank" rel="noreferrer">
                      <ExternalLink size={12} /> {t('toolbelt.detail.download')}
                    </a>
                  )}
                </div>
              )}
            </div>
          )}

          {!manifest && tool.exe_rel && (
            <p className="muted small" style={{ marginTop: 8 }}>
              {t('toolbelt.detail.launchFile', { exe: tool.exe_rel })}
            </p>
          )}
        </div>

        <div className="modal-actions">
          {tool.plugin && (
            <button
              className="ghost"
              onClick={exportPlugin}
              disabled={exporting}
              title={t('toolbelt.detail.exportTitle', { name: tool.name })}
            >
              <Package size={13} /> {exporting ? t('toolbelt.detail.exporting') : t('toolbelt.detail.export')}
            </button>
          )}
          <button className="ghost" onClick={onOpenPerms}>
            <Lock size={13} /> {t('toolbelt.detail.permCenter')}
          </button>
          <button
            className="ghost toolwall-recycle-btn"
            onClick={() => setRecycleAsk(true)}
            title={t('toolbelt.detail.recycleTitle')}
          >
            <Trash2 size={13} /> {t('toolbelt.detail.remove')}
          </button>
          <button className="ghost" onClick={onClose}>{t('toolbelt.common.close')}</button>
          <button
            className="primary"
            onClick={onLaunch}
            disabled={!tool.exe_rel}
            title={tool.exe_rel ? t('toolbelt.detail.launchTitle') : t('toolbelt.detail.noExeTitle')}
          >
            <Play size={13} /> {t('toolbelt.detail.launch')}
          </button>
        </div>
        {exportMsg && <p className="toolbelt-market-msg" style={{ marginTop: 8 }}>{exportMsg}</p>}
        {recycleMsg && <p className="toolbelt-market-msg" style={{ marginTop: 8 }}>{recycleMsg}</p>}

        {exportAsk && (
          <div className="modal-bg" onClick={() => !exporting && setExportAsk(null)}>
            <div className="modal recycle-confirm" onClick={(e) => e.stopPropagation()}>
              <div className="modal-head">
                <div><Package size={15} style={{ verticalAlign: '-2px', marginRight: 6 }} />{t('toolbelt.detail.exportConfirmTitle', { name: tool.name })}</div>
                <button className="ghost icon" onClick={() => setExportAsk(null)} disabled={exporting}><X size={15} /></button>
              </div>
              <div className="modal-body">
                <p className="stress-warn">
                  {t('toolbelt.detail.exportConfirmBody')}
                </p>
                <p className="tdm-line"><code>{exportAsk.outZip}</code></p>
                <p className="muted small">{t('toolbelt.detail.exportConfirmNote')}</p>
              </div>
              <div className="modal-actions">
                <button className="ghost" onClick={() => setExportAsk(null)} disabled={exporting}>{t('toolbelt.common.cancel')}</button>
                <button className="danger primary" onClick={() => void doExport(exportAsk.outZip)} disabled={exporting}>
                  {exporting ? t('toolbelt.detail.exporting') : t('toolbelt.detail.export')}
                </button>
              </div>
            </div>
          </div>
        )}

        {recycleAsk && (
          <div className="modal-bg" onClick={() => !recycling && setRecycleAsk(false)}>
            <div className="modal recycle-confirm" onClick={(e) => e.stopPropagation()}>
              <div className="modal-head">
                <div><Trash2 size={15} style={{ verticalAlign: '-2px', marginRight: 6 }} />{t('toolbelt.detail.removeConfirmTitle', { name: tool.name })}</div>
                <button className="ghost icon" onClick={() => setRecycleAsk(false)} disabled={recycling}><X size={15} /></button>
              </div>
              <div className="modal-body">
                <p className="stress-warn">
                  {t('toolbelt.detail.removeConfirmBody')}
                </p>
                <p className="tdm-line"><code>{tool.dir_rel}</code></p>
                <p className="muted small">{t('toolbelt.detail.removeConfirmNote', { cat: catDisplayName(tool.category) })}</p>
              </div>
              <div className="modal-actions">
                <button className="ghost" onClick={() => setRecycleAsk(false)} disabled={recycling}>{t('toolbelt.common.cancel')}</button>
                <button className="danger primary" onClick={recycleTool} disabled={recycling}>
                  {recycling ? t('toolbelt.detail.removing') : t('toolbelt.detail.removeConfirm')}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ── 缝口 A：全目录工具墙（12 分类导航 + 图标网格）──

const RECENT_KEY = 'diskpilot.toolwall.recent';

function readRecent(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    const arr = raw ? (JSON.parse(raw) as string[]) : [];
    return Array.isArray(arr) ? arr.slice(0, 12) : [];
  } catch { return []; }
}
function pushRecent(name: string): string[] {
  const next = [name, ...readRecent().filter((n) => n !== name)].slice(0, 12);
  try { localStorage.setItem(RECENT_KEY, JSON.stringify(next)); } catch { /* ignore */ }
  return next;
}

export function CatalogWall({
  catalog,
  onOpenSettings,
}: {
  catalog: ToolbeltCatalog | null;
  onOpenSettings?: (tab?: SettingsTab) => void;
}) {
  const t = useT();
  const toast = useStore((s) => s.toast);
  const [activeCat, setActiveCat] = useState<string>('__all__');
  const [detail, setDetail] = useState<ToolbeltCatalogItem | null>(null);
  const [manifests, setManifests] = useState<ToolManifest[]>([]);
  const [confirmLaunch, setConfirmLaunch] = useState<ToolbeltCatalogItem | null>(null);
  const [launching, setLaunching] = useState(false);
  const [query, setQuery] = useState('');
  const [recent, setRecent] = useState<string[]>(readRecent());
  // 「AI 可操作」分区：agent-server 的全部 MCP 工具（纯展示不启动 exe；
  // writable 字段区分只读 / 写操作，数字随后端清单动态变化）。
  const [agentTools, setAgentTools] = useState<AgentToolMeta[] | null>(null);
  const [agentDetail, setAgentDetail] = useState<AgentToolMeta | null>(null);
  // 「隐藏推广」开关：图吧包内自带流量卡/加速器返利链等纯推广 .bat，
  // 标 is_promotion 后默认**隐藏**（用户明确要求去推广）；可手动切回显示。
  const [hidePromo, setHidePromo] = useState<boolean>(() => localStorage.getItem('toolwall.hidePromo') !== '0');

  useEffect(() => {
    if (!isTauri) return;
    api.toolbeltManifests().then(setManifests).catch(() => setManifests([]));
  }, []);

  // 「AI 可操作」分区数据：agent-server 全部 MCP 工具清单（数字随真实清单）。
  // 拉取失败（agent-server 未构建/不可用）时静默降级，不阻塞工具墙本体。
  useEffect(() => {
    if (!isTauri) return;
    api.listTools()
      .then((tools) => setAgentTools(tools.length ? tools : null))
      .catch(() => setAgentTools(null));
  }, []);

    // 与后端 find_manifest 同语义的部分匹配：目录名「cpu-z」vs manifest 名
  // 「CPU-Z」、展示名「HWiNFO」vs 目录「hwinfo」都能命中，避免详情卡漏显示
  // 权限/CLI 徽标。
  const manifestFor = useCallback(
    (name: string) => {
      const q = name.trim().toLowerCase();
      if (!q) return undefined;
      return manifests.find((m) => {
        const n = m.name.toLowerCase();
        return n === q || n.includes(q) || q.includes(n);
      });
    },
    [manifests],
  );

  // 分类显示顺序：后端 category_order（原版 12 顺序）优先，未命中按现有顺序
  const navCategories = useMemo(() => {
    if (!catalog) return [];
    const order = catalog.category_order?.length ? catalog.category_order : catalog.categories.map((c) => c.name);
    const byName = new Map(catalog.categories.map((c) => [c.name, c]));
    return order
      .map((name) => byName.get(name))
      .filter((c): c is ToolbeltCatalogCategory => c !== undefined);
  }, [catalog]);

  const allTools = useMemo(() => {
    if (!catalog) return [];
    // _key 是卡片渲染 key：dir_rel 在 mock/部分 manifest 里可能缺失，用 name 兜底保证唯一
    return catalog.categories.flatMap((c) =>
      c.tools.map((t) => ({ ...t, _cat: c.name, _key: c.name + '/' + (t.dir_rel || t.name) })),
    );
  }, [catalog]);

  const activeTools = useMemo((): ToolbeltCatalogItem[] => {
    const q = query.trim().toLowerCase();
    let base: ToolbeltCatalogItem[] = allTools;
    if (activeCat !== '__all__') {
      const cat = catalog?.categories.find((c) => c.name === activeCat);
      base = cat?.tools ?? [];
    }
    if (q) {
      base = base.filter(
        (t) => t.name.toLowerCase().includes(q) || (t.description ?? '').toLowerCase().includes(q) || (t.tags ?? []).some((tag) => tag.toLowerCase().includes(q)),
      );
    }
    if (hidePromo) {
      base = base.filter((t) => !t.is_promotion);
    }
    return base;
  }, [catalog, activeCat, query, allTools, hidePromo]);

  const recentTools = useMemo(() => {
    if (activeCat !== '__all__' || query) return [] as ToolbeltCatalogItem[];
    const byName = new Map(allTools.map((t) => [t.name, t]));
    const out: ToolbeltCatalogItem[] = [];
    for (const n of recent) {
      const found = byName.get(n);
      if (found) out.push(found);
    }
    return out;
  }, [recent, allTools, activeCat, query]);

  const launchTool = useCallback(async (tool: ToolbeltCatalogItem, confirmed = false) => {
    const m = manifestFor(tool.name);
    // L3 永久禁止：直接拦截，不给确认机会
    if (m?.permission_level === 'L3') {
      toast(t('toolbelt.wall.l3Blocked', { name: tool.name }), 'err');
      setConfirmLaunch(null);
      return;
    }
    const needConfirm = tool.risk === 'medium' || tool.risk === 'high' || m?.permission_level === 'L2';
    if (needConfirm && !confirmed) {
      setConfirmLaunch(tool);
      return;
    }
    setConfirmLaunch(null);
    setLaunching(true);
    try {
      await api.toolbeltLaunch(tool.name);
      setRecent(pushRecent(tool.name));
      toast(t('toolbelt.wall.launched', { name: tool.name }), 'ok');
    } catch (e) {
      toast(String(e instanceof Error ? e.message : e), 'err');
    } finally {
      setLaunching(false);
    }
  }, [manifestFor, toast, t]);

  // 低危工具单击直接启动（有主程序时）；内置链接无 exe 则打开详情。
  // 推广项（is_promotion）不直接启动——它们是 `start <推广URL>` 跳转，单击
  // 应打开详情让用户看清是推广再决定，避免误触直接弹浏览器广告页。
  const onCardClick = (tool: ToolbeltCatalogItem) => {
    if (tool.is_promotion || !tool.exe_rel) { setDetail(tool); return; }
    const m = manifestFor(tool.name);
    const lowRisk = tool.risk !== 'medium' && tool.risk !== 'high' && m?.permission_level !== 'L2' && m?.permission_level !== 'L3';
    if (lowRisk) { void launchTool(tool); return; }
    // 中/高危：就地确认（不弹整屏 modal）
    setConfirmLaunch(tool);
  };

  if (!catalog) {
    return <div className="tb-empty"><div>{t('toolbelt.wall.scanning')}</div></div>;
  }
  if (!catalog.tools_root || catalog.categories.length === 0) {
    return (
      <div className="tb-empty">
        <div>{t('toolbelt.wall.noRoot')}</div>
      </div>
    );
  }

  const hasRecent = recentTools.length > 0;

  return (
    <div className="toolwall">
      {/* 顶栏：搜索 + 计数 */}
      <div className="toolwall-top">
        <div className="toolwall-search">
          <Search size={13} />
          <input
            placeholder={t('toolbelt.wall.searchPlaceholder')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            spellCheck={false}
          />
          {query && (
            <button className="ghost icon" onClick={() => setQuery('')} title={t('toolbelt.wall.clear')}><X size={12} /></button>
          )}
        </div>
        <span className="muted small toolwall-count">
          {t('toolbelt.wall.toolCount', { n: activeTools.length })}
          <button
            className={'ghost toolwall-hidepromo' + (hidePromo ? ' active' : '')}
            onClick={() => {
              const next = !hidePromo;
              setHidePromo(next);
              localStorage.setItem('toolwall.hidePromo', next ? '1' : '0');
            }}
            title={hidePromo ? t('toolbelt.wall.promoHidden') : t('toolbelt.wall.promoShow')}
          >
            <span className="toolwall-hidepromo-dot" /> {t('toolbelt.wall.promoToggle')}
          </button>
        </span>
      </div>

      {/* 左：12 分类导航（全部 = 聚合视图，硬件信息 = 聚合页） */}
      <div className="toolwall-nav">
        <button
          className={'toolwall-nav-item' + (activeCat === '__all__' ? ' active' : '')}
          onClick={() => setActiveCat('__all__')}
          title={t('toolbelt.wall.allTools')}
        >
          <LayoutGrid size={15} />
          <span className="toolwall-nav-name">{t('toolbelt.wall.all')}</span>
          <span className="toolwall-nav-count">{allTools.length}</span>
        </button>
        <button
          className={'toolwall-nav-item' + (activeCat === '__hardware__' ? ' active' : '')}
          onClick={() => setActiveCat('__hardware__')}
        >
          <Gauge size={15} />
          <span className="toolwall-nav-name">{t('toolbelt.cat.hardware')}</span>
        </button>
        {navCategories.map((c) => {
          const display = catDisplayName(c.name);
          const Icon = CATEGORY_ICON[c.name];
          const color = CATEGORY_COLOR[c.name] ?? '#64748b';
          return (
            <button
              key={c.name}
              className={'toolwall-nav-item' + (activeCat === c.name ? ' active' : '')}
              onClick={() => setActiveCat(c.name)}
              title={c.name}
            >
              {Icon ? <Icon size={15} style={{ color }} /> : <Package size={15} />}
              <span className="toolwall-nav-name">{display}</span>
              <span className="toolwall-nav-count">{c.tools.length}</span>
            </button>
          );
        })}
      </div>

      {/* 右：当前分类的图标网格 / 硬件聚合页 */}
      <div className="toolwall-main">
        {activeCat === '__hardware__' ? (
          <HwAggregatePanel />
        ) : (
          <>
            {hasRecent && (
              <div className="toolwall-recent">
                <div className="toolwall-grid-title recent-title">{t('toolbelt.wall.recent')}</div>
                <div className="toolwall-grid">
                  {recentTools.map((t) => (
                    <ToolCard
                      key={'recent-' + (t.dir_rel || t.name)}
                      tool={t}
                      manifest={manifestFor(t.name)}
                      onOpenDetail={setDetail}
                      onLaunch={() => onCardClick(t)}
                      launching={launching}
                    />
                  ))}
                </div>
              </div>
            )}
            <div className="toolwall-grid-head">
              <span className="toolwall-grid-title">{query ? t('toolbelt.wall.searchResult', { n: activeTools.length }) : (activeCat === '__all__' ? t('toolbelt.wall.allTools') : catDisplayName(activeCat))}</span>
              <span className="muted small">{t('toolbelt.wall.gridHint')}</span>
            </div>
            {activeTools.length === 0 ? (
              <div className="tb-empty small">
                <div>{query ? t('toolbelt.wall.noMatch', { query }) : t('toolbelt.wall.catEmpty')}</div>
              </div>
            ) : (
              <div className="toolwall-grid">
                {activeTools.map((t) => (
                  <ToolCard
                    key={(t as ToolbeltCatalogItem & { _key?: string })._key ?? t.dir_rel}
                    tool={t}
                    manifest={manifestFor(t.name)}
                    onOpenDetail={setDetail}
                    onLaunch={() => onCardClick(t)}
                    launching={launching}
                  />
                ))}
              </div>
            )}

            {/* 「AI 可操作」分区：agent-server 全部 MCP 工具（只读/写按 writable 标注） */}
            {activeCat === '__all__' && !query && agentTools && agentTools.length > 0 && (
              <div className="toolwall-ai-section">
                <div className="toolwall-grid-head">
                  <span className="toolwall-grid-title">{t('toolbelt.wall.aiSection', { n: agentTools.length })}</span>
                  <span className="muted small">{t('toolbelt.wall.aiSectionHint')}</span>
                </div>
                <div className="toolwall-grid toolwall-ai-grid">
                  {agentTools.map((mt) => (
                    <button
                      key={mt.name}
                      className="toolwall-ai-card"
                      onClick={() => setAgentDetail(mt)}
                      title={mt.description}
                    >
                      <span className="toolwall-ai-name">{mt.name}</span>
                      <span className="toolwall-ai-desc">{mt.description}</span>
                      <span className={'toolwall-ai-badge' + (mt.writable ? ' toolwall-ai-badge-write' : '')}>
                        {mt.writable ? t('toolbelt.wall.aiWrite') : t('toolbelt.wall.aiRead')}
                      </span>
                    </button>
                  ))}
                </div>
              </div>
            )}
          </>
        )}
      </div>

      {/* 就地启动确认（中/高危）：覆盖在卡片上方，非整屏 modal */}
      {confirmLaunch && (
        <div className="toolwall-inline-confirm modal-bg" onClick={() => setConfirmLaunch(null)}>
          <div className="modal confirm-modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span><ShieldAlert size={14} /> {t('toolbelt.wall.launchConfirmTitle')}</span>
              <button className="ghost icon" onClick={() => setConfirmLaunch(null)} title={t('toolbelt.common.close')}><X size={15} /></button>
            </div>
            <div className="modal-body">
              <p>
                {t('toolbelt.wall.launchConfirmA', { name: confirmLaunch.name })}<b style={{ color: RISK_COLORS[confirmLaunch.risk] ?? RISK_COLORS.medium }}>
                  {riskLabelDisplay(confirmLaunch.risk) ?? confirmLaunch.risk}
                </b>{t('toolbelt.wall.launchConfirmB')}
              </p>
              {adNoticeFor(confirmLaunch.name) && (
                <p className="ad-notice">
                  ⚠️ {adNoticeFor(confirmLaunch.name)}
                </p>
              )}
              <p className="muted small">{t('toolbelt.wall.launchConfirmAsk')}</p>
            </div>
            <div className="modal-actions">
              <button className="ghost" onClick={() => setConfirmLaunch(null)}>{t('toolbelt.common.cancel')}</button>
              <button className="primary" onClick={() => launchTool(confirmLaunch, true)} disabled={launching}>
                {launching ? t('toolbelt.wall.launching') : t('toolbelt.wall.launchConfirmBtn')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 详情卡（由卡片「信息」按钮打开） */}
      {detail && (
        <ToolDetailModal
          tool={detail}
          manifest={manifestFor(detail.name)}
          onClose={() => setDetail(null)}
          onLaunch={() => { setDetail(null); launchTool(detail); }}
          onOpenPerms={() => {
            setDetail(null);
            onOpenSettings?.('perms');
          }}
        />
      )}

      {/* 「AI 可操作」工具说明（只读 MCP 工具详情） */}
      {agentDetail && (
        <div className="modal-bg" onClick={() => setAgentDetail(null)}>
          <div className="modal confirm-modal" onClick={(e) => e.stopPropagation()} style={{ maxWidth: 480 }}>
            <div className="modal-head">
              <span><Bot size={14} /> {t('toolbelt.wall.aiDetailTitle', { name: agentDetail.name })}</span>
              <button className="ghost icon" onClick={() => setAgentDetail(null)} title={t('toolbelt.common.close')}><X size={15} /></button>
            </div>
            <div className="modal-body">
              <p>{agentDetail.description}</p>
              <div className="tdm-meta-item" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                <span className="tb-risk tb-risk-low">{agentDetail.writable ? t('toolbelt.wall.aiWrite') : t('toolbelt.wall.aiL0Read')}</span>
                <span className="muted small">{agentDetail.writable ? t('toolbelt.wall.aiWriteDesc') : t('toolbelt.wall.aiReadDesc')}</span>
              </div>
              {agentDetail.input_schema && (() => {
                const props = (agentDetail.input_schema as { properties?: Record<string, { type?: string; description?: string }> }).properties;
                const entries = Object.entries(props ?? {});
                if (!entries.length) return <p className="muted small">{t('toolbelt.wall.aiNoParams')}</p>;
                return (
                  <>
                    <p className="muted small" style={{ marginTop: 10 }}>{t('toolbelt.wall.aiParams')}</p>
                    <ul className="tdm-args-list" style={{ margin: '4px 0 0', paddingLeft: 18 }}>
                      {entries.map(([k, v]) => (
                        <li key={k}>
                          <code>{k}</code>
                          <span className="muted small"> · {v.type ?? '?'}</span>
                          {v.description && <span className="muted small"> — {v.description}</span>}
                        </li>
                      ))}
                    </ul>
                  </>
                );
              })()}
            </div>
            <div className="modal-actions">
              <button className="ghost" onClick={() => setAgentDetail(null)}>{t('toolbelt.common.close')}</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}


function ToolCard({
  tool,
  manifest,
  onOpenDetail,
  onLaunch,
  launching,
}: {
  tool: ToolbeltCatalogItem & { _cat?: string };
  manifest: ToolManifest | undefined;
  onOpenDetail: (t: ToolbeltCatalogItem) => void;
  onLaunch: () => void;
  launching: boolean;
}) {
  const t = useT();
  const color = CATEGORY_COLOR[tool.category] ?? '#64748b';
  const RiskIcon = RISK_ICON[tool.risk] ?? ShieldCheck;
  const riskColor = RISK_COLORS[tool.risk] ?? RISK_COLORS.high;
  const isLinked = tool.is_linked && !tool.is_builtin_link;
  const clickable = !!tool.exe_rel;
  const adNotice = adNoticeFor(tool.name);
  const isPromo = !!tool.is_promotion;

  return (
    <div
      className={'toolwall-card' + (tool.is_builtin_link ? ' toolwall-builtin' : '') + (clickable ? ' clickable' : '') + (isPromo ? ' toolwall-promo' : '')}
      onClick={() => onOpenDetail(tool)}
      onDoubleClick={() => clickable && !isPromo && !launching && onLaunch()}
      title={clickable ? `${tool.name}\n${t('toolbelt.card.clickHint')}${isPromo ? '\n' + t('toolbelt.card.promoHint') : ''}${adNotice ? ' · ' + adNotice : ''}` : `${tool.name}\n${t('toolbelt.card.builtinOp')}`}
    >
      <button
        className="toolwall-info-btn"
        title={t('toolbelt.card.detail')}
        onClick={(e) => { e.stopPropagation(); onOpenDetail(tool); }}
      >
        <Info size={13} />
      </button>
      <div className="toolwall-icon" style={{ background: color + '1a', color }}>
        <SafeToolIcon tool={tool} size={22} color={color} />
      </div>
      <div className="toolwall-card-name" title={tool.name}>{tool.name}</div>
      {(plainDescriptionFor(tool.name) ?? tool.description) && <div className="toolwall-card-desc">{plainDescriptionFor(tool.name) ?? tool.description}</div>}
      <div className="toolwall-card-foot">
        <span className="toolwall-risk" style={{ color: riskColor }} title={t('toolbelt.card.riskLevel')}>
          <RiskIcon size={11} /> {riskLabelDisplay(tool.risk) ?? tool.risk}
        </span>
        {manifest && manifest.invocation.mode === 'cli' && <span className="toolwall-cli-badge" title={t('toolbelt.card.cliTitle')}>CLI</span>}
        {tool.plugin && (
          <span className="toolwall-plugin-badge" title={t('toolbelt.card.pluginTitle', { version: tool.plugin.version, author: tool.plugin.author || t('toolbelt.card.anonAuthor'), entry: tool.plugin.entry })}>
            {t('toolbelt.card.plugin')}
          </span>
        )}
        {isLinked && (
          <span className="toolwall-link-badge" title={t('toolbelt.card.entryTitle', { from: tool.linked_from.map(catDisplayName).join(' / ') })}>
            {t('toolbelt.card.entry')}
          </span>
        )}
        {tool.is_builtin_link && <span className="toolwall-link-badge" title={t('toolbelt.card.builtinOp')}>{t('toolbelt.card.builtin')}</span>}
        {adNotice && (
          <span className="toolwall-ad-badge" title={adNotice}>{t('toolbelt.card.hasAd')}</span>
        )}
        {isPromo && (
          <span className="toolwall-promo-badge" title={t('toolbelt.card.promoTitle')}>{t('toolbelt.card.promo')}</span>
        )}
      </div>
    </div>
  );
}

