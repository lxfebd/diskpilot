import { forwardRef, lazy, Suspense, useEffect, useImperativeHandle, useMemo, useRef, useState } from 'react';
import { Folder, ScanLine, ListTree, MessageSquare, X, Trash2 as TrashIcon } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { api } from './api';
import { isTauri } from './env';
import { useStore } from './store';
import { LeftPanel } from './components/LeftPanel';
import { useCleanupStore } from './useCleanupStore';
import { scanTargets } from './drives';
import { ScanGate } from './scanGate';
import { DriveStrip } from './components/DriveStrip';
import { Splitter } from './components/Splitter';
import { PageHeader } from './components/PageHeader';
import { Sidebar } from './components/Sidebar';
import { ErrorBoundary } from './components/ErrorBoundary';
import { CleanupProposalDialog } from './components/CleanupProposalDialog';
import { ConfirmDialog } from './components/ConfirmDialog';
import Disclaimer from './components/Disclaimer';
import { formatBytes, ellipsizePath, normKey, driveRootOf } from './format';
import { loadSettings, isConfigured } from './advisorClient';
import { useModalFocusTrap } from './dialogFocus';
import { prefs } from './theme';
import { safeGetItem, safeSetItem } from './safeStorage';
import { AI_WIDTH_MIN, clampAiWidth } from './panelWidths';
import { APP_VERSION } from './version';
import { useT } from './i18n';
import {
  PAGE_IDS,
  SIDEBAR_COLLAPSED_WIDTH_PX,
  SIDEBAR_EXPANDED_WIDTH_PX,
  createSidebarLayoutState,
  navItemById,
  pageTitle,
  resizeSidebarLayout,
  sidebarFootprintAt,
  toggleSidebarLayout,
  type PageId,
} from './nav';
import type { Node } from './types';

// ── 页面级代码分割：默认页 overview 首屏只拉自己 + arena 的 chunk ──
// 其余页面（工具集/清理/优化器/历史/AI 侧栏/设置）按需加载。这些组件都是
// 命名导出，lazy() 需要包一层转 default。
const ChatPanel = lazy(() => import('./components/ChatPanel').then((m) => ({ default: m.ChatPanel })));
const Studio = lazy(() => import('./components/Studio').then((m) => ({ default: m.Studio })));
const Settings = lazy(() => import('./components/Settings').then((m) => ({ default: m.Settings })));
const AssetOverview = lazy(() => import('./components/AssetOverview').then((m) => ({ default: m.AssetOverview })));
const Toolbelt = lazy(() => import('./components/Toolbelt').then((m) => ({ default: m.Toolbelt })));
const CleanupPage = lazy(() => import('./components/cleanup/CleanupPage').then((m) => ({ default: m.CleanupPage })));
const HistoryPage = lazy(() => import('./components/history/HistoryPage').then((m) => ({ default: m.HistoryPage })));
const OptimizerPage = lazy(() => import('./components/OptimizerPage').then((m) => ({ default: m.OptimizerPage })));

// lazy chunk 加载完成前的占位：保持骨架布局不塌（高度继承父容器）。
function PageFallback() {
  return <div className="page-fallback" />;
}

interface ScanStatsEvent {
  mode: string;
  mft_attempted: boolean;
  mft_succeeded: boolean;
  mft_ms: number;
  walk_ms: number;
  build_tree_ms: number;
  scanner_total_ms: number;
  tag_ms: number;             // post-scan walk: detect_compiled + truncation
  cmd_total_ms: number;
  files_seen: number;
  bytes_seen: number;
  dirs_in_acc: number;
}

interface ScanDiag {
  backend: ScanStatsEvent | null;
  ipcMs: number | null;       // null when backend stats event didn't arrive
  scanCallMs: number;         // total api.scan() round-trip
  setRootMs: number;          // setRoot+select sync work
  totalMs: number;            // entire scan() handler
}

interface DriveInfo {
  path: string;
  total_bytes: number;
  used_bytes: number;
  free_bytes: number;
  /** 映射盘（subst）的目标路径，普通盘为空。 */
  mapped_to?: string;
}

// ScanBar 暴露给 App 的命令接口：扫描开始（目标列表）+ 盘根总量探针。
export interface ScanBarHandle {
  begin: (targets: string[]) => void;
  probeTotals: (targets: string[]) => void;
  // R465：本轮扫描被中断（取消/失败）——进度条不许再冒充「完成」态。
  interrupt: (kind: 'cancelled' | 'failed') => void;
}

const DEFAULT_LEFT = 620;
const DEFAULT_RIGHT = 320;
const MIN_LEFT = 320;
const MIN_RIGHT = 220;
const MIN_CENTER = 360;
// 右栏（Studio）是辅助栏：内容区不足三栏预算时自动隐藏，保住左栏+中栏。
const THREE_COL_BUDGET = MIN_LEFT + MIN_CENTER + MIN_RIGHT;

export default function App() {
  const t = useT();
  const root = useStore((s) => s.root);
  const setRoot = useStore((s) => s.setRoot);
  const setScaffolds = useStore((s) => s.setScaffolds);
  const scaffolds = useStore((s) => s.scaffolds);
  const selectedPath = useStore((s) => s.selectedPath);
  const select = useStore((s) => s.selectPath);
  const reclaimedBytes = useStore((s) => s.reclaimedBytes);
  const toasts = useStore((s) => s.toasts);
  const dismissToast = useStore((s) => s.dismissToast);

  // R347：aria-modal 只是说给读屏软件的承诺，Tab 仍会走到背景页——这里全局接管一次。
  useModalFocusTrap();

  const [scanning, setScanning] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // 扫描进度条独立子组件自持高频状态（scan-progress 事件每 tick 都更新），
  // App 只保留低频 scanning。用 ref 把「扫描开始/目标列表/总量探针」下发给
  // ScanBar，避免进度更新穿透到 App 整树重渲染。
  const scanBarRef = useRef<ScanBarHandle>(null);
  // R383：全应用只留一个扫描闸门。扫描入口有五处（盘卡片、扫描全部、体检红盘格、
  // 救援横幅、页脚重扫），并发起两份会双倍读盘、进度事件互相覆盖，先结束的那份
  // 还会把 scanning 提前置回 false。闸门在真正扫描前才取，缓存秒开路径不受影响。
  const scanGate = useRef(new ScanGate()).current;
  const [pickedPath, setPickedPath] = useState<string>('');
  const [showSettings, setShowSettings] = useState(false);
  const [settingsTab, setSettingsTab] = useState<import('./components/Settings').SettingsTab | undefined>(undefined);
  const [advisorTag, setAdvisorTag] = useState<{ provider: string } | null>(null);
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  // 页面分发：设置不在其中——它是全局模态（点导航项只开弹窗，不切页）。
  const [view, setView] = useState<Exclude<PageId, 'settings'>>(PAGE_IDS.overview);
  // R354：窗口标题 = 「DiskPilot · 当前页面」。任务栏、窗口列表、读屏软件都读它，
  // 此前切到清理/历史/工具箱后标题始终是「DiskPilot」，看不出在哪一页。
  useEffect(() => {
    document.title = pageTitle(view, t);
  }, [t, view]);

  // R363：切页后把焦点送进新页面内容区的主元素——键盘/读屏用户点完侧栏，
  // 焦点还留在侧栏按钮上，新页面内容没人接（读屏也听不到「换页了」）。
  // 首屏不抢焦点：用户还没操作过，先停在文档起点。
  const mainRef = useRef<HTMLElement | null>(null);
  const firstView = useRef(true);
  useEffect(() => {
    if (firstView.current) { firstView.current = false; return; }
    mainRef.current?.focus();
  }, [view]);
  const [diag, setDiag] = useState<ScanDiag | null>(null);
  // Holds the latest scan-stats event so we can merge it into ScanDiag once
  // api.scan() returns. Tauri emits the event right before the command resolves.
  const lastBackendStats = useRef(null as ScanStatsEvent | null);
  // setRoot(rootNode)+select 的同步耗时：scan 完成后前端要把整棵树塞进 store，
  // 大磁盘时这一步可能比后端 IO 还慢，单独计时喂给 diag（曾恒 0，诊断失真）。
  const lastSetRootMs = useRef(0);

  // ── 全局 AI 侧栏（贯穿所有页面）──
  const [aiOpen, setAiOpen] = useState<boolean>(() => safeGetItem('diskpilot.aiOpen') !== '0');
  // R408：初始化也要夹上界——大屏记住的宽值不能把窄窗的中栏挤爆；
  // 侧栏占位按当前视口的展开态算（窄窗自动折叠，与 sidebar 初始态同口径）。
  const [aiWidth, setAiWidth] = useState<number>(() =>
    clampAiWidth(
      Number(safeGetItem('diskpilot.aiWidth')),
      window.innerWidth,
      sidebarFootprintAt(window.innerWidth, safeGetItem('diskpilot.sidebarExpanded') !== '0'),
    ),
  );
  useEffect(() => { safeSetItem('diskpilot.aiOpen', aiOpen ? '1' : '0'); }, [aiOpen]);
  useEffect(() => { safeSetItem('diskpilot.aiWidth', String(aiWidth)); }, [aiWidth]);

  // 关窗/刷新前归档活动对话：活动 turns 平时不逐条落盘，不归档会在
  // 关闭后整段丢失。用 getState 取最新状态，避免 effect 闭包过期。
  useEffect(() => {
    const onUnload = () => useStore.getState().archiveActiveChat();
    window.addEventListener('beforeunload', onUnload);
    return () => window.removeEventListener('beforeunload', onUnload);
  }, []);

  // ── 导航侧栏折叠三态（响应式 + 用户显式选择，见 nav.ts）──
  // 显式收起/展开跨会话记住（与 AI 栏 aiOpen 同款存储）；缺省展开。
  const [sidebar, setSidebar] = useState(() =>
    createSidebarLayoutState(window.innerWidth, safeGetItem('diskpilot.sidebarExpanded') !== '0'),
  );
  useEffect(() => {
    const onResize = () => setSidebar((s) => resizeSidebarLayout(s, window.innerWidth));
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  useEffect(() => {
    safeSetItem('diskpilot.sidebarExpanded', sidebar.preferredExpanded ? '1' : '0');
  }, [sidebar.preferredExpanded]);
  const toggleSidebar = () => setSidebar(toggleSidebarLayout);
  // 侧栏占的横向宽度（含 1px 右边框）：下面所有内容区宽度预算都要扣掉它，
  // 否则「侧栏展开 + AI 栏打开」时三栏工作台会溢出被裁。
  const sidebarFootprint = (sidebar.expanded ? SIDEBAR_EXPANDED_WIDTH_PX : SIDEBAR_COLLAPSED_WIDTH_PX) + 1;
  // 侧栏导航：设置走模态入口，其余切页。
  const navigate = (id: PageId) => {
    if (id === PAGE_IDS.settings) { setShowSettings(true); return; }
    setView(id);
  };
  // 总览/Studio 跳清理页：可选传 scaffoldId 预选脚本，不传则进页自动挑第一个命中。
  const openCleanup = (scaffoldId?: string) => {
    useCleanupStore.getState().select(scaffoldId);
    setView(PAGE_IDS.cleanup);
  };
  // Studio「最近清理」→ 操作历史整页（P2c）。
  const openHistory = () => setView(PAGE_IDS.history);

  // AI 侧栏拖宽
  const dragAi = (dx: number) => {
    // 主内容区至少留 480px（AI_MAX_BUDGET，含最小左栏 320 + 中栏 360 的预算内），
    // 上界口径收进 clampAiWidth（R408），此处不再手写 Math.max(260,...)。
    setAiWidth((w) => clampAiWidth(w + dx, window.innerWidth, sidebarFootprint));
  };

  const refreshAdvisorTag = () => {
    const s = loadSettings();
    setAdvisorTag(isConfigured(s) ? { provider: s.provider } : null);
  };
  useEffect(() => { refreshAdvisorTag(); }, []);
  // WebView 崩溃自愈心跳：每 5s 告诉后端「前端还活着」。后端 watchdog 在
  // 连续 12s 收不到心跳时判定 WebView2 渲染进程崩溃，自动 reload 自愈。
  useEffect(() => {
    const timer = window.setInterval(() => {
      void api.webviewHeartbeat();
    }, 5000);
    return () => window.clearInterval(timer);
  }, []);
  // AI 侧栏占位是恒定的（扩展时），三栏工作台的实际可用宽度要扣掉它。
  const aiFootprint = aiOpen ? aiWidth + 8 : 0;
  // 右栏可见性：内容区（窗口 − AI 栏）至少能放下「左栏+中栏+右栏」最小预算才显示；
  // 否则隐藏右栏，把宽度让给目录树和详情面板。窗口放大后自动恢复。
  const [rightVisible, setRightVisible] = useState<boolean>(() => true);
  useEffect(() => {
    const avail = () => window.innerWidth - aiFootprint - sidebarFootprint - 40;
    const show = avail() >= THREE_COL_BUDGET;
    setRightVisible(show);
    const onResize = () => setRightVisible(avail() >= THREE_COL_BUDGET);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, [aiFootprint, sidebarFootprint]);
  const [leftWidth, setLeftWidth] = useState<number>(() => {
    const v = Number(safeGetItem('diskpilot.leftWidth'));
    return Number.isFinite(v) && v > MIN_LEFT ? v : DEFAULT_LEFT;
  });
  const [rightWidth, setRightWidth] = useState<number>(() => {
    const v = Number(safeGetItem('diskpilot.rightWidth'));
    return Number.isFinite(v) && v > MIN_RIGHT ? v : DEFAULT_RIGHT;
  });

  useEffect(() => { safeSetItem('diskpilot.leftWidth', String(leftWidth)); }, [leftWidth]);
  useEffect(() => { safeSetItem('diskpilot.rightWidth', String(rightWidth)); }, [rightWidth]);

  // 窗口缩放（或 AI 栏开合）后，若三栏总和超出可用宽度，自动把左栏先收缩、
  // 右栏兜底，让中间始终保底 MIN_CENTER，避免工作台溢出被裁。
  useEffect(() => {
    const fit = () => {
      const winW = window.innerWidth;
      const avail = Math.max(MIN_LEFT + MIN_CENTER + MIN_RIGHT, winW - aiFootprint - sidebarFootprint - 40);
      const over = leftWidth + rightWidth + MIN_CENTER - avail;
      if (over > 0) {
        const lShrink = Math.min(over, leftWidth - MIN_LEFT);
        const rShrink = over - lShrink;
        const l2 = Math.max(MIN_LEFT, leftWidth - lShrink);
        const r2 = Math.max(MIN_RIGHT, rightWidth - Math.max(0, rShrink));
        setLeftWidth(l2);
        setRightWidth(r2);
        // R408：左/右都收到底仍超预算 → 收 AI 侧栏，保住中栏 MIN_CENTER。
        if (l2 + r2 + MIN_CENTER - avail > 0) {
          setAiWidth((w) => clampAiWidth(w, winW, sidebarFootprint));
        }
      }
    };
    fit();
    window.addEventListener('resize', fit);
    return () => window.removeEventListener('resize', fit);
  }, [aiFootprint, sidebarFootprint, leftWidth, rightWidth]);

  const dragLeft = (dx: number) => {
    setLeftWidth((w) => {
      const winW = window.innerWidth;
      // 可用宽度 = 窗口 - 侧栏 - AI 侧栏占位 - 右栏（若显示） - 中栏保底
      const others = (rightVisible ? rightWidth + 8 : 0) + MIN_CENTER;
      const maxLeft = Math.max(MIN_LEFT, winW - sidebarFootprint - aiFootprint - others - 24);
      return Math.max(MIN_LEFT, Math.min(maxLeft, w + dx));
    });
  };
  const dragRight = (dx: number) => {
    setRightWidth((w) => {
      const winW = window.innerWidth;
      const maxRight = Math.max(MIN_RIGHT, winW - sidebarFootprint - aiFootprint - leftWidth - MIN_CENTER - 24);
      return Math.max(MIN_RIGHT, Math.min(maxRight, w - dx));
    });
  };

  useEffect(() => { api.listScaffolds().then(setScaffolds).catch(() => {}); }, [setScaffolds]);

  // 启动即列出本机所有磁盘，长辈不用手动选文件夹。
  // R231：跟随扫描完成信号重拉——每次扫描后磁盘条/趋势反映最新磁盘值；
  // 缓存命中不 bump scanSeq（那是 ChatPanel 的语义），但清理后重扫同一盘
  // 磁盘卡也要更新，所以用 reclaimedBytes 驱动（清理时它必变；L107 已订阅）。
  const scanSeq = useStore((s) => s.scanSeq);
  useEffect(() => {
    api.listDrives().then(setDrives).catch(() => {});
  }, [scanSeq, reclaimedBytes]);

  // 托盘菜单文案：启动时按当前语言推一次（切语言时 Settings 会再推）。
  useEffect(() => {
    if (!isTauri) return;
    api.traySync(
      t('settings.general.trayShow'),
      t('settings.general.trayQuit'),
      t('settings.general.trayTooltip'),
    ).catch(() => {});
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!isTauri) return;
    const unlisten = listen<ScanStatsEvent>('scan-stats', (e) => {
      lastBackendStats.current = e.payload;
    });
    return () => { unlisten.then((u) => u()); };
  }, []);

  // 清理提醒（R3）：后端定时算完建议后 emit。只通知「有可清项」，不做任何
  // 清理动作——用户点击引导跳总览，确认窗仍是唯一执行入口。
  useEffect(() => {
    if (!isTauri) return;
    const unlisten = listen<import('./api').ReminderPayload>('cleanup-reminder', (e) => {
      const p = e.payload;
      // health-butler P2-1：有规则命中的话在通知里点出（旧后端无该字段则跳过）。
      const hitN = p.auto_list_hits?.length ?? 0;
      const body = hitN > 0
        ? t('shell.toast.cleanupReminder', { size: formatBytes(p.total_bytes), n: p.drives.length }) + ' · ' + t('system.reminder.autoListHits', { items: hitN })
        : t('shell.toast.cleanupReminder', { size: formatBytes(p.total_bytes), n: p.drives.length });
      useStore.getState().toast(
        body,
        'ok',
        // R103：点击直达总览建议清理区（那里仍是唯一执行入口）。
        { label: t('shell.toast.actionOverview'), run: () => setView('overview') },
      );
    });
    return () => { unlisten.then((u) => u()); };
    // t 进依赖：切语言后重订阅，事件 toast 才不会用旧语言的文案函数。
  }, [t]);

  // 周巡检简报（health-butler P1）：后台定时生成后 emit。同样是只读聚合，
  // 只通知「本周快照已生成」，不含任何清理动作；详情在操作历史页。
  useEffect(() => {
    if (!isTauri) return;
    const unlisten = listen<import('./api').WeeklyReport>('health-weekly', (e) => {
      const p = e.payload;
      useStore.getState().toast(
        t('shell.toast.healthWeekly', { size: formatBytes(p.total_bytes) }),
        'ok',
        // R103：点击直达操作历史页看周报全文。
        { label: t('shell.toast.actionHistory'), run: () => setView('history') },
      );
    });
    return () => { unlisten.then((u) => u()); };
  }, [t]);

  // 空间耗尽预警（R99）：后端按空间趋势外推，发现 ≤N 天会满的盘 emit。
  // 只通知「哪块盘快满了 + 预计天数」，清理仍走确认窗，绝不自动删。
  useEffect(() => {
    if (!isTauri) return;
    const unlisten = listen<import('./api').SpaceFullWarn[]>('space-full-reminder', (e) => {
      const warns = e.payload;
      if (!Array.isArray(warns) || warns.length === 0) return;
      const first = warns[0];
      useStore.getState().toast(
        t('shell.toast.spaceFull', {
          path: first.path,
          days: first.days === 0 ? t('shell.toast.spaceFullNow') : String(first.days),
          extra: warns.length > 1 ? t('shell.toast.spaceFullMore', { n: warns.length - 1 }) : '',
        }),
        'warn',
        // R103：点击直达总览看空间趋势。
        { label: t('shell.toast.actionOverview'), run: () => setView('overview') },
      );
    });
    return () => { unlisten.then((u) => u()); };
  }, [t]);

  // R291 温度持续偏高 → 提示去应用散热曲线（health-butler P2-2）。
  // **只导航，不代写**：写入调速通道仍走 fan_control 的 confirmed + 权限中心
  // 双确认；本条不绕过、也不自动应用任何曲线。
  // 动作直达「设置 → 系统工具」的风扇控制区，而不是总览——风扇控制在那边，
  // 跳总览等于让用户自己再找一遍（R294 修正此前跳 Overview 的误导）。
  useEffect(() => {
    if (!isTauri) return;
    const unlisten = listen<import('./api').ThermalSustained>('thermal-sustained', (e) => {
      const p = e.payload;
      if (!p) return;
      useStore.getState().toast(
        t('shell.toast.thermalSustained', {
          temp: p.temp_max_c != null ? String(Math.round(p.temp_max_c)) : '?',
          threshold: String(Math.round(p.threshold_c)),
          samples: String(p.samples),
        }),
        'warn',
        {
          label: t('shell.toast.actionFanCurve'),
          run: () => { setSettingsTab('system'); setShowSettings(true); },
        },
      );
    });
    return () => { unlisten.then((u) => u()); };
  }, [t]);

  // R292 启动项变化。**只说新增/消失了几项，不说是谁做的**：软件自装与恶意
  // 自装在数据上无法区分，后端没有能力判断，前端更不该替它断言。
  useEffect(() => {
    if (!isTauri) return;
    const unlisten = listen<import('./api').StartupChanged>('startup-changed', (e) => {
      const p = e.payload;
      if (!p) return;
      if (p.added.length === 0 && p.gone.length === 0) return;
      const goneTxt =
        p.gone.length > 0 ? t('shell.toast.startupGone', { gone: p.gone.length }) : '';
      const gapTxt =
        p.gap_days > 0
          ? String(p.gap_days)
          : t('shell.toast.startupNoGap');
      useStore.getState().toast(
        t('shell.toast.startupChanged', {
          added: p.added.length,
          gone: goneTxt,
          days: gapTxt,
        }),
        'warn',
        // 动作直达「系统优化」页：启动项明细与启停都在那里（R304 此前跳总览，
        // 而总览只有一行不可点的计数——与 R294 散热 toast 同族问题）。
        { label: t('shell.toast.actionStartup'), run: () => setView(PAGE_IDS.optimizer) },
      );
    });
    return () => { unlisten.then((u) => u()); };
  }, [t]);

  // R293 扫描久未（只通知「多久没扫」，不推测用户为什么没扫——可能是主动
  // 不需要、也可能换了盘。归因超出数据能力）。
  useEffect(() => {
    if (!isTauri) return;
    const unlisten = listen<import('./api').ScanStaleWarn[]>('scan-stale', (e) => {
      const warns = e.payload;
      if (!Array.isArray(warns) || warns.length === 0) return;
      const first = warns[0];
      // 从未扫过与「很久没扫」是两回事，措辞必须分开。
      const body = first.never
        ? t('shell.toast.scanStaleNever', { path: first.path })
        : t('shell.toast.scanStale', {
            path: first.path,
            days: first.days != null ? String(first.days) : '?',
          });
      useStore.getState().toast(
        body
          + (warns.length > 1
            ? t('shell.toast.scanStaleMore', { n: warns.length - 1 })
            : ''),
        'warn',
        { label: t('shell.toast.actionOverview'), run: () => setView('overview') },
      );
    });
    return () => { unlisten.then((u) => u()); };
  }, [t]);

  const pickDirectory = async () => {
    if (!isTauri) {
      // 提示语走文案表；默认值 'C:\\' 是数据，保持原样。
      const p = window.prompt(t('shell.scan.browserPrompt'), 'C:\\');
      if (p) setPickedPath(p);
      return;
    }
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === 'string') setPickedPath(picked);
  };

  const scan = async (paths?: string[], opts?: { force?: boolean }) => {
    const targets = paths && paths.length > 0 ? paths : [pickedPath];
    if (!targets[0]) return;
    setErr(null);
    const force = !!opts?.force;

    // 逐个目标查缓存：命中直接复用已扫的树，未命中（或强制刷新）才真正扫描。
    // 全盘扫完后再点盘符 → 全部命中 → 秒开，不重新遍历。
    const cache = useStore.getState().scanCache;
    const cached = new Map<string, Node>();
    const uncached: string[] = [];
    for (const target of targets) {
      const hit = !force ? cache[normKey(target)] : undefined;
      if (hit) cached.set(target, hit);
      else uncached.push(target);
    }
    if (uncached.length === 0 && cached.size > 0) {
      showTargets(targets, cached, new Map(), true);
      return;
    }

    // R383：已有扫描在跑时提示并放弃本轮（闸门由 finally 释放，取消也走 finally）。
    if (!scanGate.tryEnter()) {
      useStore.getState().toast(t('shell.scan.busy'));
      return;
    }

    setScanning(true); setDiag(null);
    // begin 会清空上一轮进度并触发盘根总量探针（进度分母）。
    scanBarRef.current?.begin(uncached);
    lastBackendStats.current = null;

    // 全盘模式：默认并行扫盘（设置里可关），把各盘根节点合并为一个"我的电脑"虚拟根。
    const parallel = isTauri && uncached.length > 1 && prefs.parallelScan;

    const tTotal0 = performance.now();
    try {
      const scanOne = async (target: string): Promise<Node> => {
        setPickedPath(target);
        return api.scan(target, prefs.keepFilesPerDir);
      };
      const tScan0 = performance.now();
      let fresh: Node[];
      if (parallel) {
        fresh = await Promise.all(uncached.map(scanOne));
      } else {
        fresh = [];
        for (const target of uncached) fresh.push(await scanOne(target));
      }
      const tScan1 = performance.now();

      // 新扫的树写入缓存（按目标路径归一化 key）。
      const freshByTarget = new Map<string, Node>();
      uncached.forEach((target, i) => freshByTarget.set(target, fresh[i]));
      const added: Record<string, Node> = {};
      for (const [target, node] of freshByTarget) added[normKey(target)] = node;
      useStore.getState().cacheDrives(added);

      showTargets(targets, cached, freshByTarget, false);

      const totalMs = performance.now() - tTotal0;
      // Cast through the ref accessor: TS narrows `.current` to the last
      // assignment it sees in this flow (the `= null` reset earlier), missing
      // the listener's assignment from another effect.
      const backend = (lastBackendStats.current as unknown) as ScanStatsEvent | null;
      const ipcMs: number | null =
        backend !== null ? Math.max(0, (tScan1 - tScan0) - backend.cmd_total_ms) : null;
      const next: ScanDiag = {
        backend,
        ipcMs,
        scanCallMs: tScan1 - tScan0,
        setRootMs: lastSetRootMs.current,
        totalMs,
      };
      setDiag(next);
      console.log('[diskpilot.diag]', {
        backend,
        scanCallMs: next.scanCallMs.toFixed(1),
        ipcMs: ipcMs?.toFixed(1) ?? null,
        setRootMs: lastSetRootMs.current.toFixed(1),
        totalMs: totalMs.toFixed(1),
      });
    } catch (e) {
      // 用户主动取消：后端以稳定前缀 `scan:cancelled:` 标记，是正常中断，
      // 不清空已有树、不弹错误条。
      if (api.isScanCancelledError(e)) {
        console.log('[diskpilot] scan cancelled by user');
        // R465：取消不是完成——进度条收回 100% 的「完成」态，换成「已取消」的中性说明。
        scanBarRef.current?.interrupt('cancelled');
        return;
      }
      // R465：失败同样不是完成——错误横幅已经报错，进度条不能同时说「已完成」。
      scanBarRef.current?.interrupt('failed');
      setErr(String(e));
    } finally {
      scanGate.leave();
      setScanning(false);
    }
  };

  // 根据「缓存命中 + 本次新扫」两类结果组装 root。targets 顺序保留，
  // 单目标直接用它自己的树，多目标合并为"我的电脑（全部磁盘）"虚拟根。
  const showTargets = (
    targets: string[],
    cached: Map<string, Node>,
    fresh: Map<string, Node>,
    fromCache: boolean,
  ) => {
    const nodes = targets
      .map((target) => cached.get(target) ?? fresh.get(target))
      .filter(Boolean) as Node[];
    if (nodes.length === 0) return;
    const multi = targets.length > 1;
    const mergedSize = nodes.reduce((s, n) => s + (n.size || 0), 0);
    const mergedFiles = nodes.reduce((s, n) => s + (n.file_count || 0), 0);
    // 虚拟根的 path 是稳定标识：store.ts 里按 `root.path === '全部磁盘'` 判定
    // 跨盘根并决定剪枝范围，属于「用于比较的字符串常量」，不随语言变（i18n 刻意不译）。
    const rootNode: Node = {
      name: multi ? t('shell.root.allDrives') : (nodes[0].name || nodes[0].path),
      path: multi ? '全部磁盘' : nodes[0].path, // @i18n-keep 与 store.ts 比较用的稳定标识，不随语言变
      is_dir: true,
      size: mergedSize,
      file_count: mergedFiles,
      children: multi ? nodes : nodes[0].children ?? [],
      top_extensions: [],
    };
    const t0 = performance.now();
    setRoot(rootNode);
    select(rootNode.path);
    lastSetRootMs.current = performance.now() - t0;
    // 只有真正跑了扫描（fresh 非空）才递增 scanSeq：缓存命中打开不触发，
    // ChatPanel 靠它区分「同路径的新扫描」与「切页导致的重挂载」。
    if (fresh.size > 0) useStore.getState().bumpScanSeq();
    if (fromCache) {
      useStore.getState().toast(t('shell.toast.openedFromCache'), 'ok');
    }
  };

  const pageNav = navItemById(view);

  return (
    <div className="app">
      {/* 开屏免责声明：首次启动强制阅读（10s 倒计时 + 滑到底部），同意后不再弹 */}
      <Disclaimer />
      {/* R363：跳转链接——键盘用户 Tab 第一击就到主内容，不必穿过整个侧栏 */}
      <a className="skip-link" href="#main-content">{t('shell.skipToMain')}</a>
      {/* 导航侧栏：页面分发 + 折叠三态；品牌与设置入口都收在这里 */}
      <Sidebar
        current={view}
        onNavigate={navigate}
        expanded={sidebar.expanded}
        onToggle={toggleSidebar}
        busy={scanning ? PAGE_IDS.workspace : null}
        settingsDot={!!advisorTag}
        settingsHint={advisorTag ? t('shell.nav.settingsBound', { provider: advisorTag.provider }) : t('shell.nav.settingsUnbound')}
      />
      {/* 全局 AI 侧栏：贯穿所有页面，可折叠/可拖宽 */}
      <div className={'ai-rail' + (aiOpen ? ' open' : '')} style={aiOpen ? { width: aiWidth } : undefined}>
        <div className="ai-rail-head">
          <span className="ai-rail-title"><MessageSquare size={14} /> {t('shell.airail.title')}</span>
          <button className="ghost icon" onClick={() => setAiOpen(false)} title={t('shell.airail.collapse')}><X size={14} /></button>
        </div>
        <div className="ai-rail-body">
          {/* R394：AI 助手是常驻面板，出错此前会让整棵树崩掉（Suspense 不接错误）。 */}
          <ErrorBoundary fallbackLabel={t('shell.boundary.chatFailed')}>
            <Suspense fallback={<PageFallback />}>
              <ChatPanel onOpenSettings={() => { setSettingsTab('ai'); setShowSettings(true); }} />
            </Suspense>
          </ErrorBoundary>
        </div>
      </div>
      {aiOpen && <Splitter onDrag={dragAi} value={aiWidth} min={AI_WIDTH_MIN} />}

      <div className="app-content">
      <PageHeader
        title={pageNav ? t(pageNav.titleKey) : ''}
        subtitle={pageNav?.subtitleKey ? t(pageNav.subtitleKey) : undefined}
        actions={
          <>
            {view === 'workspace' && (
              <span className="muted small ws-hint">{t('shell.nav.wsHint')}</span>
            )}
            {reclaimedBytes > 0 && (
              <span className="reclaimed-pill" title={t('shell.nav.reclaimedTitle')}>
                {t('shell.nav.reclaimed', { size: formatBytes(reclaimedBytes) })}
              </span>
            )}
            {root && (
              <span className="muted small ws-status">
                {t('shell.nav.rootStatus', { size: formatBytes(root.size), n: root.file_count.toLocaleString() })}
              </span>
            )}
            {view === 'workspace' && (
              <>
                <button className="ghost icon" onClick={pickDirectory} title={pickedPath ? t('shell.scan.picked', { path: pickedPath }) : t('shell.scan.pickFolder')}>
                  <Folder size={14} />
                </button>
                <button
                  className="primary scan-btn"
                  onClick={() => {
                    if (scanning) {
                      void api.cancelScan();
                      return;
                    }
                    void scan();
                  }}
                  disabled={!pickedPath}
                >
                  <ScanLine size={14} /> {scanning ? t('shell.scan.cancel') : t('shell.scan.start')}
                </button>
              </>
            )}
          </>
        }
      />

      <ScanBar scanning={scanning} ref={scanBarRef} />
      {/* R97 浏览器预览模式：非 Tauri 环境全部数据来自 mock，顶部横幅标识
          「模拟数据」——诚实性：不许把演示数据当成真实扫描结果。 */}
      {!isTauri && (
        <div className="banner preview" role="note">
          {t('shell.previewBanner')}
        </div>
      )}
      {/* R389：扫描失败的红横幅此前只有「下一次扫描开始」才消失，没有关闭入口，
          读屏/鼠标用户都得一直看着旧错误。给它一个 X，点了立刻清掉。 */}
      {err && (
        <div className="banner error" role="alert">
          <span className="banner-msg">{err}</span>
          <button
            className="banner-dismiss"
            onClick={() => setErr(null)}
            aria-label={t('common.close')}
            title={t('common.close')}
          >
            <X size={13} />
          </button>
        </div>
      )}
      {!err && !scanning && diag?.backend?.mode === 'walkdir' && (
        <div className="hint">{t('shell.scan.adminHint')}</div>
      )}

      {view === 'overview' ? (
        <main id="main-content" ref={mainRef} tabIndex={-1} className="overview">
          <ErrorBoundary fallbackLabel={t('shell.boundary.overviewFailed')}>
            <Suspense fallback={<PageFallback />}>
              <AssetOverview
                root={root}
                drives={drives}
                scaffolds={scaffolds}
                scanning={scanning}
                onScanDrive={(p) => scan([p])}
                onScanAll={() => drives.length > 0 && scan(scanTargets(drives))}
                onRefresh={(p) => scan([p], { force: true })}
                // R91: 趋势行点击带盘路径 → 先打开该盘（缓存命中秒开）再切页，
                // 用户从「C 盘曲线」进来就该落在 C 盘工作台，而不是默认视图。
                // R248：空间大头 Top 10 行也带路径——子目录路径时扫完盘根再
                // selectPath 定位到该目录，工作台一进来就停在那个大头目录上
                // （FileView 按 selectedPath 逐段下钻，无需节点引用）。
                onGoWorkspace={async (p) => {
                  if (!p) { setView('workspace'); return; }
                  const isRoot = /^[A-Za-z]:[\\/]$/.test(p) || p === '全部磁盘'; // @i18n-keep 与 showTargets 的虚拟根稳定标识同源，不随语言变
                  if (isRoot) {
                    scan([p]);
                    setView('workspace');
                    return;
                  }
                  // 盘根 = 第一个分隔符之后（C:\Users\AppData → C:\）；见
                  // format.driveRootOf（含 UNC 处理）。
                  await scan([driveRootOf(p)]);
                  if (useStore.getState().scanCache[normKey(driveRootOf(p))]) {
                    useStore.getState().selectPath(p);
                  }
                  setView('workspace');
                }}
                onOpenCleanup={(scaffoldId?: string) => openCleanup(scaffoldId)}
                onGoHardware={() => setView('tools')}
                onGoStartup={() => setView(PAGE_IDS.optimizer)}
              />
            </Suspense>
          </ErrorBoundary>
        </main>
      ) : view === 'tools' ? (
        <main id="main-content" ref={mainRef} tabIndex={-1} className="overview">
          <ErrorBoundary fallbackLabel={t('shell.boundary.toolbeltFailed')}>
          <Suspense fallback={<PageFallback />}>
            <Toolbelt
              drives={drives}
              scanning={scanning}
              onScanDrive={(p) => scan([p])}
              onScanAll={() => drives.length > 0 && scan(scanTargets(drives))}
              onGoWorkspace={() => setView('workspace')}
              onOpenSettings={(tab) => { setSettingsTab(tab); setShowSettings(true); }}
            />
          </Suspense>
          </ErrorBoundary>
        </main>
      ) : view === 'cleanup' ? (
        <main id="main-content" ref={mainRef} tabIndex={-1} className="cleanup-page">
          <ErrorBoundary fallbackLabel={t('shell.boundary.cleanupFailed')}>
            <Suspense fallback={<PageFallback />}>
              <CleanupPage onOpenHistory={openHistory} />
            </Suspense>
          </ErrorBoundary>
        </main>
      ) : view === 'privacy' ? (
        <main id="main-content" ref={mainRef} tabIndex={-1} className="cleanup-page">
          <ErrorBoundary fallbackLabel={t('shell.boundary.cleanupFailed')}>
            <Suspense fallback={<PageFallback />}>
              <CleanupPage tagFilter="privacy" onOpenHistory={openHistory} />
            </Suspense>
          </ErrorBoundary>
        </main>
      ) : view === 'optimizer' ? (
        <main id="main-content" ref={mainRef} tabIndex={-1} className="optimizer-page">
          <ErrorBoundary fallbackLabel={t('shell.boundary.optimizerFailed')}>
            <Suspense fallback={<PageFallback />}>
              <OptimizerPage />
            </Suspense>
          </ErrorBoundary>
        </main>
      ) : view === 'history' ? (
        <main id="main-content" ref={mainRef} tabIndex={-1} className="history-page">
          <ErrorBoundary fallbackLabel={t('shell.boundary.historyFailed')}>
            <Suspense fallback={<PageFallback />}>
              <HistoryPage onOpenCleanup={() => openCleanup()} />
            </Suspense>
          </ErrorBoundary>
        </main>
      ) : (
      <main
        id="main-content"
        ref={mainRef}
        tabIndex={-1}
        className="ws-main"
        style={{
          gridTemplateColumns: rightVisible
            ? `${leftWidth}px 8px 1fr 8px ${rightWidth}px`
            : `${leftWidth}px 8px 1fr`,
        }}
      >
        {drives.length > 0 && (
          <div className="ws-strip">
            <DriveStrip
              drives={drives}
              scanning={scanning}
              onScanAll={() => drives.length > 0 && scan(scanTargets(drives))}
              onScanDrive={(p) => scan([p])}
              onRefresh={(p) => scan([p], { force: true })}
            />
          </div>
        )}
        <aside className="left" aria-label={t('shell.leftPanel.aria')}>
          {root ? <LeftPanel root={root} selectedPath={selectedPath} onSelect={select} /> : <EmptyLeft />}
        </aside>

        <Splitter onDrag={dragLeft} onDoubleClick={() => setLeftWidth(DEFAULT_LEFT)} value={leftWidth} min={MIN_LEFT} />

        <section className="center">
          <ErrorBoundary fallbackLabel={t('shell.boundary.detailFailed')}>
            <FileDetailPanel />
          </ErrorBoundary>
        </section>

        <Splitter onDrag={dragRight} onDoubleClick={() => setRightWidth(DEFAULT_RIGHT)} value={rightWidth} min={MIN_RIGHT} />

        {rightVisible && (
        <aside className="right" aria-label={t('shell.rightPanel.aria')}>
          <ErrorBoundary fallbackLabel={t('shell.boundary.studioFailed')}>
            <Suspense fallback={<PageFallback />}>
              <Studio onOpenCleanup={(id) => openCleanup(id)} onOpenHistory={openHistory} />
            </Suspense>
          </ErrorBoundary>
        </aside>
        )}
      </main>
      )}

      <footer>
        <span>{t('shell.footer.rules', { version: APP_VERSION, n: scaffolds.length })}</span>
        <span>{root?.path ?? t('shell.footer.noScan')}</span>
      </footer>
      </div>

      {/* AI 侧栏折叠后的浮动唤起钮 */}
      {!aiOpen && (
        <button className="ai-rail-toggle" onClick={() => setAiOpen(true)} title={t('shell.airail.expand')}>
          <MessageSquare size={16} />
        </button>
      )}

      {showSettings && (
        /* R394：设置窗是懒加载页面，出错此前同样没有兜底。 */
        <ErrorBoundary fallbackLabel={t('shell.boundary.settingsFailed')}>
          <Suspense fallback={<PageFallback />}>
            <Settings onClose={() => { setShowSettings(false); refreshAdvisorTag(); }} initialTab={settingsTab} />
          </Suspense>
        </ErrorBoundary>
      )}
      <CleanupProposalDialog />

      {/* R319：toast 是本应用的主要反馈通道，之前没有任何 live region——
          读屏软件对「清理完成 / 打不开 / 提醒」一律静默。容器常驻，内容一变就播报。 */}
      <div className="toast-stack" role="status" aria-live="polite">
        {toasts.map((toast) => (
          <div key={toast.id} className={'toast ' + toast.kind}>
            <span className="toast-text">{toast.text}</span>
            {toast.action && (
              <button
                type="button"
                className="toast-action"
                onClick={(ev) => {
                  ev.stopPropagation();
                  toast.action!.run();
                  dismissToast(toast.id);
                }}
              >
                {toast.action.label}
              </button>
            )}
            <button
              type="button"
              className="toast-close"
              aria-label={t('common.close')}
              onClick={() => dismissToast(toast.id)}
            >
              <X size={12} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

function EmptyLeft() {
  const t = useT();
  return (
    <div className="empty">
      <div className="empty-title">{t('shell.empty.title')}</div>
      <div className="empty-sub">
        {t('shell.empty.howto')}<br /><br />
        {t('shell.empty.layout')}<br />
        {t('shell.empty.instant')}
      </div>
    </div>
  );
}

// ── 工作台中部：选中项详情 + 清理建议（替代原 AI 聊天位）──
// 全局 AI 已移到最左侧栏，中部专注「当前选中对象」的详情与快捷操作。
function FileDetailPanel() {
  const t = useT();
  const root = useStore((s) => s.root);
  const selectedPath = useStore((s) => s.selectedPath);
  const selectedNode = useStore((s) => s.selectedNode);
  const select = useStore((s) => s.selectPath);
  const recycle = useStore((s) => s.recyclePaths);
  const pushTurn = useStore((s) => s.pushChatTurn);
  const setBusy = useStore((s) => s.setChatBusy);
  // R94: 回收是破坏性动作，必须两步确认（铁律#6）——TreeView/FileView 都有
  // ConfirmDialog 兜底，唯独中央详情面板此前单击即执行，误点即移入回收站。
  const [pendingRecycle, setPendingRecycle] = useState<{ path: string; size: number } | null>(null);

  // C3：不再从 root 整树 DFS 找选中节点。选中节点由点击处直接把引用存进
  // store（selectedNode），这里 O(1) 读取。selectedNode 可能为 null——
  // 由旧调用点只传 path（如 FileView 的过滤结果，节点不在当前 root 上）
  // 或回收剪枝后清空导致；此时回退用 path 大小写不敏感的整树查找兜底
  // （低频路径，只在无引用时触发，不影响常规点击的主路径性能）。
  const node = useMemo(() => {
    if (selectedNode) return selectedNode;
    if (!root || !selectedPath) return null;
    const walk = (n: Node): Node | null => {
      if (n.path === selectedPath) return n;
      for (const c of n.children ?? []) {
        const f = walk(c);
        if (f) return f;
      }
      return null;
    };
    return walk(root);
  }, [selectedNode, root, selectedPath]);

  if (!node) {
    return (
      <div className="fdp-empty">
        <div className="fdp-empty-title">{t('shell.fdp.pickFolder')}</div>
        <div className="muted small">{t('shell.fdp.pickHint')}</div>
      </div>
    );
  }

  const pct = root && root.size > 0 ? (node.size / root.size) * 100 : 0;

  const askAi = () => {
    // 这段是发给 AI 模型的 prompt 文本（i18n 刻意不译，见 src/i18n/namespaces/index.ts 不译清单）。
    pushTurn({
      id: Math.random().toString(36).slice(2),
      role: 'user',
      text: `分析这个目录：${node.path}\n它占用了 ${formatBytes(node.size)}（占比 ${pct.toFixed(1)}%），共 ${node.file_count?.toLocaleString() ?? '?'} 个文件。告诉我里面是什么、哪些可以清理、预计能释放多少。`, // @i18n-keep 发给模型的提问
    });
    setBusy(true);
  };

  return (
    <div className="fdp">
      <div className="fdp-head">
        <span className="fdp-path" title={node.path}>{node.is_dir ? '📁' : '📄'} {node.name || node.path}</span>
        <button className="ghost icon" onClick={() => select(node.path, node)} title={t('shell.fdp.locate')}><ListTree size={14} /></button>
      </div>
      <div className="fdp-meta">
        <span className="fdp-stat"><b>{formatBytes(node.size)}</b> {t('shell.fdp.used')}</span>
        {node.is_dir && node.file_count != null && (
          <span className="fdp-stat"><b>{node.file_count.toLocaleString()}</b> {t('shell.fdp.files')}</span>
        )}
        {node.is_dir && <span className="fdp-stat"><b>{pct.toFixed(1)}%</b> {t('shell.fdp.pctOfScan')}</span>}
      </div>
      {node.is_dir && (
        <div className="fdp-bar"><span style={{ width: `${Math.min(100, pct)}%` }} /></div>
      )}
      <div className="fdp-path-full">{node.path}</div>

      <div className="fdp-actions">
        <button className="ghost" onClick={askAi} title={t('shell.fdp.askTitle')}>
          <MessageSquare size={13} /> {t('shell.fdp.askAi')}
        </button>
        {/* '手动回收' 是发给后端 recycle_paths 的 reason 参数值（写进 undo.jsonl），
            与 TreeView/FileView 同一枚字符串，按 i18n 不译清单保持中文。
            R94: 先弹确认面板，确认后才真正执行。 */}
        <button className="ghost" onClick={() => setPendingRecycle({ path: node.path, size: node.size })} disabled={!node.is_dir}>{/* @i18n-keep */}
          <TrashIcon size={13} /> {t('shell.fdp.recycle')}
        </button>
      </div>

      {node.is_dir && (
        <div className="fdp-children">
          <div className="fdp-children-title">{t('shell.fdp.topChildren')}</div>
          {[...(node.children ?? [])]
            .sort((a, b) => b.size - a.size)
            .slice(0, 6)
            .map((c) => (
              <button key={c.path} className="fdp-child" onClick={() => select(c.path)}>
                <span className="fdp-child-name" title={c.path}>{c.is_dir ? '📁' : '📄'} {c.name}</span>
                <span className="fdp-child-size">{formatBytes(c.size)}</span>
              </button>
            ))}
        </div>
      )}

      {pendingRecycle && (
        <ConfirmDialog
          title={t('shell.tooltree.recycle')}
          body={t('shell.tooltree.recycleConfirm', {
            name: pendingRecycle.path.split(/[\\/]/).filter(Boolean).pop() || pendingRecycle.path,
            size: formatBytes(pendingRecycle.size),
          })}
          confirmLabel={t('shell.fdp.recycle')}
          onConfirm={() => {
            // '手动回收' 是写进回收记录/undo 日志的后端参数值，不随界面语言变。
            void recycle([{ path: pendingRecycle.path, size_hint: pendingRecycle.size }], '手动回收'); // @i18n-keep
            setPendingRecycle(null);
          }}
          onCancel={() => setPendingRecycle(null)}
        />
      )}
    </div>
  );
}

// ── 扫描进度条 ──
// 高频状态（scan-progress 事件每 tick 一次）全部自持在组件内：滚动文件数/
// 字节/当前路径只重渲这一条进度条，不再穿透 App 整树。App 只在扫描开始/
// 结束时翻转 scanning（低频），并通过 ref 下发「目标列表 + 盘根总量探针」。
function isDriveRoot(p: string): boolean {
  // C: / C:\ / C:/  — anything beyond is a subfolder
  return /^[A-Za-z]:[\\/]?$/.test(p);
}

// 盘根用 volumeInfo（一次系统调用，盘总占用即进度分母）；子目录不再预跑
// estimate_size 估分母（会在正式扫描前把目标完整多走一遍，IO 翻倍），
// 子目录扫描进度条退化为不定长（文件数+已扫字节）。
function probeDriveTotals(targets: string[], setTotalBytes: (n: number) => void): void {
  let v = 0;
  for (const target of targets) {
    if (!isDriveRoot(target)) continue;
    api.volumeInfo(target)
      .then((info) => {
        if (info) { v += info.used_bytes; if (v > 0) setTotalBytes(v); }
      })
      .catch(() => {});
  }
}

const ScanBar = forwardRef<ScanBarHandle, { scanning: boolean }>(function ScanBar({ scanning }, ref) {
  const t = useT();
  const [progress, setProgress] = useState<{ files: number; bytes: number; path: string } | null>(null);
  const [totalBytes, setTotalBytes] = useState<number | null>(null);
  const [pathLabel, setPathLabel] = useState<string | null>(null);
  // R465：本轮是否被中断（取消/失败）。begin 清空，interrupt 置位，完成态只认 interrupted === null。
  const [interrupted, setInterrupted] = useState<null | 'cancelled' | 'failed'>(null);

  useImperativeHandle(ref, () => ({
    begin: (targets: string[]) => {
      // 新一次扫描开始：清空上一轮进度与分母，重新触发盘根总量探针。
      setProgress(null);
      setTotalBytes(null);
      setPathLabel(null);
      setInterrupted(null);
      probeDriveTotals(targets, setTotalBytes);
    },
    interrupt: (kind) => {
      setInterrupted(kind);
    },
    probeTotals: (targets: string[]) => {
      probeDriveTotals(targets, setTotalBytes);
    },
  }), []);

  // 监听 scan-progress（组件内自持依赖，App 不再订阅）：
  // 并行扫多盘时每个盘的事件都往同一个进度条写，后到的字节/文件单调累积
  // （Math.max），保证显示里的数据不倒退。
  useEffect(() => {
    if (!isTauri) return;
    const unlisten = listen<{ files_seen: number; bytes_seen: number; current_path: string }>(
      'scan-progress',
      (e) => {
        const p = e.payload;
        setProgress((prev) => {
          const bytes = Math.max(prev?.bytes ?? 0, p.bytes_seen);
          const files = Math.max(prev?.files ?? 0, p.files_seen);
          const path = p.current_path || prev?.path || '';
          return { files, bytes, path };
        });
        setPathLabel(p.current_path || null);
      },
    );
    return () => { unlisten.then((u) => u()); };
  }, []);

  const complete = progress && totalBytes ? { progress, totalBytes } : null;

  return (
    <>
      {scanning && (
        <div className="scan-bar">
          <div
            className={'scan-bar-fill' + (complete ? '' : ' indeterminate')}
            role="progressbar"
            aria-label={t('shell.scanbar.aria')}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={complete ? Math.round(Math.min(99, (complete.progress.bytes / complete.totalBytes) * 100)) : undefined}
            style={complete ? { width: `${Math.min(99, (complete.progress.bytes / complete.totalBytes) * 100)}%` } : undefined}
          />
          <div className="scan-bar-label" title={pathLabel ?? undefined}>
            {progress
              ? `${t('shell.scanbar.progress', { files: progress.files.toLocaleString(), bytes: formatBytes(progress.bytes) })}${pathLabel ? ` · ${ellipsizePath(pathLabel)}` : ''}`
              : t('shell.scanbar.preparing')}
          </div>
        </div>
      )}
      {!scanning && progress && interrupted === null && (
        <div className="scan-bar scan-bar-done">
          <div className="scan-bar-fill done" aria-hidden style={{ width: '100%' }} />
          <div className="scan-bar-label" title={pathLabel ?? undefined}>
            {`${t('shell.scanbar.done', { files: progress.files.toLocaleString(), bytes: formatBytes(progress.bytes) })}${pathLabel ? ` · ${ellipsizePath(pathLabel)}` : ''}`}
          </div>
        </div>
      )}
      {!scanning && progress && interrupted === 'cancelled' && (
        <div className="scan-bar scan-bar-cancelled" role="status">
          <div className="scan-bar-label" title={pathLabel ?? undefined}>
            {t('shell.scanbar.cancelled', { files: progress.files.toLocaleString(), bytes: formatBytes(progress.bytes) })}
          </div>
        </div>
      )}
    </>
  );
});
