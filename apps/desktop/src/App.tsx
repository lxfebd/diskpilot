import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef, useState } from 'react';
import { Folder, ScanLine, Settings as SettingsIcon, LayoutGrid, ListTree, Wrench, MessageSquare, X, Trash2 as TrashIcon } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { api } from './api';
import { isTauri } from './env';
import { useStore } from './store';
import { LeftPanel } from './components/LeftPanel';
import { ChatPanel } from './components/ChatPanel';
import { Studio } from './components/Studio';
import { Settings } from './components/Settings';
import { AssetOverview } from './components/AssetOverview';
import { Toolbelt } from './components/Toolbelt';
import { DriveStrip } from './components/DriveStrip';
import { Splitter } from './components/Splitter';
import { Logo } from './components/Logo';
import { ErrorBoundary } from './components/ErrorBoundary';
import { CleanupProposalDialog } from './components/CleanupProposalDialog';
import Disclaimer from './components/Disclaimer';
import { formatBytes, ellipsizePath } from './format';
import { loadSettings, isConfigured } from './advisorClient';
import { prefs } from './theme';
import { useT } from './i18n';
import type { Node } from './types';

function normKey(p: string): string {
  return p.replace(/[\\/]+$/, '').toUpperCase();
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
}

// ScanBar 暴露给 App 的命令接口：扫描开始（目标列表）+ 盘根总量探针。
export interface ScanBarHandle {
  begin: (targets: string[]) => void;
  probeTotals: (targets: string[]) => void;
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

  const [scanning, setScanning] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // 扫描进度条独立子组件自持高频状态（scan-progress 事件每 tick 都更新），
  // App 只保留低频 scanning。用 ref 把「扫描开始/目标列表/总量探针」下发给
  // ScanBar，避免进度更新穿透到 App 整树重渲染。
  const scanBarRef = useRef<ScanBarHandle>(null);
  const [pickedPath, setPickedPath] = useState<string>('');
  const [showSettings, setShowSettings] = useState(false);
  const [settingsTab, setSettingsTab] = useState<import('./components/Settings').SettingsTab | undefined>(undefined);
  const [advisorTag, setAdvisorTag] = useState<{ provider: string } | null>(null);
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [view, setView] = useState<'overview' | 'workspace' | 'tools'>('overview');
  const [diag, setDiag] = useState<ScanDiag | null>(null);
  // Holds the latest scan-stats event so we can merge it into ScanDiag once
  // api.scan() returns. Tauri emits the event right before the command resolves.
  const lastBackendStats = useRef(null as ScanStatsEvent | null);

  // ── 全局 AI 侧栏（贯穿所有页面）──
  const [aiOpen, setAiOpen] = useState<boolean>(() => localStorage.getItem('diskpilot.aiOpen') !== '0');
  const [aiWidth, setAiWidth] = useState<number>(() => {
    const v = Number(localStorage.getItem('diskpilot.aiWidth'));
    return Number.isFinite(v) && v >= 260 ? v : 300;
  });
  useEffect(() => { localStorage.setItem('diskpilot.aiOpen', aiOpen ? '1' : '0'); }, [aiOpen]);
  useEffect(() => { localStorage.setItem('diskpilot.aiWidth', String(aiWidth)); }, [aiWidth]);
  // AI 侧栏拖宽
  const dragAi = (dx: number) => {
    setAiWidth((w) => {
      const winW = window.innerWidth;
      // 主内容区至少留 480px（含最小左栏 320 + 中栏 360 的预算内），
      // AI 侧栏最多占到 winW - 480。
      const maxAi = Math.max(260, winW - 480);
      return Math.max(260, Math.min(maxAi, w + dx));
    });
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
    const avail = () => window.innerWidth - aiFootprint - 40;
    const show = avail() >= THREE_COL_BUDGET;
    setRightVisible(show);
    const onResize = () => setRightVisible(avail() >= THREE_COL_BUDGET);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, [aiFootprint]);
  const [leftWidth, setLeftWidth] = useState<number>(() => {
    const v = Number(localStorage.getItem('diskpilot.leftWidth'));
    return Number.isFinite(v) && v > MIN_LEFT ? v : DEFAULT_LEFT;
  });
  const [rightWidth, setRightWidth] = useState<number>(() => {
    const v = Number(localStorage.getItem('diskpilot.rightWidth'));
    return Number.isFinite(v) && v > MIN_RIGHT ? v : DEFAULT_RIGHT;
  });

  useEffect(() => { localStorage.setItem('diskpilot.leftWidth', String(leftWidth)); }, [leftWidth]);
  useEffect(() => { localStorage.setItem('diskpilot.rightWidth', String(rightWidth)); }, [rightWidth]);

  // 窗口缩放（或 AI 栏开合）后，若三栏总和超出可用宽度，自动把左栏先收缩、
  // 右栏兜底，让中间始终保底 MIN_CENTER，避免工作台溢出被裁。
  useEffect(() => {
    const fit = () => {
      const winW = window.innerWidth;
      const avail = Math.max(MIN_LEFT + MIN_CENTER + MIN_RIGHT, winW - aiFootprint - 40);
      const over = leftWidth + rightWidth + MIN_CENTER - avail;
      if (over > 0) {
        const lShrink = Math.min(over, leftWidth - MIN_LEFT);
        const rShrink = over - lShrink;
        setLeftWidth((w) => Math.max(MIN_LEFT, w - lShrink));
        setRightWidth((w) => Math.max(MIN_RIGHT, w - Math.max(0, rShrink)));
      }
    };
    fit();
    window.addEventListener('resize', fit);
    return () => window.removeEventListener('resize', fit);
  }, [aiFootprint, leftWidth, rightWidth]);

  const dragLeft = (dx: number) => {
    setLeftWidth((w) => {
      const winW = window.innerWidth;
      // 可用宽度 = 窗口 - AI 侧栏占位 - 右栏（若显示） - 中栏保底
      const others = (rightVisible ? rightWidth + 8 : 0) + MIN_CENTER;
      const maxLeft = Math.max(MIN_LEFT, winW - aiFootprint - others - 24);
      return Math.max(MIN_LEFT, Math.min(maxLeft, w + dx));
    });
  };
  const dragRight = (dx: number) => {
    setRightWidth((w) => {
      const winW = window.innerWidth;
      const maxRight = Math.max(MIN_RIGHT, winW - aiFootprint - leftWidth - MIN_CENTER - 24);
      return Math.max(MIN_RIGHT, Math.min(maxRight, w - dx));
    });
  };

  useEffect(() => { api.listScaffolds().then(setScaffolds).catch(() => {}); }, [setScaffolds]);

  // 启动即列出本机所有磁盘，长辈不用手动选文件夹。
  useEffect(() => {
    api.listDrives().then(setDrives).catch(() => {});
  }, []);

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
      useStore.getState().toast(
        t('shell.toast.cleanupReminder', { size: formatBytes(p.total_bytes), n: p.drives.length }),
        'ok',
      );
    });
    return () => { unlisten.then((u) => u()); };
    // t 进依赖：切语言后重订阅，事件 toast 才不会用旧语言的文案函数。
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
        setRootMs: 0,
        totalMs,
      };
      setDiag(next);
      console.log('[diskpilot.diag]', {
        backend,
        scanCallMs: next.scanCallMs.toFixed(1),
        ipcMs: ipcMs?.toFixed(1) ?? null,
        totalMs: totalMs.toFixed(1),
      });
    } catch (e) {
      // 用户主动取消：后端以稳定前缀 `scan:cancelled:` 标记，是正常中断，
      // 不清空已有树、不弹错误条。
      if (api.isScanCancelledError(e)) {
        console.log('[diskpilot] scan cancelled by user');
        return;
      }
      setErr(String(e));
    } finally {
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
    setRoot(rootNode);
    select(rootNode.path);
    // 只有真正跑了扫描（fresh 非空）才递增 scanSeq：缓存命中打开不触发，
    // ChatPanel 靠它区分「同路径的新扫描」与「切页导致的重挂载」。
    if (fresh.size > 0) useStore.getState().bumpScanSeq();
    if (fromCache) {
      useStore.getState().toast(t('shell.toast.openedFromCache'), 'ok');
    }
  };

  return (
    <div className="app">
      {/* 开屏免责声明：首次启动强制阅读（10s 倒计时 + 滑到底部），同意后不再弹 */}
      <Disclaimer />
      {/* 全局 AI 侧栏：贯穿所有页面，可折叠/可拖宽 */}
      <div className={'ai-rail' + (aiOpen ? ' open' : '')} style={aiOpen ? { width: aiWidth } : undefined}>
        <div className="ai-rail-head">
          <span className="ai-rail-title"><MessageSquare size={14} /> {t('shell.airail.title')}</span>
          <button className="ghost icon" onClick={() => setAiOpen(false)} title={t('shell.airail.collapse')}><X size={14} /></button>
        </div>
        <div className="ai-rail-body">
          <ChatPanel onOpenSettings={() => { setSettingsTab('ai'); setShowSettings(true); }} />
        </div>
      </div>
      {aiOpen && <Splitter onDrag={dragAi} />}

      <div className="app-content">
      <header>
        <span className="brand"><Logo size={20} /> DiskPilot</span>
        <div className="seg-viewtabs">
          <button
            className={'seg-item' + (view === 'overview' ? ' active' : '')}
            onClick={() => setView('overview')}
            title={t('shell.nav.overviewTitle')}
          >
            <LayoutGrid size={14} /> {t('shell.nav.overview')}
          </button>
          <button
            className={'seg-item' + (view === 'workspace' ? ' active' : '')}
            onClick={() => setView('workspace')}
            title={t('shell.nav.workspaceTitle')}
          >
            <ListTree size={14} /> {t('shell.nav.workspace')}
          </button>
          <button
            className={'seg-item' + (view === 'tools' ? ' active' : '')}
            onClick={() => setView('tools')}
            title={t('shell.nav.toolsTitle')}
          >
            <Wrench size={14} /> {t('shell.nav.tools')}
          </button>
        </div>
        <div className="grow" />
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
        <button
          className={'ghost icon settings-btn' + (advisorTag ? ' bound' : '')}
          onClick={() => setShowSettings(true)}
          title={advisorTag ? t('shell.nav.settingsBound', { provider: advisorTag.provider }) : t('shell.nav.settingsUnbound')}
        >
          <SettingsIcon size={16} />
          {advisorTag && <span className="settings-dot" />}
        </button>
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
      </header>

      <ScanBar scanning={scanning} ref={scanBarRef} />
      {err && <div className="banner error">{err}</div>}
      {!err && !scanning && diag?.backend?.mode === 'walkdir' && (
        <div className="hint">{t('shell.scan.adminHint')}</div>
      )}

      {view === 'overview' ? (
        <main className="overview">
          <ErrorBoundary fallbackLabel={t('shell.boundary.overviewFailed')}>
            <AssetOverview
              root={root}
              drives={drives}
              scaffolds={scaffolds}
              scanning={scanning}
              onScanDrive={(p) => scan([p])}
              onScanAll={() => drives.length > 0 && scan(drives.map((d) => d.path))}
              onRefresh={(p) => scan([p], { force: true })}
              onGoWorkspace={() => setView('workspace')}
            />
          </ErrorBoundary>
        </main>
      ) : view === 'tools' ? (
        <main className="overview">
          <Toolbelt
            drives={drives}
            scanning={scanning}
            onScanDrive={(p) => scan([p])}
            onScanAll={() => drives.length > 0 && scan(drives.map((d) => d.path))}
            onGoWorkspace={() => setView('workspace')}
            onOpenSettings={(tab) => { setSettingsTab(tab); setShowSettings(true); }}
          />
        </main>
      ) : (
      <main
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
              onScanAll={() => drives.length > 0 && scan(drives.map((d) => d.path))}
              onScanDrive={(p) => scan([p])}
              onRefresh={(p) => scan([p], { force: true })}
            />
          </div>
        )}
        <aside className="left">
          {root ? <LeftPanel root={root} selectedPath={selectedPath} onSelect={select} /> : <EmptyLeft />}
        </aside>

        <Splitter onDrag={dragLeft} onDoubleClick={() => setLeftWidth(DEFAULT_LEFT)} />

        <section className="center">
          <ErrorBoundary fallbackLabel={t('shell.boundary.detailFailed')}>
            <FileDetailPanel />
          </ErrorBoundary>
        </section>

        <Splitter onDrag={dragRight} onDoubleClick={() => setRightWidth(DEFAULT_RIGHT)} />

        {rightVisible && (
        <aside className="right">
          <ErrorBoundary fallbackLabel={t('shell.boundary.studioFailed')}>
            <Studio />
          </ErrorBoundary>
        </aside>
        )}
      </main>
      )}

      <footer>
        <span>{t('shell.footer.rules', { n: scaffolds.length })}</span>
        <span>{root?.path ?? t('shell.footer.noScan')}</span>
      </footer>
      </div>

      {/* AI 侧栏折叠后的浮动唤起钮 */}
      {!aiOpen && (
        <button className="ai-rail-toggle" onClick={() => setAiOpen(true)} title={t('shell.airail.expand')}>
          <MessageSquare size={16} />
        </button>
      )}

      {showSettings && <Settings onClose={() => { setShowSettings(false); refreshAdvisorTag(); }} initialTab={settingsTab} />}
      <CleanupProposalDialog />

      <div className="toast-stack">
        {toasts.map((toast) => (
          <button
            key={toast.id}
            className={'toast ' + toast.kind}
            onClick={() => dismissToast(toast.id)}
          >
            {toast.text}
          </button>
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
            与 TreeView/FileView 同一枚字符串，按 i18n 不译清单保持中文。 */}
        <button className="ghost" onClick={() => { void recycle([{ path: node.path, size_hint: node.size }], '手动回收'); }} disabled={!node.is_dir}>{/* @i18n-keep */}
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
    </div>
  );
}

// ── 扫描进度条 ──
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

  useImperativeHandle(ref, () => ({
    begin: (targets: string[]) => {
      // 新一次扫描开始：清空上一轮进度与分母，重新触发盘根总量探针。
      setProgress(null);
      setTotalBytes(null);
      setPathLabel(null);
      probeDriveTotals(targets, setTotalBytes);
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
            className={'scan-bar-fill' + (complete ? ' determinate' : ' indeterminate')}
            style={complete ? { width: `${Math.min(99, (complete.progress.bytes / complete.totalBytes) * 100)}%` } : undefined}
          />
          <div className="scan-bar-label" title={pathLabel ?? undefined}>
            {progress
              ? `${t('shell.scanbar.progress', { files: progress.files.toLocaleString(), bytes: formatBytes(progress.bytes) })}${pathLabel ? ` · ${ellipsizePath(pathLabel)}` : ''}`
              : t('shell.scanbar.preparing')}
          </div>
        </div>
      )}
      {!scanning && progress && (
        <div className="scan-bar">
          <div className="scan-bar-fill determinate" style={{ width: '100%' }} />
          <div className="scan-bar-label" title={pathLabel ?? undefined}>
            {`${t('shell.scanbar.done', { files: progress.files.toLocaleString(), bytes: formatBytes(progress.bytes) })}${pathLabel ? ` · ${ellipsizePath(pathLabel)}` : ''}`}
          </div>
        </div>
      )}
    </>
  );
});
