// ── 插件市场：内置 CLI 工具插件化（补写/移除 tool.plugin.json）+ 社区注册表 ──
// 独立自包含组件（state + api 调用），Settings「插件市场」tab 复用。
import { useCallback, useEffect, useState } from 'react';
import {
  ChevronDown, ChevronUp, Download, Globe, Info, Package, RefreshCw, Search, ShieldAlert, ShieldCheck, X,
} from 'lucide-react';
import { api } from '../../api';
import type {
  InstalledPlugin, MarketPlugin, RegistryPlugin, RegistryVerifyOut,
} from '../../api';
import { RISK_COLORS } from '../../colors';
import { isTauri } from '../../env';
import { useT } from '../../i18n';
import { formatBytes } from '../../format';
import { invalidateToolbeltCache } from '../../advisor/tools';
import { hasRegistryUpdate } from '../../updateSkip';

/** 索引是否"全占位"：有条目但无一提供下载地址（发布者填 URL 前展示引导）。 */
export function communityComingSoon(community: readonly RegistryPlugin[], indexed: boolean): boolean {
  return indexed && community.length > 0 && community.every((p) => !p.url);
}

/**
 * 空列表是否源于「内置去重」：索引里所有条目都与内置工具重复，被 filter_builtin_dupes
 * 过滤后 community 为空。区别于「真没条目」——此时应给用户"索引接入成功但全是内置项"的
 * 引导，而不是冷冰冰的「社区注册表为空」。
 */
export function emptiedAllDedupe(community: readonly RegistryPlugin[], skippedBuiltin: number, indexed: boolean): boolean {
  return indexed && community.length === 0 && skippedBuiltin > 0;
}

/**
 * 社区列表三态（R70）：null + warn=读失败（显示「未读到」）；null 无 warn=加载中；
 * 数组=成功（[] 即真空空市场）。读失败绝不能折叠成 [] 伪装「社区为空」。
 */
export type CommunityListState = 'loading' | 'unread' | 'ready';
export function communityListState(
  community: readonly RegistryPlugin[] | null,
  warn: string | null,
): CommunityListState {
  if (community === null) return warn ? 'unread' : 'loading';
  return 'ready';
}

/**
 * R162：已装台账排序——条目多了以后「哪个该动」要一眼看到：
 * ① 有新版的最前（这些是用户唯一需要操作的对象）；
 * ② 其余按安装时间倒序（最近装的在前，符合「刚装的东西在哪」的直觉）；
 * ③ 同刻按名字升序稳定。
 * hasUpdate 由调用方传入（它需要 community 数据，纯函数不重复拉）。
 */
export function sortInstalledPlugins<T extends { id: string; name: string; installed_at: number }>(
  installed: readonly T[],
  hasUpdate: (p: T) => boolean,
): T[] {
  return [...installed].sort((a, b) => {
    const ua = hasUpdate(a) ? 0 : 1;
    const ub = hasUpdate(b) ? 0 : 1;
    if (ua !== ub) return ua - ub;
    if (a.installed_at !== b.installed_at) return b.installed_at - a.installed_at;
    return a.name.localeCompare(b.name);
  });
}

/**
 * R163：取某个版本的更新说明（notes）——用户在决定是否更新前该看到「这版
 * 改了什么」。纯函数：versions 缺省 / 版本对不上 / notes 为空 → null（不编造，
 * 界面据此不显示说明行）。
 */
export function versionNotes(
  plugin: { version: string; versions?: { version: string; notes?: string }[] },
  version?: string,
): string | null {
  const target = version ?? plugin.version;
  const hit = (plugin.versions ?? []).find((v) => v.version === target);
  const notes = hit?.notes?.trim();
  return notes ? notes : null;
}

export function MarketPanel() {
  const t = useT();
  const [market, setMarket] = useState<MarketPlugin[] | null>(null);
  const [marketErr, setMarketErr] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [url, setUrl] = useState('');
  const [urlBusy, setUrlBusy] = useState(false);
  const [dl, setDl] = useState<{ downloaded: number; total: number } | null>(null);
  // 社区 tab（B3 + 远端分发完整版）
  const [tab, setTab] = useState<'builtin' | 'community'>('builtin');
  const [community, setCommunity] = useState<RegistryPlugin[] | null>(null);
  const [communityWarn, setCommunityWarn] = useState<string | null>(null);
  const [cQuery, setCQuery] = useState('');
  const [cBusyId, setCBusyId] = useState<string | null>(null);
  // 远端分发：索引 URL 配置 / 刷新 / 校验 / 已安装台账
  const [registryUrl, setRegistryUrl] = useState('');
  const [regBusy, setRegBusy] = useState(false);
  const [regInfo, setRegInfo] = useState<{ fetched_at: number; signed: boolean; verified: boolean; indexed: boolean; registry_url: string; plugins: number } | null>(null);
  const [verifyOut, setVerifyOut] = useState<RegistryVerifyOut | null>(null);
  const [installed, setInstalled] = useState<InstalledPlugin[] | null>(null);
  const [installedErr, setInstalledErr] = useState<string | null>(null);
  const [installedId, setInstalledId] = useState<string | null>(null);
  const [showInstalled, setShowInstalled] = useState(false);
  // 进阶折叠：未配置索引时，URL 配置/刷新/校验/台账收进一行，避免小白看到满屏术语。
  const [showAdvanced, setShowAdvanced] = useState(false);
  // 插件写操作的两步确认：confirmAsk 描述待确认动作，确认后执行（confirmed 才置位）。
  const [confirmAsk, setConfirmAsk] = useState<null | {
    title: string;
    body: string;
    onConfirm: () => void;
  }>(null);

  const refresh = useCallback(() => {
    // 安装/卸载/激活插件后工具集变化，AI 侧 toolbelt 索引缓存必须作废重拉。
    invalidateToolbeltCache();
    setMarketErr(null);
    api.pluginMarket().then(setMarket).catch((e) => {
      // 读失败 ≠ 没有插件：保持 null + 单独记错误，不能伪装成空列表
      // （空列表会渲染成「没有匹配工具」，把读失败说成查无结果）。
      setMarket(null);
      setMarketErr(String(e instanceof Error ? e.message : e));
    });
  }, []);
  useEffect(() => { refresh(); }, [refresh]);

  // 社区注册表加载 + 搜索
  const [skippedBuiltin, setSkippedBuiltin] = useState(0);
  const loadCommunity = useCallback((q = '') => {
    (q.trim() ? api.pluginRegistrySearch(q) : api.pluginRegistryList()).then((r) => {
      setCommunity(r.items);
      setSkippedBuiltin(r.skipped_builtin ?? 0);
      setCommunityWarn(r.warning ?? null);
    }).catch((e) => {
      // 读失败 ≠ 空：置 null 走「失败」分支，而不是折叠成 [] 伪装「社区为空」。
      setCommunity(null);
      setSkippedBuiltin(0);
      setCommunityWarn(t('toolbelt.mkt.loadFailed', { msg: String(e) }));
    });
  }, [t]);
  useEffect(() => {
    if (tab === 'community') loadCommunity(cQuery);
  }, [tab, cQuery, loadCommunity]);

  // 远程下载进度：plugin-download-progress 事件（B2）
  useEffect(() => {
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    import('@tauri-apps/api/event').then(async (mod) => {
      unlisten = await mod.listen<{ downloaded: number; total: number; done: boolean; installed?: string }>(
        'plugin-download-progress', (e) => {
          if (e.payload.total > 0) {
            setDl({ downloaded: e.payload.downloaded, total: e.payload.total });
          }
          if (e.payload.done) {
            setDl(null);
            setUrlBusy(false);
            if (e.payload.installed) {
              setMsg(t('toolbelt.mkt.urlInstallOk', { name: e.payload.installed }));
            }
            refresh();
          }
        });
    });
    return () => { unlisten?.(); };
  }, [refresh, t]);

  const installByUrl = useCallback(async () => {
    const u = url.trim();
    if (!u) { setMsg(t('toolbelt.mkt.needUrl')); return; }
    setConfirmAsk({
      title: t('toolbelt.mkt.confirmInstallUrlTitle'),
      body: t('toolbelt.mkt.confirmInstallUrlBody', { url: u }),
      onConfirm: () => {
        setConfirmAsk(null);
        void (async () => {
          setUrlBusy(true);
          setMsg(null);
          setDl(null);
          try {
            const dirRel = await api.pluginInstallUrl(u, true);
            setMsg(t('toolbelt.mkt.urlInstallOk', { name: dirRel }));
            refresh();
          } catch (e) {
            setMsg(t('toolbelt.mkt.urlInstallFailed', { msg: String(e) }));
          } finally {
            setUrlBusy(false);
            setDl(null);
          }
        })();
      },
    });
  }, [url, refresh, t]);

  const act = useCallback(async (p: MarketPlugin) => {
    const deactivating = p.installed;
    setConfirmAsk({
      title: deactivating ? t('toolbelt.mkt.confirmDeactivateTitle', { name: p.name }) : t('toolbelt.mkt.confirmActivateTitle', { name: p.name }),
      body: deactivating ? t('toolbelt.mkt.confirmDeactivateBody', { name: p.name }) : t('toolbelt.mkt.confirmActivateBody', { name: p.name }),
      onConfirm: () => {
        setConfirmAsk(null);
        void (async () => {
          setBusyId(p.id);
          setMsg(null);
          try {
            if (deactivating) {
              await api.pluginDeactivate(p.name, true);
              setMsg(t('toolbelt.mkt.deactivated', { name: p.name }));
            } else {
              await api.pluginActivate(p.name, true);
              setMsg(t('toolbelt.mkt.activated', { name: p.name }));
            }
            refresh();
          } catch (e) {
            setMsg(t('toolbelt.mkt.actFailed', { msg: String(e) }));
          } finally {
            setBusyId(null);
          }
        })();
      },
    });
  }, [refresh, t]);

  const filtered = (market ?? []).filter(
    (p) => !query || p.name.toLowerCase().includes(query.toLowerCase()) || p.category.includes(query),
  );
  const installedCount = (market ?? []).filter((p) => p.installed).length;

  // 已安装台账加载（installFromCommunity 的依赖也用它——声明须在其之前）。
  const loadInstalled = useCallback(() => {
    setInstalledErr(null);
    api.pluginListInstalled().then((r) => setInstalled(r.installed)).catch((e) => {
      // 读失败 ≠ 没有已安装插件：保持 null + 记错误，不伪装成空台账。
      setInstalled(null);
      setInstalledErr(String(e instanceof Error ? e.message : e));
    });
  }, []);

  // 从社区注册表一键安装：复用远程下载管线（https + Ed25519 签名强制）。
  const installFromCommunity = useCallback(async (p: RegistryPlugin) => {
    setConfirmAsk({
      title: t('toolbelt.mkt.confirmCommunityInstallTitle', { name: p.name }),
      body: t('toolbelt.mkt.confirmCommunityInstallBody'),
      onConfirm: () => {
        setConfirmAsk(null);
        void (async () => {
          setCBusyId(p.id);
          setMsg(null);
          try {
            const dirRel = await api.pluginRegistryInstall(p.id, true);
            setMsg(t('toolbelt.mkt.communityInstalled', { name: p.name, dir: dirRel }));
            loadCommunity(cQuery);
            void loadInstalled();
          } catch (e) {
            setMsg(t('toolbelt.mkt.communityInstallFailed', { msg: String(e) }));
          } finally {
            setCBusyId(null);
          }
        })();
      },
    });
  }, [loadCommunity, cQuery, t, loadInstalled]);

  // ── 远端分发完整版：索引 URL / 刷新 / 校验 / 已安装台账 ──
  const loadRegistryConfig = useCallback(() => {
    api.pluginRegistryConfig().then((r) => {
      setRegistryUrl(r.registry_url);
      setRegInfo((prev) => ({ ...(prev ?? { fetched_at: 0, signed: false, verified: false, plugins: 0 }), indexed: !!r.registry_url.trim(), registry_url: r.registry_url }));
    }).catch(() => {});
  }, []);
  useEffect(() => {
    if (tab !== 'community') return;
    loadRegistryConfig();
    loadInstalled();
  }, [tab, loadRegistryConfig, loadInstalled]);

  const doRefresh = useCallback(async () => {
    setConfirmAsk({
      title: t('toolbelt.mkt.confirmRefreshTitle'),
      body: t('toolbelt.mkt.confirmRefreshBody'),
      onConfirm: () => {
        setConfirmAsk(null);
        void (async () => {
          setRegBusy(true);
          setMsg(null);
          try {
            const r = await api.pluginRegistryRefresh(true);
            setRegInfo({
              fetched_at: r.fetched_at,
              signed: r.signed,
              verified: r.verified,
              indexed: true,
              registry_url: r.registry_url,
              plugins: r.plugins,
            });
            setMsg(r.ok ? t('toolbelt.mkt.refreshed', { n: r.plugins, sig: r.signed ? (r.verified ? t('toolbelt.mkt.sigValidSuffix') : t('toolbelt.mkt.sigInvalidSuffix')) : t('toolbelt.mkt.sigNoneSuffix') }) : t('toolbelt.mkt.refreshFailed', { msg: String(r.error) }));
            loadCommunity(cQuery);
          } catch (e) {
            setMsg(t('toolbelt.mkt.refreshFailed', { msg: String(e) }));
          } finally {
            setRegBusy(false);
          }
        })();
      },
    });
  }, [loadCommunity, cQuery, t]);

  const doVerify = useCallback(async () => {
    setMsg(null);
    try {
      setVerifyOut(await api.pluginRegistryVerify());
    } catch (e) {
      setMsg(t('toolbelt.mkt.verifyFailed', { msg: String(e) }));
    }
  }, [t]);

  const doUpdate = useCallback(async (p: InstalledPlugin) => {
    setConfirmAsk({
      title: t('toolbelt.mkt.confirmUpdateTitle', { name: p.name }),
      body: t('toolbelt.mkt.confirmUpdateBody', { version: p.version }),
      onConfirm: () => {
        setConfirmAsk(null);
        void (async () => {
          setInstalledId(p.id);
          setMsg(null);
          try {
            const r = await api.pluginRegistryUpdate(p.id, undefined, true);
            setMsg(t('toolbelt.mkt.updated', { name: p.name, version: r.version, cached: r.cached ? t('toolbelt.mkt.cachedHit') : '' }));
            await loadInstalled();
            loadCommunity(cQuery);
          } catch (e) {
            setMsg(t('toolbelt.mkt.updateFailed', { msg: String(e) }));
          } finally {
            setInstalledId(null);
          }
        })();
      },
    });
  }, [loadInstalled, loadCommunity, cQuery, t]);

  const doRollback = useCallback(async (p: InstalledPlugin) => {
    // 回滚目标 = 台账里最后一个与当前版本不同的历史版本（与后端 rollback 解析同思路）；
    // 找不到就留空由后端决定。
    const target = [...p.history].reverse().find((h) => h.version !== p.version)?.version;
    setConfirmAsk({
      title: t('toolbelt.mkt.confirmRollbackTitle', { name: p.name }),
      body: t('toolbelt.mkt.confirmRollbackBody', { version: target ?? p.version }),
      onConfirm: () => {
        setConfirmAsk(null);
        void (async () => {
          setInstalledId(p.id);
          setMsg(null);
          try {
            const r = await api.pluginRegistryRollback(p.id, undefined, true);
            setMsg(t('toolbelt.mkt.rolledBack', { name: p.name, version: r.version }));
            await loadInstalled();
            loadCommunity(cQuery);
          } catch (e) {
            setMsg(t('toolbelt.mkt.rollbackFailed', { msg: String(e) }));
          } finally {
            setInstalledId(null);
          }
        })();
      },
    });
  }, [loadInstalled, loadCommunity, cQuery, t]);

  // 卸载插件：目录移入系统回收站（可恢复），后端同步写台账 uninstall 事件
  // （installed_plugin_ids 只聚合 install/update，不写这条社区列表会永久显示「已安装」）。
  const doUninstall = useCallback(async (p: InstalledPlugin) => {
    setConfirmAsk({
      title: t('toolbelt.mkt.confirmUninstallTitle', { name: p.name }),
      body: t('toolbelt.mkt.confirmUninstallBody'),
      onConfirm: () => {
        setConfirmAsk(null);
        void (async () => {
          setInstalledId(p.id);
          setMsg(null);
          try {
            const r = await api.pluginUninstall(p.id, true);
            setMsg(t('toolbelt.mkt.uninstalled', { name: r.name }));
            await loadInstalled();
            loadCommunity(cQuery);
          } catch (e) {
            setMsg(t('toolbelt.mkt.uninstallFailed', { msg: String(e) }));
          } finally {
            setInstalledId(null);
          }
        })();
      },
    });
  }, [loadInstalled, loadCommunity, cQuery, t]);

  return (
    <div className="toolbelt-market">
      <div className="toolbelt-market-tabs">
        <button className={tab === 'builtin' ? 'active' : ''} onClick={() => setTab('builtin')}>
          <Package size={13} /> {t('toolbelt.mkt.tabBuiltin')}
        </button>
        <button className={tab === 'community' ? 'active' : ''} onClick={() => setTab('community')}>
          <Globe size={13} /> {t('toolbelt.mkt.tabCommunity')}
        </button>
      </div>

      {msg && <p className="toolbelt-market-msg">{msg}</p>}

      {tab === 'builtin' ? (
        <>
          <div className="toolbelt-market-head">
            <div>
              <h3>{t('toolbelt.mkt.builtinTitle')}</h3>
              <p className="muted small">{t('toolbelt.mkt.builtinDescA')} <code>tool.plugin.json</code>{t('toolbelt.mkt.builtinDescB')}</p>
            </div>
            <span className="toolbelt-market-count">{t('toolbelt.mkt.installedRatio', { done: installedCount, total: market ? market.length : '?' })}</span>
          </div>

          <div className="toolbelt-market-search">
            <Search size={14} />
            <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t('toolbelt.mkt.searchPlaceholder')} />
            <button className="ghost small" onClick={refresh} title={t('toolbelt.common.refresh')} aria-label={t('toolbelt.common.refresh')}><RefreshCw size={13} /> {t('toolbelt.common.refresh')}</button>
          </div>

          <div className="toolbelt-market-url">
            <Download size={14} />
            <input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder={t('toolbelt.mkt.urlPlaceholder')}
              disabled={urlBusy}
            />
            <button className="primary small" onClick={installByUrl} disabled={urlBusy}>
              {urlBusy ? t('toolbelt.mkt.downloading') : t('toolbelt.mkt.installByUrl')}
            </button>
          </div>
          {dl && dl.total > 0 && (
            <div className="toolbelt-market-progress">
              <div className="toolbelt-market-progress-bar" style={{ width: `${Math.min(100, Math.round((dl.downloaded / dl.total) * 100))}%` }} />
              <span>{formatBytes(dl.downloaded)} / {formatBytes(dl.total)}</span>
            </div>
          )}

          {marketErr !== null ? (
            <p className="error">{t('toolbelt.mkt.builtinLoadFailed', { msg: marketErr })}</p>
          ) : market === null ? (
            <p className="muted">{t('toolbelt.mkt.loading')}</p>
          ) : filtered.length === 0 ? (
            <p className="muted">{t('toolbelt.mkt.noMatch')}</p>
          ) : (
            <div className="toolbelt-market-list">
              {filtered.map((p) => (
                <div key={p.id} className={'toolbelt-market-item' + (p.installed ? ' installed' : '')}>
                  <div className="toolbelt-market-item-icon">
                    <Package size={16} />
                  </div>
                  <div className="toolbelt-market-item-main">
                    <div className="toolbelt-market-item-title">
                      <b>{p.name}</b>
                      {p.installed && <span className="toolwall-plugin-badge">{t('toolbelt.card.plugin')}</span>}
                      <span className="toolbelt-risk-chip" style={{ color: RISK_COLORS[p.risk] ?? '#94a3b8' }}>
                        {p.risk === 'high' ? t('toolbelt.mkt.riskHigh') : p.risk === 'medium' ? t('toolbelt.mkt.riskMedium') : t('toolbelt.mkt.riskLow')}
                      </span>
                      {p.permission_level && <span className="toolbelt-perm-chip">{p.permission_level}</span>}
                    </div>
                    <p className="toolbelt-market-item-desc">{p.purpose || t('toolbelt.mkt.noDesc')}</p>
                    <p className="muted small">
                      {p.category} · {p.publisher ? `${t('toolbelt.mkt.publisherPrefix')}${p.publisher} · ` : ''}{p.exe_rel}
                    </p>
                  </div>
                  <button
                    className={p.installed ? 'ghost small' : 'primary small'}
                    disabled={busyId === p.id}
                    onClick={() => act(p)}
                  >
                    {busyId === p.id ? t('toolbelt.mkt.processing') : p.installed ? t('toolbelt.mkt.deactivate') : t('toolbelt.mkt.activate')}
                  </button>
                </div>
              ))}
            </div>
          )}
        </>
      ) : (
        <>
          <div className="toolbelt-market-head">
            <div>
              <h3>{t('toolbelt.mkt.tabCommunity')}</h3>
              <p className="muted small">{t('toolbelt.mkt.communityDescA')} <code>app_data_dir</code>{t('toolbelt.mkt.communityDescB')}</p>
            </div>
            <span className="toolbelt-market-count">{t('toolbelt.cli.totalCount', { n: community?.length ?? 0 })}</span>
          </div>

          {/* 进阶：索引 URL 配置 + 刷新 + 校验（未配置索引时折叠，避免满屏术语） */}
          {!showAdvanced && !(regInfo && regInfo.indexed) ? (
            <button className="ghost small" style={{ marginTop: 8 }} onClick={() => setShowAdvanced(true)}>
              <ChevronDown size={13} style={{ verticalAlign: '-2px', marginRight: 3 }} />
              {t('toolbelt.mkt.advancedLead')}
            </button>
          ) : (
            <>
          <div className="toolbelt-market-head" style={{ marginTop: 8 }}>
            <div style={{ flex: 1, minWidth: 0, display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap' }}>
              <input
                value={registryUrl}
                onChange={(e) => setRegistryUrl(e.target.value)}
                placeholder={t('toolbelt.mkt.indexUrlPlaceholder')}
                style={{ flex: 1, minWidth: 180 }}
                spellCheck={false}
              />
              <button
                className="ghost small"
                disabled={regBusy}
                onClick={() => {
                  const u = registryUrl.trim();
                  setConfirmAsk({
                    title: t('toolbelt.mkt.confirmSetUrlTitle'),
                    body: t('toolbelt.mkt.confirmSetUrlBody', { url: u || t('toolbelt.mkt.confirmSetUrlEmpty') }),
                    onConfirm: () => {
                      setConfirmAsk(null);
                      void (async () => {
                        setRegBusy(true);
                        setMsg(null);
                        try {
                          await api.pluginSetRegistryUrl(u, true);
                          setRegInfo((prev) => ({ ...(prev ?? { fetched_at: 0, signed: false, verified: false, plugins: 0 }), indexed: !!u, registry_url: u }));
                          setMsg(u ? t('toolbelt.mkt.urlSaved') : t('toolbelt.mkt.urlCleared'));
                        } catch (e) {
                          setMsg(t('toolbelt.mkt.urlSaveFailed', { msg: String(e) }));
                        } finally {
                          setRegBusy(false);
                        }
                      })();
                    },
                  });
                }}
                title={t('toolbelt.mkt.saveUrlTitle')}
              >
                {t('toolbelt.mkt.saveUrl')}
              </button>
              <button className="primary small" disabled={regBusy} onClick={() => void doRefresh()} title={t('toolbelt.mkt.refreshTitle')}>
                <RefreshCw size={12} className={regBusy ? 'spin' : undefined} style={{ verticalAlign: '-2px', marginRight: 3 }} />
                {regBusy ? t('toolbelt.mkt.refreshing') : t('toolbelt.mkt.refreshIndex')}
              </button>
              <button className="ghost small" onClick={() => void doVerify()} title={t('toolbelt.mkt.verifyTitle')}>
                <ShieldCheck size={12} style={{ verticalAlign: '-2px', marginRight: 3 }} />
                {t('toolbelt.mkt.verify')}
              </button>
              <button className="ghost small" onClick={() => setShowInstalled((v) => !v)} title={t('toolbelt.mkt.installedLedgerTitle')}>
                <Package size={12} style={{ verticalAlign: '-2px', marginRight: 3 }} />
                {showInstalled ? t('toolbelt.mkt.collapseInstalled') : `${t('toolbelt.mkt.installedLedger')}${installed ? ` ${installed.length}` : ''}`}
              </button>
              {regInfo && regInfo.indexed && (
                <button className="ghost small" onClick={() => setShowAdvanced(false)} title={t('toolbelt.common.close')}>
                  <ChevronUp size={13} style={{ verticalAlign: '-2px', marginRight: 3 }} />
                  {t('toolbelt.mkt.advancedLead')}
                </button>
              )}
            </div>
          </div>
            </>
          )}

          {regInfo && regInfo.indexed && (
            <p className="muted small" style={{ marginTop: 6 }}>
              {t('toolbelt.mkt.indexLabel')}{regInfo.registry_url}
              {regInfo.fetched_at > 0 && <>{t('toolbelt.hwagg.updatedAt', { time: new Date(regInfo.fetched_at * 1000).toLocaleString() })}</>}
              {regInfo.plugins > 0 && <> · {t('toolbelt.mkt.pluginCount', { n: regInfo.plugins })}</>}
              {regInfo.signed && <> · {regInfo.verified ? t('toolbelt.mkt.sigOk') : t('toolbelt.mkt.sigBad')}</>}
            </p>
          )}
          {regInfo && !regInfo.indexed && (
            <p className="toolbelt-market-warn" style={{ marginTop: 6 }}>
              <Info size={12} style={{ verticalAlign: '-2px', marginRight: 3 }} />
              {t('toolbelt.mkt.noIndexHint')}
            </p>
          )}

          {/* 校验结果 */}
          {verifyOut && (
            <div className="toolbelt-market-warn" style={{ marginTop: 6 }}>
              <b>{t('toolbelt.mkt.verifyHeading')}</b>{t('toolbelt.mkt.verifyColon')}{verifyOut.signed ? (verifyOut.envelope_ok ? t('toolbelt.mkt.sigOk') : t('toolbelt.mkt.sigInvalidWith', { err: String(verifyOut.envelope_error) })) : t('toolbelt.mkt.indexUnsigned')}
              {verifyOut.items.some((i) => i.issue) && (
                <ul style={{ marginTop: 4, paddingLeft: 18 }}>
                  {verifyOut.items.filter((i) => i.issue).map((i) => (
                    <li key={i.id} className="muted small">{t('toolbelt.mkt.verifyItem', { name: i.name, version: i.version, issue: i.issue ?? '' })}</li>
                  ))}
                </ul>
              )}
            </div>
          )}

          {/* 已安装台账 */}
          {showInstalled && (
            <div className="toolbelt-market-list" style={{ marginTop: 8, maxHeight: 220, overflowY: 'auto' }}>
              {installed === null ? (
                installedErr !== null
                  ? <p className="muted small error">{t('toolbelt.mkt.installedLoadFailed', { msg: installedErr })}</p>
                  : <p className="muted small">{t('toolbelt.mkt.loading')}</p>
              ) : installed.length === 0 ? (
                <p className="muted small">{t('toolbelt.mkt.installedEmpty')}</p>
              ) : (
                sortInstalledPlugins(installed, (p) => {
                  const c = (community ?? []).find((x) => x.id === p.id);
                  return c ? hasRegistryUpdate(c, [p]) : false;
                }).map((p) => {
                  // R160：台账条目也标「有新版」——社区数据里同 id 的版本更新
                  // 时，「更新」按钮才有意义；否则用户只能点一下试试。
                  const inCommunity = (community ?? []).find((c) => c.id === p.id);
                  const upd = inCommunity ? hasRegistryUpdate(inCommunity, [p]) : false;
                  return (
                  <div key={p.id} className="toolbelt-market-item">
                    <div className="toolbelt-market-item-main">
                      <div className="toolbelt-market-item-title">
                        <b>{p.name}</b>
                        <span className="toolbelt-perm-chip">v{p.version}</span>
                        {upd && inCommunity?.version && (
                          <span className="toolwall-plugin-badge toolbelt-mkt-update-badge">
                            {t('toolbelt.mkt.updateAvailable')} v{inCommunity.version}
                          </span>
                        )}
                        <span className="toolbelt-risk-chip">{p.source === 'registry' ? t('toolbelt.mkt.srcRegistry') : p.source === 'url' ? t('toolbelt.mkt.srcUrl') : p.source === 'zip' ? t('toolbelt.mkt.srcZip') : p.source}</span>
                      </div>
                      <p className="muted small">
                        {t('toolbelt.mkt.installedAt', { time: new Date(p.installed_at * 1000).toLocaleString() })}
                        {p.history.length > 0 && <> · {t('toolbelt.mkt.historyVersions', { n: p.history.length })}</>}
                      </p>
                    </div>
                    <div style={{ display: 'flex', gap: 6 }}>
                      <button
                        className={upd ? 'primary small' : 'ghost small'}
                        disabled={installedId === p.id}
                        onClick={() => void doUpdate(p)}
                        title={t('toolbelt.mkt.updateTitle')}
                      >
                        {t('toolbelt.mkt.update')}
                      </button>
                      {p.history.length > 0 && (
                        <button
                          className="ghost small"
                          disabled={installedId === p.id}
                          onClick={() => void doRollback(p)}
                          title={t('toolbelt.mkt.rollbackTitle')}
                        >
                          {t('toolbelt.mkt.rollback')}
                        </button>
                      )}
                      <button
                        className="ghost small"
                        disabled={installedId === p.id}
                        onClick={() => void doUninstall(p)}
                        title={t('toolbelt.mkt.uninstallTitle')}
                      >
                        {t('toolbelt.mkt.uninstall')}
                      </button>
                    </div>
                  </div>
                  );
                })
              )}
            </div>
          )}

          <div className="toolbelt-market-search">
            <Search size={14} />
            <input value={cQuery} onChange={(e) => setCQuery(e.target.value)} placeholder={t('toolbelt.mkt.searchCommunityPlaceholder')} />
            <button className="ghost small" onClick={() => loadCommunity(cQuery)} title={t('toolbelt.common.refresh')}><RefreshCw size={13} /> {t('toolbelt.common.refresh')}</button>
          </div>

          {communityWarn && <p className="toolbelt-market-warn">{communityWarn}</p>}
          {skippedBuiltin > 0 && (
            <p className="muted small" style={{ marginTop: 4 }}>
              {t('toolbelt.mkt.builtinSkipped', { n: skippedBuiltin })}
            </p>
          )}

          {community === null ? (
            // null = 加载中或失败：三态由 communityListState 判定，文案两分支——
            // 读失败显示「未读到」（warn 横幅带原因），加载中才显示 loading。
            communityListState(community, communityWarn) === 'unread' ? (
              <p className="toolbelt-market-warn">{t('toolbelt.mkt.communityUnread')}</p>
            ) : (
              <p className="muted">{t('toolbelt.mkt.loading')}</p>
            )
          ) : community.length === 0 ? (
            // 索引接入成功但列表空：区分「全被内置去重」与「真没条目」。
            // 前者给引导，后者才用冷冰冰的空态文案。
            emptiedAllDedupe(community, skippedBuiltin, !!regInfo?.indexed) ? (
              <p className="toolbelt-market-warn" style={{ marginTop: 6 }}>
                <Info size={12} style={{ verticalAlign: '-2px', marginRight: 3 }} />
                {t('toolbelt.mkt.allDedupeHint', { n: skippedBuiltin })}
              </p>
            ) : (
              <p className="muted">{t('toolbelt.mkt.communityEmpty')}</p>
            )
          ) : (
            <>
              {/* 索引已接入但全部条目尚未提供下载地址：引导用户这是「等待上架」而非空市场 */}
              {communityComingSoon(community, !!regInfo?.indexed) && (
                <p className="toolbelt-market-warn" style={{ marginTop: 6 }}>
                  <Info size={12} style={{ verticalAlign: '-2px', marginRight: 3 }} />
                  {t('toolbelt.mkt.communityAllComingSoon', { n: community.length })}
                </p>
              )}
            <div className="toolbelt-market-list">
              {community.map((p) => {
                // R159：已装但远端更新 → 允许更新并显式标注；否则保持原来的
                // 安装/已装语义（不误报更新：见 hasRegistryUpdate）。
                const upd = hasRegistryUpdate(p, installed ?? []);
                // R163：有新版时把该版本的更新说明取出来——用户决定更新前
                // 该知道「这版改了什么」；没有说明则不出行（不编造）。
                const updNotes = upd ? versionNotes(p) : null;
                return (
                <div key={p.id} className="toolbelt-market-item">
                  <div className="toolbelt-market-item-icon">
                    <Globe size={16} />
                  </div>
                  <div className="toolbelt-market-item-main">
                    <div className="toolbelt-market-item-title">
                      <b>{p.name}</b>
                      {p.version && <span className="toolbelt-perm-chip">v{p.version}</span>}
                      {p.installed && <span className="toolwall-plugin-badge">{t('toolbelt.mkt.installedBadge')}</span>}
                      {upd && (
                        <span className="toolwall-plugin-badge toolbelt-mkt-update-badge">
                          {t('toolbelt.mkt.updateAvailable')}
                        </span>
                      )}
                      {p.verified && <span className="toolwall-plugin-badge">{t('toolbelt.mkt.verified')}</span>}
                      {p.license && <span className="toolbelt-perm-chip">{t('toolbelt.mkt.license', { license: p.license })}</span>}
                      {p.depends_on && p.depends_on.length > 0 && <span className="toolbelt-risk-chip">{t('toolbelt.mkt.depends', { deps: p.depends_on.join(', ') })}</span>}
                      {p.risk && (
                        <span className="toolbelt-risk-chip" style={{ color: RISK_COLORS[p.risk] ?? '#94a3b8' }}>
                          {p.risk === 'high' ? t('toolbelt.mkt.riskHigh') : p.risk === 'medium' ? t('toolbelt.mkt.riskMedium') : t('toolbelt.mkt.riskLow')}
                        </span>
                      )}
                      {p.tags?.map((tag) => <span key={tag} className="toolbelt-perm-chip">{tag}</span>)}
                    </div>
                    <p className="toolbelt-market-item-desc">{p.description || t('toolbelt.mkt.noDesc')}</p>
                    {/* R163：更新说明——决定更新前先看这版改了什么 */}
                    {updNotes && (
                      <p className="muted small toolbelt-mkt-upd-notes">
                        <b>{t('toolbelt.mkt.updNotesHead', { version: p.version })}</b> {updNotes}
                      </p>
                    )}
                    <p className="muted small">
                      {p.category} · {t('toolbelt.mkt.authorPrefix')}{p.author || t('toolbelt.common.unknown')}{p.downloads > 0 ? ` · ${t('toolbelt.mkt.downloads', { n: p.downloads })}` : ''}
                      {p.homepage ? <> · <a href={p.homepage} target="_blank" rel="noreferrer" onClick={(e) => e.stopPropagation()}>{t('toolbelt.mkt.homepage')}</a></> : ''}
                    </p>
                  </div>
                  <button
                    className="primary small"
                    disabled={cBusyId === p.id || !p.url || (!!p.installed && !upd)}
                    onClick={() => installFromCommunity(p)}
                    title={upd ? t('toolbelt.mkt.updateHint') : p.installed ? t('toolbelt.mkt.installedHint') : p.url ? t('toolbelt.mkt.installTitle') : t('toolbelt.mkt.noUrlYet')}
                  >
                    {cBusyId === p.id ? t('toolbelt.mkt.downloading')
                      : upd ? t('toolbelt.mkt.update')
                      : p.installed ? t('toolbelt.mkt.installedBadge')
                      : p.url ? t('toolbelt.mkt.install') : t('toolbelt.mkt.comingSoon')}
                  </button>
                </div>
                );
              })}
            </div>
            </>
          )}
        </>
      )}

      {confirmAsk && (
        <div className="modal-bg" onClick={() => setConfirmAsk(null)}>
          <div className="modal recycle-confirm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <div><ShieldAlert size={15} style={{ verticalAlign: '-2px', marginRight: 6 }} />{confirmAsk.title}</div>
              <button className="ghost icon" onClick={() => setConfirmAsk(null)}><X size={15} /></button>
            </div>
            <div className="modal-body">
              <p className="stress-warn">{confirmAsk.body}</p>
              <p className="muted small">{t('toolbelt.mkt.confirmNote')}</p>
            </div>
            <div className="modal-actions">
              <button className="ghost" onClick={() => setConfirmAsk(null)}>{t('toolbelt.common.cancel')}</button>
              <button className="danger primary" onClick={confirmAsk.onConfirm}>{t('toolbelt.common.confirm')}</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

