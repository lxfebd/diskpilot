import { useEffect, useReducer, useState } from 'react';
import { getHwProfile, recommendLocal, type HwProfile } from '../hwProfile';
import { X, CheckCircle2, Info, Eye, EyeOff, Settings2, Palette, Sparkles, SlidersHorizontal, ShieldCheck, RefreshCw, ExternalLink, Download, RotateCw, Wrench, Plug, Package, Bell } from 'lucide-react';
import { api } from '../api';
import { isTauri } from '../env';
import { ConfirmDialog } from './ConfirmDialog';
import { check as updaterCheck, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { PermissionCenter } from './PermissionCenter';
import { SystemTools } from './SystemTools';
import { McpServers } from './McpServers';
import { MarketPanel } from './toolbelt/market';
import { ReminderSettings } from './ReminderSettings';
import {
  loadSettings,
  saveSettings,
  clearSettings,
  detectProvider,
  freeChat,
  type Provider,
} from '../advisorClient';
import {
  loadTheme,
  saveTheme,
  prefs,
  setPref,
  getKeepFiles,
  setKeepFiles,
  type ThemeSettings,
  type ThemeId,
  type PrefName,
} from '../theme';
import { useT, setLang, getLangMode, type Lang } from '../i18n';

type Props = { onClose: () => void; initialTab?: SettingsTab };

// 表里存的是**文案键**而不是中文：模块顶层常量若在定义处求值，会把中文烤死在
// 首次 import，切语言不再生效。渲染时再 t()。
const PROVIDER_LABEL: Record<Provider, string> = {
  openai: 'settings.proto.openai',
  anthropic: 'settings.proto.anthropic',
  gemini: 'settings.proto.gemini',
  ollama: 'settings.proto.ollama',
};

// 手动开关里的选项要短，五个塞在一行；「已识别」标签用完整版说明。
const PROVIDER_LABEL_SHORT: Record<Provider, string> = {
  openai: 'settings.proto.openai',
  anthropic: 'settings.proto.anthropic',
  gemini: 'settings.proto.gemini',
  ollama: 'settings.proto.ollamaShort',
};

export type SettingsTab = 'appearance' | 'ai' | 'general' | 'perms' | 'system' | 'mcp' | 'market' | 'reminder';

// 预设色板：每套主题各有几个「主色」，点一下就换 accent 家族。
// 主色取自参考库的品牌原始值，让自定义色快捷预览贴近对应设计语言。
const SWATCHES: Record<ThemeId, string[]> = {
  vercel:     ['#121212', '#1447e6', '#16a34a', '#b45309', '#e7000b'],
  minimal:    ['#18181b', '#52525b', '#71717a', '#27272a', '#8f8fa3'],
  doubao:     ['#0065fd', '#00b578', '#d9800a', '#ef4444', '#9a8fd9'],
  claude:     ['#c96442', '#934828', '#e9e6dc', '#a56a2e', '#7aa887'],
  google:     ['#1a73e8', '#188038', '#b06000', '#d93025', '#9a8fd9'],
  volcengine: ['#1664ff', '#0f9b62', '#c28300', '#e02e1f', '#9a90e0'],
  nerv:       ['#ea343a', '#4fd0a0', '#f0a35e', '#a083e8', '#f4f9ff'],
  trae:       ['#4b3fe3', '#00b983', '#ff6b45', '#f2a90c', '#6a6fff'],
  motionfit:  ['#ff4000', '#d6ff0a', '#00ff1e', '#ff3def', '#737373'],
  barbie:     ['#e11d48', '#ffafcc', '#a85d76', '#d89fc4', '#7a5c4a'],
  brand:      ['#007aff', '#0055b3', '#34c759', '#ff9500', '#ff3b30'],
  golden:     ['#b98a4f', '#8a6136', '#d4c7a4', '#5a4f43', '#a56a2e'],
  vibecamp:   ['#f1481e', '#d63a14', '#211d1a', '#8a2914', '#a56a2e'],
  '21th':     ['#5262e8', '#232327', '#2f9e63', '#b07a2e', '#d64527'],
  steam:      ['#66c0f4', '#a4d7f9', '#7ecba1', '#f5b071', '#ff6d85'],
  dark:       ['#ea343a', '#4fd0a0', '#f0a35e', '#a083e8', '#f4f9ff'],
  pro:        ['#1664ff', '#4d8dff', '#6fd4a0', '#f5b071', '#ff7d8a'],
}

const THEME_LABEL: Record<ThemeId, string> = {
  vercel:     'theme.vercel',
  minimal:    'theme.minimal',
  doubao:     'theme.doubao',
  claude:     'theme.claude',
  google:     'theme.google',
  volcengine: 'theme.volcengine',
  nerv:       'theme.nerv',
  trae:       'theme.trae',
  motionfit:  'theme.motionfit',
  barbie:     'theme.barbie',
  brand:      'theme.brand',
  golden:     'theme.golden',
  vibecamp:   'theme.vibecamp',
  '21th':     'theme.21th',
  steam:      'theme.steam',
  dark:       'theme.dark',
  pro:        'theme.pro',
};
// 主题名走 i18n（THEME_LABEL 存 key，渲染时 t() 取值，未翻译回退中文）。
const themeLabel = (id: ThemeId, t: (k: string) => string): string => t(THEME_LABEL[id]);

export function Settings({ onClose, initialTab }: Props) {
  const t = useT();
  const [tab, setTab] = useState<SettingsTab>(initialTab ?? 'appearance');
  const [lang, setLangState] = useState<'system' | Lang>(getLangMode());

  // ── AI tab state ──
  const [baseUrl, setBaseUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [model, setModel] = useState('');
  const [showKey, setShowKey] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [providerOverride, setProviderOverride] = useState<Provider | undefined>(undefined);
  const [showManual, setShowManual] = useState(false);
  const [testing, setTesting] = useState(false);

  // ── Appearance tab state ──
  const [theme, setTheme] = useState(loadTheme());

  // ── General tab state ──
  // 通用开关以 localStorage（prefs getter）为唯一数据源：渲染时直接读 getter，
  // 切换后 bump 一次版本触发重渲染，避免组件内再存一份状态导致双源漂移。
  const [, bumpPrefs] = useReducer((v: number) => v + 1, 0);
  const [keepFiles, setKeepFilesState] = useState(getKeepFiles());
  // 本机性能档位：决定本地 Ollama 是否吃得消、该建议云端还是本地。
  const [hw, setHw] = useState<HwProfile | null>(null);
  // 检查更新：null = 未查过；字符串 = 出错信息；对象 = 结果。
  const [update, setUpdate] = useState<import('../api').UpdateInfo | null | string | undefined>(undefined);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  // 一键更新：null = 未下载；'confirm' = 待两步确认；'downloading' = 下载中；'installed' = 装完待重启；字符串 = 出错。
  const [updatePhase, setUpdatePhase] = useState<null | 'confirm' | 'downloading' | 'installed' | string>(null);
  const [updateProgress, setUpdateProgress] = useState(0);

  const checkUpdate = async () => {
    setCheckingUpdate(true);
    try {
      setUpdate(await api.checkUpdate());
    } catch (e) {
      setUpdate(String(e instanceof Error ? e.message : e));
    } finally {
      setCheckingUpdate(false);
    }
  };

  const installUpdate = async (u: Update) => {
    setUpdatePhase('downloading');
    setUpdateProgress(0);
    let total = 0;
    let downloaded = 0;
    try {
      await u.downloadAndInstall((event) => {
        if (event.event === 'Started' && event.data.contentLength != null) {
          total = event.data.contentLength;
        } else if (event.event === 'Progress') {
          downloaded += event.data.chunkLength;
          if (total > 0) setUpdateProgress(Math.round((downloaded / total) * 100));
        }
      });
      setUpdatePhase('installed');
    } catch (e) {
      setUpdatePhase(String(e instanceof Error ? e.message : e));
    }
  };

  // 两步确认通过后才真正检查并下载：点击按钮只弹确认框。
  const confirmAndInstall = async () => {
    try {
      const u = await updaterCheck();
      if (!u) {
        setUpdatePhase(null);
        return;
      }
      await installUpdate(u);
    } catch (e) {
      setUpdatePhase(String(e instanceof Error ? e.message : e));
    }
  };

  const restartApp = async () => {
    try {
      await relaunch();
    } catch (e) {
      setUpdatePhase(String(e instanceof Error ? e.message : e));
    }
  };

  useEffect(() => {
    const existing = loadSettings();
    if (existing) {
      setModel(existing.model);
      setApiKey(existing.apiKey);
      setBaseUrl(existing.baseUrl);
      setProviderOverride(existing.providerOverride);
      setSaved(true);
    }
  }, []);

  // 读取本机性能档位（CPU 核数/内存），用于给 AI 配置建议「用云端还是本地」。
  useEffect(() => {
    getHwProfile().then(setHw).catch(() => setHw(null));
  }, []);

  // 主题在别处被改（如 future 的快捷切换）时，设置页里的色板/滑条要跟着变。
  useEffect(() => {
    const onTheme = (e: Event) => {
      const detail = (e as CustomEvent<ThemeSettings | undefined>).detail;
      if (detail) setTheme(detail);
    };
    window.addEventListener('diskpilot:theme-changed', onTheme);
    return () => window.removeEventListener('diskpilot:theme-changed', onTheme);
  }, []);

  // 硬件加速开关以后端 general.json 为权威（它决定 WebView2 是否 --disable-gpu），
  // 打开设置页时同步一次，避免与 localStorage 漂移。
  useEffect(() => {
    if (!isTauri) return;
    api.generalConfig()
      .then((c) => {
        setPref('hardwareAccel', c.hardware_accel);
        bumpPrefs();
      })
      .catch(() => {});
  }, []);

  const provider = providerOverride ?? detectProvider(baseUrl);
  const needsKey = provider !== 'ollama';

  // 本机性能档位对应的本地 Ollama 建议（弱机→云端，中机→可本地，强机→放心本地）。
  // 横幅配色走语义 token 类（hint-warn/hint-info/hint-ok/hint-danger），不内联浅色 hex，
  // 否则深色主题下 --ink-2 反相为浅色 → 浅底浅字不可读（dark.css 无需补丁即适配）。
  const hwRec = hw ? recommendLocal(hw.tier) : null;
  const hwRecBanner =
    hwRec === 'suggest-cloud' ? (
      <div className="hint hint-warn">
        <strong>{t('settings.ai.hwRecCloud')}</strong><br />
        {t('settings.ai.hwRecCloudDesc')}
      </div>
    ) : hwRec === 'allow-local' ? (
      <div className="hint hint-info">
        {t('settings.ai.hwRecTryLocal')}
      </div>
    ) : hwRec === 'recommend-local' ? (
      <div className="hint hint-ok">
        {t('settings.ai.hwRecLocal')}
      </div>
    ) : null;
  const weakMachineWarn =
    hw?.tier === 'weak' ? (
      <div className="hint hint-danger" style={{ marginTop: 8 }}>
        <strong>{t('settings.ai.weakWarnTitle')}</strong>{t('settings.ai.weakWarn')}
      </div>
    ) : null;

  const save = async () => {
    setErr(null); setMsg(null);
    if (!baseUrl.trim()) { setErr(t('settings.ai.errBaseUrl')); return; }
    if (!model.trim())   { setErr(t('settings.ai.errModel')); return; }
    if (needsKey && !apiKey.trim()) { setErr(t('settings.ai.errKey')); return; }
    try {
      saveSettings({ provider, model, apiKey, baseUrl, providerOverride });
      setMsg(t('settings.ai.saved'));
      setSaved(true);
    } catch (e) {
      setErr(String(e));
    }
  };

  const wipe = () => {
    clearSettings();
    setApiKey('');
    setBaseUrl('');
    setModel('');
    setProviderOverride(undefined);
    setShowManual(false);
    setSaved(false);
    setMsg(t('settings.ai.wiped'));
  };

  const testConn = async () => {
    setErr(null); setMsg(null); setTesting(true);
    try {
      saveSettings({ provider, model, apiKey, baseUrl, providerOverride });
      const reply = await freeChat('', '回复"连接成功"四个字，别的什么都别说。'); // @i18n-keep 发给模型的探针 prompt，恒中文
      setMsg(`${t('settings.ai.connOk')}${reply.slice(0, 40)}`);
    } catch (e) {
      setErr(`${t('settings.ai.connFail')}${String(e)}`);
    } finally {
      setTesting(false);
    }
  };

  const updateTheme = (patch: Partial<typeof theme>) => {
    const next = { ...theme, ...patch };
    setTheme(next);
    saveTheme(next);
  };

  const togglePref = (name: PrefName, on: boolean) => {
    setPref(name, on);
    bumpPrefs();
    if (name === 'hardwareAccel') {
      // 硬件加速由后端 Rust 启动时读 general.json 决定，重启后生效。
      api.setGeneral(on).catch(() => {});
    }
  };

  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>{t('settings.title')} {saved && <CheckCircle2 size={16} style={{ verticalAlign: 'middle', marginLeft: 6, color: 'var(--accent-strong)' }} />}</div>
          <button className="ghost icon" onClick={onClose}><X size={16} /></button>
        </div>

        <div className="settings-tabs">
          <button className={`settings-tab${tab === 'appearance' ? ' active' : ''}`} onClick={() => setTab('appearance')}>
            <Palette size={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('settings.appearance')}
          </button>
          <button className={`settings-tab${tab === 'ai' ? ' active' : ''}`} onClick={() => setTab('ai')}>
            <Sparkles size={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('settings.ai')}
          </button>
          <button className={`settings-tab${tab === 'general' ? ' active' : ''}`} onClick={() => setTab('general')}>
            <SlidersHorizontal size={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('settings.general')}
          </button>
          <button className={`settings-tab${tab === 'perms' ? ' active' : ''}`} onClick={() => setTab('perms')}>
            <ShieldCheck size={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('settings.perms')}
          </button>
          <button className={`settings-tab${tab === 'system' ? ' active' : ''}`} onClick={() => setTab('system')}>
            <Wrench size={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('settings.system')}
          </button>
          <button className={`settings-tab${tab === 'mcp' ? ' active' : ''}`} onClick={() => setTab('mcp')}>
            <Plug size={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('settings.mcp')}
          </button>
          <button className={`settings-tab${tab === 'market' ? ' active' : ''}`} onClick={() => setTab('market')}>
            <Package size={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('settings.market')}
          </button>
          <button className={`settings-tab${tab === 'reminder' ? ' active' : ''}`} onClick={() => setTab('reminder')}>
            <Bell size={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('settings.reminder')}
          </button>
        </div>

        {tab === 'appearance' && (
          <div className="settings-pane">
            <label className="field">
              <span>{t('settings.language')}</span>
              <div className="seg seg-3">
                {(['system', 'zh', 'en'] as const).map((l) => (
                  <button
                    key={l}
                    type="button"
                    className={`seg-opt${lang === l ? ' active' : ''}`}
                    onClick={() => {
                      setLang(l);
                      setLangState(l);
                    }}
                  >
                    {l === 'system' ? t('settings.language.follow') : l === 'zh' ? '中文' : 'English'} {/* @i18n-keep 语言名一律用原生书写，切语言时也保持不变 */}
                  </button>
                ))}
              </div>
              <span className="muted small">{t('settings.language.hint')}</span>
            </label>

            <label className="field">
              <span>{t('settings.theme')}</span>
              <div className="seg seg-3">
                {(Object.keys(THEME_LABEL) as unknown as ThemeId[]).map((id) => (
                  <button
                    key={id}
                    type="button"
                    className={`seg-opt${theme.id === id ? ' active' : ''}`}
                    onClick={() => updateTheme({ id, accent: null })}
                  >
                    {themeLabel(id, t)}
                  </button>
                ))}
              </div>
            </label>

            <label className="field">
              <span>{t('settings.themeColor')}</span>
              <div className="swatch-grid">
                <button
                  type="button"
                  className={`swatch auto${theme.accent ? '' : ' active'}`}
                  onClick={() => updateTheme({ accent: null })}
                  title={t('settings.themeAutoTitle')}
                >{t('settings.themeAuto')}</button>
                {SWATCHES[theme.id].map((c) => (
                  <button
                    key={c}
                    type="button"
                    className={`swatch${theme.accent === c ? ' active' : ''}`}
                    style={{ background: c }}
                    onClick={() => updateTheme({ accent: c })}
                    title={c}
                  />
                ))}
                <span className="swatch-color">
                  <input
                    type="color"
                    value={theme.accent ?? '#ff6fa8'}
                    onChange={(e) => updateTheme({ accent: e.target.value })}
                  />
                  <span className="muted small">{t('settings.themeCustom')}</span>
                </span>
              </div>
            </label>

            <label className="field">
              <span>{t('settings.fontSize')}</span>
              <div className="seg seg-4" style={{ marginBottom: 8 }}>
                {[0.85, 1, 1.15, 1.3].map((f) => {
                  const label =
                    f === 0.85 ? t('settings.fontSmall') : f === 1 ? t('settings.fontNormal') : f === 1.15 ? t('settings.fontLarge') : t('settings.fontXlarge');
                  return (
                    <button
                      key={f}
                      type="button"
                      className={`seg-opt${Math.abs(theme.fontScale - f) < 0.03 ? ' active' : ''}`}
                      onClick={() => updateTheme({ fontScale: f })}
                    >
                      {label}
                    </button>
                  );
                })}
              </div>
              <div className="range-row">
                <input
                  type="range"
                  min={0.8}
                  max={1.3}
                  step={0.05}
                  value={theme.fontScale}
                  onChange={(e) => updateTheme({ fontScale: Number(e.target.value) })}
                />
                <span className="range-val">{Math.round(theme.fontScale * 100)}%</span>
              </div>
              <span className="muted small">{t('settings.fontScaleHint')}</span>
            </label>

            <p className="muted small">{t('settings.applyImmediate')}</p>
          </div>
        )}

        {tab === 'ai' && (
          <div className="settings-pane">
            <p className="hint">
              <Info size={12} />
              <span>{t('settings.ai.hint')}</span>
            </p>
            {hw && (
              <>
                <div className="settings-group-title" style={{ marginTop: 16 }}>
                  {t('settings.ai.hwTier', {
                    label: hw.label,
                    source: hw.fromBackend ? t('settings.ai.hwTierBackend') : t('settings.ai.hwTierBrowser'),
                  })}
                </div>
                <div className="field">
                  <span>{t('settings.ai.ollamaAdvice')}</span>
                  {hwRecBanner}
                  {weakMachineWarn}
                </div>
              </>
            )}

            <label className="field">
              <span>{t('settings.ai.baseUrl')}</span>
              <input
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="https://api.openai.com/v1"
              />
            </label>

            {baseUrl.trim() && (
              <div className="provider-detect">
                <span className="badge">
                  {t('settings.ai.detected')}{t(PROVIDER_LABEL[provider])}
                  {providerOverride && t('settings.ai.manualSuffix')}
                </span>
                <button type="button" className="provider-detect-toggle" onClick={() => setShowManual((v) => !v)}>
                  <Settings2 size={11} />
                  {t('settings.ai.manualSpecify')}
                </button>
              </div>
            )}

            {baseUrl.trim() && showManual && (
              <div className="seg seg-5">
                <button
                  type="button"
                  className={`seg-opt${providerOverride === undefined ? ' active' : ''}`}
                  onClick={() => setProviderOverride(undefined)}
                >
                  {t('settings.ai.auto')}
                </button>
                {(Object.keys(PROVIDER_LABEL) as unknown as Provider[]).map((p) => (
                  <button
                    key={p}
                    type="button"
                    className={`seg-opt${providerOverride === p ? ' active' : ''}`}
                    onClick={() => setProviderOverride(p)}
                  >
                    {t(PROVIDER_LABEL_SHORT[p])}
                  </button>
                ))}
              </div>
            )}

            {needsKey && (
              <label className="field">
                <span>{t('settings.ai.apiKey')}</span>
                <div style={{ display: 'flex', gap: 6 }}>
                  <input
                    type={showKey ? 'text' : 'password'}
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    placeholder="sk-..."
                    style={{ flex: 1 }}
                  />
                  <button
                    type="button"
                    className="ghost icon"
                    onClick={() => setShowKey((v) => !v)}
                    title={showKey ? t('settings.ai.hide') : t('settings.ai.show')}
                  >
                    {showKey ? <EyeOff size={14} /> : <Eye size={14} />}
                  </button>
                </div>
              </label>
            )}

            <label className="field">
              <span>{t('settings.ai.model')}</span>
              <input
                value={model}
                onChange={(e) => setModel(e.target.value)}
                placeholder="gpt-4o-mini · deepseek-chat · claude-haiku-4-5 …"
              />
            </label>

            {msg && <div className="ok">{msg}</div>}
            {err && <div className="error">{err}</div>}

            <p className="muted small" style={{ marginTop: 4 }}>
              {t('settings.ai.privacy')}
            </p>

            <div className="modal-actions">
              {saved && <button className="ghost" onClick={wipe}>{t('settings.ai.clear')}</button>}
              <button className="ghost" onClick={testConn} disabled={testing || !baseUrl.trim() || !model.trim()}>
                {testing ? t('settings.ai.testing') : t('settings.ai.test')}
              </button>
              <button className="primary" onClick={save}>{t('settings.ai.save')}</button>
            </div>
          </div>
        )}

        {tab === 'general' && (
          <div className="settings-pane">
            <div className="settings-group-title">AI</div>
            <div className="switch-row">
              <div>
                <div className="switch-label">{t('settings.general.webEnabled')}</div>
                <div className="switch-desc">{t('settings.general.webEnabledDesc')}</div>
              </div>
              <input type="checkbox" className="switch" checked={prefs.webEnabled} onChange={(e) => togglePref('webEnabled', e.target.checked)} />
            </div>

            <div className="switch-row">
              <div>
                <div className="switch-label">{t('settings.general.showThinking')}</div>
                <div className="switch-desc">{t('settings.general.showThinkingDesc')}</div>
              </div>
              <input type="checkbox" className="switch" checked={prefs.showThinking} onChange={(e) => togglePref('showThinking', e.target.checked)} />
            </div>

            <div className="settings-group-title" style={{ marginTop: 18 }}>{t('settings.general.perf')}</div>
            <div className="switch-row">
              <div>
                <div className="switch-label">{t('settings.general.parallelScan')}</div>
                <div className="switch-desc">{t('settings.general.parallelScanDesc')}</div>
              </div>
              <input type="checkbox" className="switch" checked={prefs.parallelScan} onChange={(e) => togglePref('parallelScan', e.target.checked)} />
            </div>

            <div className="switch-row">
              <div>
                <div className="switch-label">{t('settings.general.hardwareAccel')}</div>
                <div className="switch-desc">{t('settings.general.hardwareAccelDesc')}</div>
              </div>
              <input type="checkbox" className="switch" checked={prefs.hardwareAccel} onChange={(e) => togglePref('hardwareAccel', e.target.checked)} />
            </div>

            <label className="field">
              <span>{t('settings.general.keepFiles')}</span>
              <div className="seg seg-4">
                {[100, 300, 500, 1000].map((n) => (
                  <button
                    key={n}
                    type="button"
                    className={`seg-opt${keepFiles === n ? ' active' : ''}`}
                    onClick={() => {
                      setKeepFiles(n);
                      setKeepFilesState(n);
                    }}
                  >
                    {n}
                  </button>
                ))}
              </div>
              <span className="muted small">{t('settings.general.keepFilesHint')}</span>
            </label>

            <div className="settings-group-title" style={{ marginTop: 18 }}>{t('settings.general.update')}</div>
            <div className="update-row">
              <div className="update-main">
                <div className="switch-label">
                  {update === undefined && <>{t('settings.general.updateHint', { ver: isTauri ? t('settings.general.updateVerPending') : '' })}</>}
                  {update === null && <span className="muted">{t('settings.general.updateNone')}</span>}
                  {typeof update === 'string' && <span className="error-inline">{t('settings.general.updateFail', { msg: update })}</span>}
                  {update !== null && typeof update === 'object' && !update.available && (
                    <span><CheckCircle2 size={13} style={{ verticalAlign: '-2px', color: 'var(--accent-strong)', marginRight: 4 }} />{t('settings.general.updateLatest', { ver: update.current })}</span>
                  )}
                  {update !== null && typeof update === 'object' && update.available && (
                    <span><b>{t('settings.general.updateFound', { latest: update.latest, current: update.current })}</b></span>
                  )}
                </div>
                {update !== null && typeof update === 'object' && update.available && update.notes && (
                  <p className="muted small update-notes">{update.notes}</p>
                )}
                {update !== null && typeof update === 'object' && update.available && (
                  <a className="ghost small" href={update.url} target="_blank" rel="noreferrer" style={{ marginTop: 4, display: 'inline-block' }}>
                    {t('settings.general.updateGo')} <ExternalLink size={11} style={{ verticalAlign: '-1px' }} />
                  </a>
                )}
                {update !== null && typeof update === 'object' && update.available && isTauri && (
                  <div className="update-install" style={{ marginTop: 8 }}>
                    {updatePhase === 'installed' ? (
                      <>
                        <button className="primary small" onClick={restartApp}>
                          <RotateCw size={12} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('settings.general.updateRestart')}
                        </button>
                        <span className="muted small">{t('settings.general.updateDownloaded')}</span>
                      </>
                    ) : updatePhase === 'downloading' ? (
                      <div className="update-progress">
                        <div className="update-progress-bar" style={{ width: `${updateProgress}%` }} />
                        <span className="muted small">{t('settings.general.updateInstalling', { pct: updateProgress })}</span>
                      </div>
                    ) : (
                      <button
                        className="primary small"
                        onClick={() => { setUpdatePhase('confirm'); }}
                        title={t('settings.general.updateInstallTitle')}
                      >
                        <Download size={12} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('settings.general.updateInstall')}
                      </button>
                    )}
                    {typeof updatePhase === 'string' && updatePhase !== 'confirm' && updatePhase !== 'installed' && (
                      <span className="error-inline small" style={{ display: 'block', marginTop: 4 }}>{updatePhase}</span>
                    )}
                  </div>
                )}
              </div>
              <button className="ghost small" onClick={checkUpdate} disabled={checkingUpdate || !isTauri} title={isTauri ? t('settings.general.updateCheckTitle') : t('settings.general.updateDesktopOnly')}>
                <RefreshCw size={12} className={checkingUpdate ? 'spin' : undefined} /> {checkingUpdate ? t('settings.general.updateChecking') : t('settings.general.updateCheck')}
              </button>
            </div>

            {update !== null && typeof update === 'object' && update.available && updatePhase === 'confirm' && (
              <ConfirmDialog
                title={t('settings.general.updateConfirmTitle', { latest: update.latest })}
                body={t('settings.general.updateConfirmBody', { latest: update.latest, current: update.current })}
                confirmLabel={t('settings.general.updateInstall')}
                onConfirm={confirmAndInstall}
                onCancel={() => setUpdatePhase(null)}
              />
            )}

            <p className="muted small" style={{ marginTop: 10 }}>{t('settings.general.aiInactiveHint')}</p>
            <p className="muted small" style={{ marginTop: 4 }}>{t('settings.general.aiNotSetupWorks')}</p>
          </div>
        )}

        {tab === 'perms' && (
          <div className="settings-pane">
            <p className="hint">
              <ShieldCheck size={12} />
              <span>
                {t('settings.perms.hint')}
              </span>
            </p>
            <PermissionCenter />
            <p className="muted small" style={{ marginTop: 10 }}>
              {t('settings.perms.foot')}
            </p>
          </div>
        )}

        {tab === 'system' && <SystemTools />}

        {tab === 'mcp' && <McpServers />}

        {tab === 'market' && (
          <div className="settings-pane">
            <MarketPanel />
          </div>
        )}

        {tab === 'reminder' && <ReminderSettings />}

        <div className="modal-actions" style={{ marginTop: 14 }}>
          <button className="ghost" onClick={onClose}>{t('settings.close')}</button>
        </div>
      </div>
    </div>
  );
}
