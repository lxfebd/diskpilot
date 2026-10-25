import { useEffect, useReducer, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import { getHwProfile, recommendLocal, type HwProfile } from '../hwProfile';
import { X, CheckCircle2, Info, Eye, EyeOff, Settings2, Palette, Sparkles, SlidersHorizontal, ShieldCheck, RefreshCw, ExternalLink, Download, RotateCw, Wrench, Plug, Package, Bell, FolderOpen } from 'lucide-react';
import { api } from '../api';
import { isTauri } from '../env';
import { ConfirmDialog } from './ConfirmDialog';
import { useEscapeKey } from '../escapeKey';
import { safeGetItem, safeRemoveItem, safeSetItem } from '../safeStorage';
import { cycleIndex } from '../a11y';
import { useReturnFocus } from '../dialogFocus';
import { openDataDirWithToast } from '../reveal';
import { openExternal } from '../external';
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
  normalizeSettings,
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
import {
  UPDATE_SKIP_KEY,
  formatSkipRecord,
  parseSkipRecord,
  shouldPromptUpdate,
} from '../updateSkip';

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

// R365：8 个分栏按 WAI-ARIA tabs 模式接线——读屏能念出「当前是哪一栏、共几栏」，
// 方向键在分栏之间移动（Home/End 跳首尾），Tab 键只在当前栏进一次。
// 此前只是 8 个带 active 类的普通按钮：选中态对读屏不可见，键盘要 Tab 八次。
const SETTINGS_TABS: { id: SettingsTab; labelKey: string; icon: typeof Palette }[] = [
  { id: 'appearance', labelKey: 'settings.appearance', icon: Palette },
  { id: 'ai', labelKey: 'settings.ai', icon: Sparkles },
  { id: 'general', labelKey: 'settings.general', icon: SlidersHorizontal },
  { id: 'perms', labelKey: 'settings.perms', icon: ShieldCheck },
  { id: 'system', labelKey: 'settings.system', icon: Wrench },
  { id: 'mcp', labelKey: 'settings.mcp', icon: Plug },
  { id: 'market', labelKey: 'settings.market', icon: Package },
  { id: 'reminder', labelKey: 'settings.reminder', icon: Bell },
];

/** R352：AI 配置的草稿——改完点「保存」才落盘，点 X / 按 Esc 关窗就会丢。 */
export interface AiDraft {
  baseUrl: string;
  apiKey: string;
  model: string;
  providerOverride?: Provider;
}

/** R352：草稿与已保存的配置是否不同（未载入配置时以全空为基线）。 */
export function aiDraftDirty(draft: AiDraft, base: AiDraft | null): boolean {
  const b: AiDraft = base ?? { baseUrl: '', apiKey: '', model: '', providerOverride: undefined };
  return (
    draft.baseUrl !== b.baseUrl ||
    draft.apiKey !== b.apiKey ||
    draft.model !== b.model ||
    draft.providerOverride !== b.providerOverride
  );
}

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

  // R365：分栏之间的键盘移动。只换选中项并把焦点带过去（roving tabindex），
  // 不做「焦点进但内容不换」的半套行为。
  const onTabKey = (e: ReactKeyboardEvent<HTMLButtonElement>, idx: number) => {
    const len = SETTINGS_TABS.length;
    let next = -1;
    if (e.key === 'ArrowRight') next = cycleIndex(idx, len, 1);
    else if (e.key === 'ArrowLeft') next = cycleIndex(idx, len, -1);
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = len - 1;
    if (next < 0) return;
    e.preventDefault();
    const id = SETTINGS_TABS[next].id;
    setTab(id);
    document.getElementById(`settings-tab-${id}`)?.focus();
  };
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
  // R352：已保存的配置快照（判断草稿有没有改动；关窗会丢，得先问一句）。
  const [baseline, setBaseline] = useState<AiDraft | null>(null);
  const [askDiscard, setAskDiscard] = useState(false);

  // ── Appearance tab state ──
  const [theme, setTheme] = useState(loadTheme());

  // ── General tab state ──
  // 通用开关以 localStorage（prefs getter）为唯一数据源：渲染时直接读 getter，
  // 切换后 bump 一次版本触发重渲染，避免组件内再存一份状态导致双源漂移。
  const [, bumpPrefs] = useReducer((v: number) => v + 1, 0);
  const [keepFiles, setKeepFilesState] = useState(getKeepFiles());
  // 后端 general.json 权威开关（hardwareAccel/closeToTray）读取状态：
  // null=未查，'ok'=已同步，'err'=读失败（开关按本地默认渲染，但必须告知
  // 「未读到后端当前状态」，避免把默认值当真相）。
  const [backendPrefState, setBackendPrefState] = useState<null | 'ok' | 'err'>(null);
  // 本机性能档位：决定本地 Ollama 是否吃得消、该建议云端还是本地。
  const [hw, setHw] = useState<HwProfile | null>(null);
  // 检查更新：null = 未查过；字符串 = 出错信息；对象 = 结果。
  const [update, setUpdate] = useState<import('../api').UpdateInfo | null | string | undefined>(undefined);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  // 一键更新：null = 未下载；'confirm' = 待两步确认；'downloading' = 下载中；'installed' = 装完待重启。
  const [updatePhase, setUpdatePhase] = useState<null | 'confirm' | 'downloading' | 'installed'>(null);
  // 更新链路错误单独存放：与 updatePhase 互斥，避免字符串错误被等于 'confirm'/'installed' 等状态值吞掉。
  const [updateErr, setUpdateErr] = useState<string | null>(null);
  const [updateProgress, setUpdateProgress] = useState(0);
  // R158：已跳过的版本（localStorage 持久化）——用户明确不想装的版本不再
  // 反复提示；出现更新的版本时重新提示（跳过是版本相关的，不是永久静音）。
  // R161：记录里带跳过时刻，超过 UPDATE_SKIP_DAYS 天同样重新提示（跳过
  // 不该是永久静音；旧格式纯版本串仍能读）。
  const [skipRec, setSkipRec] = useState(() => parseSkipRecord(safeGetItem(UPDATE_SKIP_KEY)));
  const skippedVer = skipRec?.version ?? null;

  const skipThisVersion = (ver: string) => {
    const rec = { version: ver, atMs: Date.now() };
    safeSetItem(UPDATE_SKIP_KEY, formatSkipRecord(rec.version, rec.atMs));
    setSkipRec(rec);
  };

  const unskipVersion = () => {
    safeRemoveItem(UPDATE_SKIP_KEY);
    setSkipRec(null);
  };

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
    setUpdateErr(null);
    setUpdateProgress(0);
    let total = 0;
    let downloaded = 0;
    try {
      await u.downloadAndInstall((event) => {
        if (event.event === 'Started' && event.data.contentLength != null) {
          total = event.data.contentLength;
        } else if (event.event === 'Progress') {
          downloaded += event.data.chunkLength;
          // R463：进度钳在 100 以内——内容长度与实际下载量不一致时，aria-valuenow 不会超过 aria-valuemax。
          if (total > 0) setUpdateProgress(Math.min(100, Math.round((downloaded / total) * 100)));
        }
      });
      setUpdatePhase('installed');
    } catch (e) {
      // R463：下载失败回到待装态——错误照样显示，安装按钮跟着回来，不必重启应用才能重试。
      setUpdatePhase(null);
      setUpdateErr(String(e instanceof Error ? e.message : e));
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
      setUpdateErr(String(e instanceof Error ? e.message : e));
    }
  };

  const restartApp = async () => {
    try {
      await relaunch();
    } catch (e) {
      setUpdateErr(String(e instanceof Error ? e.message : e));
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
      setBaseline({
        baseUrl: existing.baseUrl,
        apiKey: existing.apiKey,
        model: existing.model,
        providerOverride: existing.providerOverride,
      });
    }
  }, []);

  // 读取本机性能档位（CPU 核数/内存），用于给 AI 配置建议「用云端还是本地」。
  // R417：设置弹层关得快时迟到的响应不落地到已卸载组件。
  useEffect(() => {
    let alive = true;
    getHwProfile().then((p) => { if (alive) setHw(p); }).catch(() => { if (alive) setHw(null); });
    return () => { alive = false; };
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

  // 硬件加速 / 托盘常驻开关以后端 general.json 为权威（硬件加速决定 WebView2
  // 是否 --disable-gpu，托盘决定点关闭按钮的行为），打开设置页时同步一次。
  useEffect(() => {
    if (!isTauri) return;
    api.generalConfig()
      .then((c) => {
        setPref('hardwareAccel', c.hardware_accel);
        setPref('closeToTray', c.close_to_tray);
        setBackendPrefState('ok');
        bumpPrefs();
      })
      .catch(() => { setBackendPrefState('err'); });
  }, []);

  const provider = providerOverride ?? detectProvider(baseUrl);
  const needsKey = provider !== 'ollama';

  // R352：AI 配置改了没点「保存」就关窗，此前会静默丢掉——先给提示，关窗时再问一句。
  const aiDirty = aiDraftDirty({ baseUrl, apiKey, model, providerOverride }, baseline);
  const requestClose = () => {
    if (aiDirty) setAskDiscard(true);
    else onClose();
  };
  useEscapeKey(requestClose);
  // R355：设置窗也是「关掉要把焦点还回去」的一层——此前关掉后焦点落回 body，
  // 键盘用户要从页面开头重新 Tab 一遍。
  useReturnFocus(true);
  const dlgRef = useRef<HTMLDivElement | null>(null);
  // 打开时把焦点移到对话框本体（role=dialog 自带 aria-label）：读屏先念出「设置」标题，
  // 再 Tab 才进第一项——而不是从 body 开始找。
  useEffect(() => {
    dlgRef.current?.focus();
  }, []);

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

  // 打开应用数据目录（R385/R386）：撤销台账/隔离区/提醒/硬件历史都在这，备份或手动清理要能一键到。
  const openDataDir = () => { void openDataDirWithToast(); };

  const save = async () => {
    setErr(null); setMsg(null);
    if (!baseUrl.trim()) { setErr(t('settings.ai.errBaseUrl')); return; }
    if (!model.trim())   { setErr(t('settings.ai.errModel')); return; }
    if (needsKey && !apiKey.trim()) { setErr(t('settings.ai.errKey')); return; }
    try {
      // R437：粘贴带空白时先归一化——输入框跟着变成 canonical，所见即所存。
      const n = normalizeSettings({ provider, model, apiKey, baseUrl, providerOverride });
      saveSettings(n);
      setBaseline({ baseUrl: n.baseUrl, apiKey: n.apiKey, model: n.model, providerOverride: n.providerOverride });
      setBaseUrl(n.baseUrl);
      setModel(n.model);
      setApiKey(n.apiKey);
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
    setBaseline({ baseUrl: '', apiKey: '', model: '', providerOverride: undefined });
    setShowManual(false);
    setSaved(false);
    setMsg(t('settings.ai.wiped'));
  };

  const testConn = async () => {
    setErr(null); setMsg(null); setTesting(true);
    try {
      // R437：同保存——试连走的是 loadSettings 的读路径，落盘前先归一化。
      const n = normalizeSettings({ provider, model, apiKey, baseUrl, providerOverride });
      saveSettings(n);
      // 试连也把配置落了盘，所以草稿不再算「未保存」。
      setBaseline({ baseUrl: n.baseUrl, apiKey: n.apiKey, model: n.model, providerOverride: n.providerOverride });
      setBaseUrl(n.baseUrl);
      setModel(n.model);
      setApiKey(n.apiKey);
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
    // 硬件加速/托盘这两项的后端写失败不应静默：UI 已按新值渲染，
    // 若后端没接住会脱节，必须把失败亮给用户。
    const backendWrite = (p: Promise<unknown>, label: string) => {
      p.catch((e) => {
        setErr(`${label}: ${String(e instanceof Error ? e.message : e)}`);
      });
    };
    if (name === 'hardwareAccel') {
      // 硬件加速由后端 Rust 启动时读 general.json 决定，重启后生效。
      backendWrite(api.setGeneral(on), t('settings.general.hardwareAccel'));
    }
    if (name === 'closeToTray') {
      // 托盘常驻：写后端 general.json，窗口关闭按钮即刻按新值执行。
      backendWrite(api.setCloseToTray(on), t('settings.general.closeToTray'));
    }
  };

  return (
    <>
      {/* R352：有未保存的 AI 改动时关窗先问一句——直接关掉等于把用户刚敲的配置扔了。 */}
      {askDiscard && (
        <ConfirmDialog
          title={t('settings.discardTitle')}
          body={t('settings.discardBody')}
          confirmLabel={t('settings.discardConfirm')}
          cancelLabel={t('settings.discardCancel')}
          onConfirm={() => { setAskDiscard(false); onClose(); }}
          onCancel={() => setAskDiscard(false)}
        />
      )}
    <div ref={dlgRef} tabIndex={-1} className="modal-bg" role="dialog" aria-modal="true" aria-label={t('settings.title')} onClick={requestClose}>
      <div className="modal settings-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>{t('settings.title')} {saved && <CheckCircle2 size={16} style={{ verticalAlign: 'middle', marginLeft: 6, color: 'var(--accent-strong)' }} />}</div>
          <button className="ghost icon" onClick={requestClose} title={t('settings.close')} aria-label={t('settings.close')}><X size={16} /></button>
        </div>

        <div className="settings-tabs" role="tablist" aria-label={t('settings.tabsAria')}>
          {SETTINGS_TABS.map((tb, idx) => {
            const Icon = tb.icon;
            const on = tab === tb.id;
            return (
              <button
                key={tb.id}
                id={`settings-tab-${tb.id}`}
                role="tab"
                aria-selected={on}
                aria-controls={`settings-pane-${tb.id}`}
                tabIndex={on ? 0 : -1}
                className={`settings-tab${on ? ' active' : ''}`}
                onClick={() => setTab(tb.id)}
                onKeyDown={(e) => onTabKey(e, idx)}
              >
                <Icon size={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t(tb.labelKey)}
              </button>
            );
          })}
        </div>

        {tab === 'appearance' && (
          <div className="settings-pane" role="tabpanel" id="settings-pane-appearance" aria-labelledby="settings-tab-appearance">
            <label className="field">
              <span>{t('settings.language')}</span>
              <div className="seg seg-3">
                {(['system', 'zh', 'en'] as const).map((l) => (
                  <button
                    key={l}
                    type="button"
                    className={`seg-opt${lang === l ? ' active' : ''}`} aria-pressed={lang === l}
                    onClick={() => {
                      setLang(l);
                      setLangState(l);
                      // 托盘菜单文案随语言重推（后端菜单项 id 固定只换 text）。
                      api.traySync(
                        t('settings.general.trayShow'),
                        t('settings.general.trayQuit'),
                        t('settings.general.trayTooltip'),
                      ).catch(() => {});
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
              {/* 17 套主题用 3 列分段控件会溢出截断（长中文名被省略号吃掉）；
                 改用自适应 flex 换行块，每个主题完整可读、可独立点击 */}
              <div className="seg seg-4 auto theme-pick">
                {(Object.keys(THEME_LABEL) as unknown as ThemeId[]).map((id) => (
                  <button
                    key={id}
                    type="button"
                    className={`seg-opt${theme.id === id ? ' active' : ''}`} aria-pressed={theme.id === id}
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
                  className={`swatch auto${theme.accent ? '' : ' active'}`} aria-pressed={!theme.accent}
                  onClick={() => updateTheme({ accent: null })}
                  title={t('settings.themeAutoTitle')}
                >{t('settings.themeAuto')}</button>
                {SWATCHES[theme.id].map((c) => (
                  <button
                    key={c}
                    type="button"
                    className={`swatch${theme.accent === c ? ' active' : ''}`} aria-pressed={theme.accent === c}
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
                      className={`seg-opt${Math.abs(theme.fontScale - f) < 0.03 ? ' active' : ''}`} aria-pressed={Math.abs(theme.fontScale - f) < 0.03}
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
          <div className="settings-pane" role="tabpanel" id="settings-pane-ai" aria-labelledby="settings-tab-ai">
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
                autoComplete="off"
                spellCheck={false}
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
                  className={`seg-opt${providerOverride === undefined ? ' active' : ''}`} aria-pressed={providerOverride === undefined}
                  onClick={() => setProviderOverride(undefined)}
                >
                  {t('settings.ai.auto')}
                </button>
                {(Object.keys(PROVIDER_LABEL) as unknown as Provider[]).map((p) => (
                  <button
                    key={p}
                    type="button"
                    className={`seg-opt${providerOverride === p ? ' active' : ''}`} aria-pressed={providerOverride === p}
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
                  {/* R413：API key 不是登录口令——autoComplete="new-password" 防止 WebView2 的
                      密码管理器把它当站点密码跨站保存、并在别的表单里自动填充。 */}
                  <input
                    type={showKey ? 'text' : 'password'}
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    placeholder="sk-..."
                    style={{ flex: 1 }}
                    autoComplete="new-password"
                    spellCheck={false}
                    autoCapitalize="off"
                    autoCorrect="off"
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
                autoComplete="off"
                spellCheck={false}
              />
            </label>

            {msg && <div className="ok" role="status">{msg}</div>}
            {err && <div className="error" role="alert">{err}</div>}

            <p className="muted small" style={{ marginTop: 4 }}>
              {t('settings.ai.privacy')}
            </p>

            {aiDirty && (
              <p className="muted small" style={{ marginTop: 8 }}>{t('settings.ai.unsaved')}</p>
            )}

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
          <div className="settings-pane" role="tabpanel" id="settings-pane-general" aria-labelledby="settings-tab-general">
            <div className="settings-group-title">{t('settings.general.ai')}</div>
            <div className="switch-row">
              <div>
                <div className="switch-label">{t('settings.general.webEnabled')}</div>
                <div className="switch-desc">{t('settings.general.webEnabledDesc')}</div>
              </div>
              <input type="checkbox" className="switch" aria-label={t('settings.general.webEnabled')} checked={prefs.webEnabled} onChange={(e) => togglePref('webEnabled', e.target.checked)} />
            </div>

            <div className="switch-row">
              <div>
                <div className="switch-label">{t('settings.general.showThinking')}</div>
                <div className="switch-desc">{t('settings.general.showThinkingDesc')}</div>
              </div>
              <input type="checkbox" className="switch" aria-label={t('settings.general.showThinking')} checked={prefs.showThinking} onChange={(e) => togglePref('showThinking', e.target.checked)} />
            </div>

            <div className="settings-group-title" style={{ marginTop: 18 }}>{t('settings.general.perf')}</div>
            <div className="switch-row">
              <div>
                <div className="switch-label">{t('settings.general.parallelScan')}</div>
                <div className="switch-desc">{t('settings.general.parallelScanDesc')}</div>
              </div>
              <input type="checkbox" className="switch" aria-label={t('settings.general.parallelScan')} checked={prefs.parallelScan} onChange={(e) => togglePref('parallelScan', e.target.checked)} />
            </div>

            <div className="switch-row">
              <div>
                <div className="switch-label">{t('settings.general.hardwareAccel')}</div>
                <div className="switch-desc">{t('settings.general.hardwareAccelDesc')}</div>
              </div>
              <input type="checkbox" className="switch" aria-label={t('settings.general.hardwareAccel')} checked={prefs.hardwareAccel} onChange={(e) => togglePref('hardwareAccel', e.target.checked)} />
            </div>

            <div className="switch-row">
              <div>
                <div className="switch-label">{t('settings.general.closeToTray')}</div>
                <div className="switch-desc">{t('settings.general.closeToTrayDesc')}</div>
              </div>
              <input type="checkbox" className="switch" aria-label={t('settings.general.closeToTray')} checked={prefs.closeToTray} onChange={(e) => togglePref('closeToTray', e.target.checked)} />
            </div>

            {backendPrefState === 'err' && (
              <div className="hint" style={{ marginBottom: 8 }}>
                <span className="muted small">{t('settings.general.backendUnread')}</span>
              </div>
            )}

            <label className="field">
              <span>{t('settings.general.keepFiles')}</span>
              <div className="seg seg-4">
                {[100, 300, 500, 1000].map((n) => (
                  <button
                    key={n}
                    type="button"
                    className={`seg-opt${keepFiles === n ? ' active' : ''}`} aria-pressed={keepFiles === n}
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
                  {update !== null && typeof update === 'object' && update.available && shouldPromptUpdate(skippedVer, update.latest, skipRec?.atMs ?? 0) && (
                    <span><b>{t('settings.general.updateFound', { latest: update.latest, current: update.current })}</b></span>
                  )}
                  {/* R158：用户已跳过这个版本——如实说清「有新版本但你选了跳过」，
                      给恢复入口（不是静默不提示）。 */}
                  {update !== null && typeof update === 'object' && update.available && !shouldPromptUpdate(skippedVer, update.latest, skipRec?.atMs ?? 0) && (
                    <span className="muted small">
                      {t('settings.general.updateSkipped', { latest: update.latest })}
                      <button type="button" className="ghost small" style={{ marginLeft: 6 }} onClick={unskipVersion}>
                        {t('settings.general.updateUnskip')}
                      </button>
                    </span>
                  )}
                </div>
                {update !== null && typeof update === 'object' && update.available && update.notes && (
                  <p className="muted small update-notes">{update.notes}</p>
                )}
                {update !== null && typeof update === 'object' && update.available && (
                  <a className="ghost small" href={update.url} onClick={(e) => { e.preventDefault(); void openExternal(update.url); }} style={{ marginTop: 4, display: 'inline-block' }}>
                    {t('settings.general.updateGo')} <ExternalLink size={11} style={{ verticalAlign: '-1px' }} />
                  </a>
                )}
{update !== null && typeof update === 'object' && update.available && shouldPromptUpdate(skippedVer, update.latest, skipRec?.atMs ?? 0) && (
                  <button
                    type="button"
                    className="ghost small"
                    style={{ marginTop: 4, display: 'inline-block' }}
                    onClick={() => skipThisVersion(update.latest)}
                    title={t('settings.general.updateSkipTitle', { latest: update.latest })}
                  >
                    {t('settings.general.updateSkip')}
                  </button>
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
                        <div className="update-progress-bar" role="progressbar" aria-label={t('settings.general.updateAria')} aria-valuemin={0} aria-valuemax={100} aria-valuenow={updateProgress} style={{ width: `${updateProgress}%` }} />
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
                    {typeof updateErr === 'string' && updateErr && (
                      <span className="error-inline small" style={{ display: 'block', marginTop: 4 }}>{updateErr}</span>
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

            <div className="settings-group-title" style={{ marginTop: 18 }}>{t('settings.general.dataDir')}</div>
            <p className="muted small" style={{ marginTop: 4 }}>{t('settings.general.dataDirDesc')}</p>
            <button type="button" className="ghost small" style={{ marginTop: 6 }} onClick={() => { void openDataDir(); }}>
              <FolderOpen size={12} style={{ verticalAlign: '-2px', marginRight: 4 }} />{t('settings.general.dataDirOpen')}
            </button>

            <p className="muted small" style={{ marginTop: 10 }}>{t('settings.general.aiInactiveHint')}</p>
            <p className="muted small" style={{ marginTop: 4 }}>{t('settings.general.aiNotSetupWorks')}</p>
          </div>
        )}

        {tab === 'perms' && (
          <div className="settings-pane" role="tabpanel" id="settings-pane-perms" aria-labelledby="settings-tab-perms">
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
          <div className="settings-pane" role="tabpanel" id="settings-pane-market" aria-labelledby="settings-tab-market">
            <MarketPanel />
          </div>
        )}

        {tab === 'reminder' && <ReminderSettings />}

        <div className="modal-actions" style={{ marginTop: 14 }}>
          <button className="ghost" onClick={requestClose}>{t('settings.close')}</button>
        </div>
      </div>
    </div>
    </>
  );
}
