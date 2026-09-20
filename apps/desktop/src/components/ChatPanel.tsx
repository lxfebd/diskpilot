import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { Send, Trash2, ShieldCheck, ShieldAlert, ShieldX, X, MessageSquare, Sparkles, Folder, File, ImagePlus, Plus, Terminal, Square } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { api } from '../api';
import { RISK_COLORS } from '../colors';
import type { HwInfo } from '../api';
import { isTauri } from '../env';
import { useStore, type TraceItem, type ChatTurn } from '../store';
import { formatBytes } from '../format';
import { agentChat, overviewChat, resolveCliTool, resolveStress, resolveSessionConfirm, type TraceEvent, type PendingCliTool, type PendingSessionConfirm } from '../advisorClient';
import { grantSessionAuth, clearSessionAuths, PERMS } from '../permissions';
import { deriveChatHistory } from '../advisor/chatHistory';
import { lastToolCall, runningSecs } from '../advisor/traceTimer';
import { prefs } from '../theme';
import type { Node, AdvisorResponse, Scaffold } from '../types';
import { HwReportCard, StressConfirmPanel, HwTestMonitor, useHwTestMonitor, type StressConfirm } from './HwPanels';
import { useT } from '../i18n';
import { isConfigured, loadSettings } from '../advisorClient';

function uid() {
  return Math.random().toString(36).slice(2);
}

// 权限 id → 展示名文案键。查不到定义时退回 id 本身（t() 未命中键会原样返回）。
const permNameKey = (id: string) => PERMS.find((p) => p.id === id)?.labelKey ?? id;

// 已自动触发过 AI 总览的扫描（root.path + scanSeq）。放在模块级而不是
// useRef：切页会让 ChatPanel 卸载重挂载、ref 归零，导致同一次扫描
// 每切回一次页面就重新生成一遍总览。
let analyzedScanKey: string | null = null;

function findNodeByPath(root: Node | null, path: string): Node | null {
  if (!root) return null;
  if (root.path === path) return root;
  for (const c of root.children) {
    const f = findNodeByPath(c, path);
    if (f) return f;
  }
  return null;
}

function buildOverviewSummary(root: Node) {
  const flatten = (n: Node, depth: number, out: { path: string; name: string; size: number; depth: number; is_dir: boolean }[]) => {
    if (depth > 0) {
      out.push({ path: n.path, name: n.name, size: n.size, depth, is_dir: n.is_dir });
    }
    if (depth < 2) {
      for (const c of n.children ?? []) flatten(c, depth + 1, out);
    }
  };
  const all: { path: string; name: string; size: number; depth: number; is_dir: boolean }[] = [];
  flatten(root, 0, all);
  all.sort((a, b) => b.size - a.size);
  const top = all.slice(0, 25).map((x) => ({
    path: x.path,
    name: x.name,
    size_human: formatBytes(x.size),
    size_bytes: x.size,
    depth: x.depth,
    kind: x.is_dir ? 'dir' : 'file',
  }));
  return {
    root: root.path,
    total_size_human: formatBytes(root.size),
    total_files: root.file_count,
    top_entries: top,
  };
}

export function ChatPanel({ onOpenSettings }: { onOpenSettings?: () => void }) {
  const t = useT();
  // AI 配置态：设置页保存后无需重挂载即恢复显示正常空态。
  const [aiReady, setAiReady] = useState(() => isConfigured(loadSettings()));
  const refreshAiReady = () => setAiReady(isConfigured(loadSettings()));
  useEffect(() => {
    refreshAiReady();
    const onFocus = () => refreshAiReady();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, []);
  const root = useStore((s) => s.root);
  // 拆 selector：只订阅渲染真正需要的字段。整对象订阅（s.chat）会在
  // traceTo/patchChatTurn 每次事件（克隆 turns 数组）时触发 ChatPanel
  // 全量重渲染——AI 多轮下来 turns 越大越卡。拆开后 busy 变化只重渲
  // 底部输入条，turns 变化配合 memo 的 TurnRow 只重渲受影响的气泡。
  const turns = useStore((s) => s.chat.turns);
  const chatNode = useStore((s) => s.chat.node);
  const chatBusy = useStore((s) => s.chat.busy);
  const scanSeq = useStore((s) => s.scanSeq);
  const pushTurn = useStore((s) => s.pushChatTurn);
  const patchTurn = useStore((s) => s.patchChatTurn);
  const setBusy = useStore((s) => s.setChatBusy);
  const resetChat = useStore((s) => s.resetChat);
  const chatSessions = useStore((s) => s.chatSessions);
  const activeChatId = useStore((s) => s.activeChatId);
  const newChat = useStore((s) => s.newChat);
  const switchChat = useStore((s) => s.switchChat);
  const deleteChat = useStore((s) => s.deleteChat);
  const scaffolds = useStore((s) => s.scaffolds);
  const addReclaimed = useStore((s) => s.addReclaimed);
  const studioRequest = useStore((s) => s.studioRequest);
  const consumeStudio = useStore((s) => s.consumeStudio);
  const setProposal = useStore((s) => s.setProposal);

  const [input, setInput] = useState('');
  const [pendingDrops, setPendingDrops] = useState<{ path: string; name: string }[]>([]);
  const [pendingImages, setPendingImages] = useState<{ id: string; name: string; dataUrl: string; mimeType: string }[]>([]);
  const [dropping, setDropping] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  // 待用户确认的 CLI 工具调用（图吧工具箱中/高风险 run_cli_tool）。
  const [pendingCli, setPendingCli] = useState<PendingCliTool | null>(null);
  const [pendingSession, setPendingSession] = useState<PendingSessionConfirm | null>(null);
  // 待用户确认的硬件压测（run_hardware_test，L1 受控）。
  const [pendingStress, setPendingStress] = useState<StressConfirm | null>(null);
  // 待用户两步确认的「回收此项」（AI 建议卡上的回收按钮 —— 点一下只挂确认面板，
  // 面板里再点「确认回收」才真正落盘）。
  const [pendingRecycle, setPendingRecycle] = useState<{ path: string; name: string; size: number; reason: string } | null>(null);
  // AI 调用 get_hardware_info / analyze_disk_health 时的结构化数据，渲染为卡片。
  const [hwCard, setHwCard] = useState<{ info: HwInfo | null; health: import('../api').HwDiskHealth | null } | null>(null);
  // 当前进行中 AI 请求的取消控制器（beginReq 创建、endReq 清空）：停止按钮
  // abort 它 → agentChat 的 o.signal 触发 → 浏览器分支掐 fetch / Tauri 分支
  // 调 ai_cancel 掐后端。
  const cancelRef = useRef<AbortController | null>(null);
  const stopAi = () => {
    if (inflight.current > 0 && cancelRef.current) {
      cancelRef.current.abort();
      // 通知后端停止正在跑的压测（stress_test / stress_test_gpu）：AI 可能在
      // 对话里启动了长压测，停止按钮一并掐掉。stress_cancel 写跨进程停止文件
      // （%TEMP%/diskpilot-stress-stop.flag），压测进程（哪怕不是本次调用 spawn
      // 的那个实例）监控循环检测到即退出——跨进程天然有效。
      void api.callTool?.('stress_cancel', {}).catch(() => {});
    }
  };
  // 把异常渲染成友好文案：用户主动停止（abort/已取消）不算「调用失败」。
  const errText = (e: unknown) => {
    const s = String(e);
    return /已取消|abort|AbortError|user aborted/i.test(s) // @i18n-keep 判定用的字符串常量，不随界面语言变
      ? t('chat.err.stopped')
      : t('chat.err.callFailed', { msg: s });
  };
  // 进行中的 AI 请求计数：多请求并发时，只有最后一个请求收尾才把
  // chat.busy 置 false，避免旧请求先回、提前清掉新请求的 busy 状态。
  const inflight = useRef(0);
  const beginReq = () => {
    inflight.current += 1;
    // 第一个请求开始时创建取消控制器；全部收尾后（endReq 置 0）清空。
    if (inflight.current === 1) cancelRef.current = new AbortController();
    setBusy(true);
  };
  const endReq = () => {
    inflight.current = Math.max(0, inflight.current - 1);
    if (inflight.current === 0) {
      setBusy(false);
      cancelRef.current = null;
    }
  };

  // 硬件压测监控条：后端每 2 秒推 hw-test-progress 事件，用户可随时停止。
  const monitor = useHwTestMonitor(() => setPendingStress(null));

  const node = chatNode;

  // 把 agent 的流式 trace 事件（思考 / 工具调用 / 结果）沉淀到指定 assistant turn。
  const traceTo = (turnId: string) => (e: TraceEvent) => {
    const cur = useStore.getState().chat.turns.find((x) => x.id === turnId);
    const bubble =
      e.kind === 'tool_call' ? t('chat.trace.toolRunning', { text: e.text.replace(/^调用\s*/, '') }) // @i18n-keep 剥后端事件文本的前缀，匹配用常量
      : e.kind === 'tool_result' ? t('chat.trace.analyzing')
      : e.kind === 'thinking' ? t('chat.trace.thinking')
      : cur?.text ?? t('chat.trace.thinkingShort');
    // 追加不得超过 200 条：长对话的思考/工具事件会无限增长，每次又是
    // 整段拷贝，AI 多轮下来消息体越来越重、UI 越来越卡。
    const prev = cur?.trace ?? [];
    const next = prev.length >= 200 ? prev.slice(prev.length - 199) : prev;
    patchTurn(turnId, {
      trace: [...next, { kind: e.kind, text: e.text, detail: e.detail, ts: Date.now() }],
      text: bubble,
      pending: true,
    });
  };

  // 多轮上下文：从完整会话 turns 派生（事件源式），而不是只 slice 最近 N 条。
  // 已完成（非 pending）的 user/assistant 回合全量参与；工具过程压成一行概要；
  // 超预算时最老的完整回合压成「早前…」摘要，最近的 8 条永远完整保留。
  // 每条最前面注入当前扫描状态，让 AI 每轮都知道正在看的路径/规模，
  // 追问（"C 盘哪里最大"、"这个能删吗"）不必重复交代背景。
  const chatHistory = (): { role: 'user' | 'assistant'; content: string }[] => {
    const derived = deriveChatHistory(turns, { budget: 6000, maxTail: 8 });
    const scanLine = root
      ? `（当前状态：正分析 ${root.path}，共 ${formatBytes(root.size)}、${root.file_count.toLocaleString()} 个文件。可拖入其他路径或点选左侧节点切换话题。）` // @i18n-keep 发给模型的状态注入行，属 prompt 不属文案
      : '';
    return scanLine ? [{ role: 'user', content: scanLine }, ...derived] : derived;
  };

  // Auto-fire overview the moment scan finishes, once per scan.
  // key 里带 scanSeq：同一路径被强制重扫（scanSeq 递增）时允许重新分析；
  // 纯切页（root.path/scanSeq 都不变）则跳过，不再重复生成总览。
  useEffect(() => {
    if (!root) return;
    const key = `${root.path}:${scanSeq}`;
    if (analyzedScanKey === key) return;
    analyzedScanKey = key;
    if (!prefs.autoOverview) return;
    runOverview(root);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [root?.path, scanSeq]);

  // No more auto-advice on node change — the user runs ONE conversation,
  // dropped items become pending pills until they send.

  // Handle Studio card clicks — synthesize a prompt about the scaffold.
  useEffect(() => {
    if (!studioRequest) return;
    const sc = scaffolds.find((s) => s.id === studioRequest.scaffoldId);
    consumeStudio();
    if (!sc) return;
    runStudioPrompt(sc);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [studioRequest?.ts]);

  useEffect(() => {
    scrollerRef.current?.scrollTo({ top: scrollerRef.current.scrollHeight, behavior: 'smooth' });
  }, [turns.length, chatBusy]);

  // Walk root, collect ALL nodes tagged with this scaffold. Mirrors Studio's
  // findAllMatchesByScaffold — a scaffold legitimately matches in multiple
  // places (e.g. wechat-pc hits Documents\WeChat Files + AppData\Roaming\Tencent\WeChat
  // + ProgramData\Tencent\WeChat etc.). We don't recurse into a subtree that
  // already matched, so each match is the topmost root of its scaffold-tagged
  // region — sizes/children are disjoint by construction.
  const findAllScaffoldNodes = (sc: Scaffold): Node[] => {
    if (!root) return [];
    const out: Node[] = [];
    const dfs = (n: Node) => {
      if (n.scaffold_id === sc.id) { out.push(n); return; }
      for (const c of n.children ?? []) dfs(c);
    };
    dfs(root);
    out.sort((a, b) => b.size - a.size);
    return out;
  };

  const runStudioPrompt = async (sc: Scaffold) => {
    const matches = findAllScaffoldNodes(sc);
    const totalSize = matches.reduce((s, m) => s + m.size, 0);
    const totalFiles = matches.reduce((s, m) => s + m.file_count, 0);

    // Phrase the question from the user's POV: they're looking at the right-
    // side card and asking "what's actually in this thing across ALL matched
    // locations". Important: don't pick a single path — wechat-pc has 5
    // legitimate locations, and earlier we were sending only the first DFS hit
    // which was sometimes the empty Temp\WeChat Files folder.
    const userText = matches.length === 0
      ? `右侧的【${sc.name}】这次扫描里没扫到。它一般会在哪些路径下？里面通常存什么？` // @i18n-keep 发给模型的提问（prompt）
      : matches.length === 1
        ? `右侧显示扫描里检测到了【${sc.name}】(${formatBytes(totalSize)}, \`${matches[0].path}\`)。这个文件夹里具体都是什么？哪些是可以删的？` // @i18n-keep 发给模型的提问（prompt）
        : `右侧扫描里检测到了【${sc.name}】，分布在 ${matches.length} 个位置，合计 ${formatBytes(totalSize)} / ${totalFiles.toLocaleString()} 文件：\n${matches.map((m) => `- \`${m.path}\` (${formatBytes(m.size)})`).join('\n')}\n\n这些文件夹各自都是什么？哪些是可以删的？`; // @i18n-keep 发给模型的提问（prompt）
    pushTurn({ id: uid(), role: 'user', text: userText });

    beginReq();
    const turnId = uid();
    pushTurn({ id: turnId, role: 'assistant', text: t('chat.studio.analyzing', { name: sc.name }), pending: true, scaffoldId: sc.id });
    try {
      // Sample each match (cap 8 paths per match → 24-40 total samples for
      // typical 3-5 location scaffolds). Drop the empty roots so the AI doesn't
      // spend tokens on "and there's an empty folder too".
      const nonEmpty = matches.filter((m) => m.size > 0 || (m.children?.length ?? 0) > 0);
      const sampledMatches = await Promise.all(
        nonEmpty.map(async (m) => {
          const samples = m.is_dir
            ? await api.inspect(m.path, 8).catch(() => [] as string[])
            : [];
          return {
            path: m.path,
            size: formatBytes(m.size),
            file_count: m.file_count,
            top_extensions: (m.top_extensions ?? []).slice(0, 5),
            top_children: (m.children ?? []).slice(0, 8).map((c) => ({
              name: c.name,
              size: formatBytes(c.size),
              is_dir: c.is_dir,
            })),
            sample_paths: samples,
          };
        }),
      );
      const ctx = {
        app: sc.name,
        scaffold_id: sc.id,
        risk: sc.risk,
        disclaimer: sc.disclaimer,
        declared_paths: sc.detect,
        cleanable_scopes: sc.scopes.map((s) => ({ id: s.id, label: s.label, mode: s.mode, glob: s.glob })),
        scanned_matches: sampledMatches,
        scanned_total: matches.length > 0
          ? { location_count: matches.length, total_size: formatBytes(totalSize), total_files: totalFiles }
          : null,
      };
      const reply = await agentChat({
        user: `用户在 Studio 里点了【${sc.name}】这张卡片。下面是这个清理脚本的元数据，以及本次扫描中匹配到的所有位置（每个位置含 top children + 抽样路径）。请按位置分别说明里面是什么、哪些可以删、用什么方式删——不要只挑一个位置说：\n${JSON.stringify(ctx, null, 2)}\n\n用户的问题：${userText}`, // @i18n-keep 发给模型的指令（prompt）
        history: chatHistory(),
        signal: cancelRef.current?.signal,
        onEvent: traceTo(turnId),
        onProposal: (p) => setProposal({ id: uid(), title: p.title, items: p.items.map((i) => ({ ...i, size_bytes: null })) }),
        onToolConfirm: (tool) => setPendingCli(tool),
        onHwCard: (card) => setHwCard(card),
        onStressConfirm: (req) => setPendingStress(req),
        onSessionConfirm: (req) => setPendingSession(req),
      });
      patchTurn(turnId, { text: reply, pending: false });
    } catch (e) {
      patchTurn(turnId, { text: errText(e), pending: false });
    } finally {
      endReq();
    }
  };

  const runOverview = async (r: Node) => {
    beginReq();
    const turnId = uid();
    pushTurn({
      id: turnId,
      role: 'assistant',
      text: t('chat.overview.scanned', { path: r.path, size: formatBytes(r.size), count: r.file_count.toLocaleString() }),
      pending: true,
    });
    try {
      // 优先走 Rust 侧 chat_scan_context（后端缓存的扫描树直接算 Top 条目 +
      // 可回收字节），只有非 Tauri/后端无树时才回退浏览器侧 JS 遍历。
      const rustCtx = await api.chatScanContext(25);
      let summary;
      if (rustCtx) {
        summary = {
          root: rustCtx.root,
          total_size_human: formatBytes(rustCtx.total_size),
          total_files: rustCtx.total_files,
          reclaimable_bytes_human: formatBytes(rustCtx.reclaimable_bytes),
          top_entries: rustCtx.top_entries.map((e) => ({
            path: e.path,
            name: e.name,
            size_human: formatBytes(e.size),
            size_bytes: e.size,
            depth: e.depth,
            kind: e.is_dir ? 'dir' : 'file',
          })),
        };
      } else {
        summary = buildOverviewSummary(r);
      }
      const reply = await overviewChat(summary, cancelRef.current?.signal);
      patchTurn(turnId, { text: reply, pending: false });
    } catch (e) {
      patchTurn(turnId, {
        text: errText(e) + '\n' + t('chat.overview.dragHint'),
        pending: false,
      });
    } finally {
      endReq();
    }
  };

  const askFollowUp = async (presetText?: string) => {
    const text = (presetText ?? input).trim();
    if (!text && pendingDrops.length === 0 && pendingImages.length === 0) return;
    setInput('');
    const drops = pendingDrops.slice();
    const images = pendingImages.slice();
    setPendingDrops([]);
    setPendingImages([]);

    const dropDesc = drops.length > 0 ? t('chat.drop.about', { paths: drops.map((d) => d.path).join('、') }) : '';
    const imgDesc = images.length > 0 ? t('chat.drop.images', { n: images.length }) : '';
    const userText = [text, dropDesc, imgDesc].filter(Boolean).join('\n');
    pushTurn({ id: uid(), role: 'user', text: userText });
    beginReq();
    const turnId = uid();
    pushTurn({ id: turnId, role: 'assistant', text: t('chat.trace.thinkingShort'), pending: true });

    try {
      const targets = drops.length > 0 && root
        ? drops.map((d) => findNodeByPath(root, d.path)).filter(Boolean) as Node[]
        : node ? [node] : [];

      const ctx = targets.map((it) => ({
        path: it.path,
        name: it.name,
        size: formatBytes(it.size),
        is_dir: it.is_dir,
        file_count: it.file_count,
        top_extensions: (it.top_extensions ?? []).slice(0, 6),
        sample_children: (it.children ?? []).slice(0, 8).map((c) => ({ name: c.name, size: formatBytes(c.size), is_dir: c.is_dir })),
      }));
      const contextLine = ctx.length > 0 ? `目标对象：${JSON.stringify(ctx, null, 2)}` : ''; // @i18n-keep 发给模型的 prompt 上下文行
      const question = text || (images.length > 0 ? '看看这张图，告诉我是什么、能不能删。' : '这些是什么？能不能删？'); // @i18n-keep 发给模型的默认提问
      const reply = await agentChat({
        user: contextLine ? `${contextLine}\n\n用户的问题：${question}` : `用户的问题：${question}`, // @i18n-keep 发给模型的提问包装
        // 多轮上下文：把已完成（非 pending）的 user/assistant 回合回放给 AI，
        // 让它记得前面对话（"刚才那个文件夹/上面说的清理方式"）。只取最近
        // 30 条、且跳过系统/硬件卡片回合，避免 context 无限膨胀。
        history: chatHistory(),
        signal: cancelRef.current?.signal,
        images: images.length > 0 ? images.map((i) => ({ dataUrl: i.dataUrl, mimeType: i.mimeType })) : undefined,
        onEvent: traceTo(turnId),
        onProposal: (p) => setProposal({ id: uid(), title: p.title, items: p.items.map((i) => ({ ...i, size_bytes: null })) }),
        onToolConfirm: (tool) => setPendingCli(tool),
        onHwCard: (card) => setHwCard(card),
        onStressConfirm: (req) => setPendingStress(req),
        onSessionConfirm: (req) => setPendingSession(req),
      });
      patchTurn(turnId, { text: reply, pending: false });
    } catch (e) {
      patchTurn(turnId, { text: errText(e), pending: false });
    } finally {
      endReq();
    }
  };

  // 快捷入口：不要求先扫磁盘，直接让 AI 干一件事（看配置/看健康/自我介绍）。
  const quickAsk = (text: string) => {
    if (chatBusy) return;
    // 未配置 AI 时不发起调用（后端会报「AI 未配置」）——弹设置引导而不是给一条错误气泡。
    if (!isConfigured(loadSettings())) {
      onOpenSettings?.();
      return;
    }
    askFollowUp(text);
  };

  const fileToDataUrl = (file: File) =>
    new Promise<string>((resolve, reject) => {
      const r = new FileReader();
      r.onerror = () => reject(r.error);
      r.onload = () => resolve(r.result as string);
      r.readAsDataURL(file);
    });

  const addImageFile = async (file: File) => {
    if (!file.type.startsWith('image/')) return;
    if (file.size > 20 * 1024 * 1024) {
      pushTurn({ id: uid(), role: 'system', text: t('chat.sys.imageTooBig', { name: file.name }) });
      return;
    }
    try {
      const dataUrl = await fileToDataUrl(file);
      setPendingImages((prev) => [
        ...prev,
        { id: uid(), name: file.name || 'image', dataUrl, mimeType: file.type || 'image/png' },
      ]);
    } catch (e) {
      pushTurn({ id: uid(), role: 'system', text: t('chat.sys.imageReadFailed', { msg: String(e) }) });
    }
  };

  const onPaste = async (e: React.ClipboardEvent) => {
    const items = e.clipboardData?.items;
    if (!items) return;
    for (const it of items) {
      if (it.kind === 'file') {
        const f = it.getAsFile();
        if (f && f.type.startsWith('image/')) {
          e.preventDefault();
          await addImageFile(f);
        }
      }
    }
  };

  const onDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    setDropping(false);
    // Image file drop (from filesystem / browser): take precedence over the
    // internal DiskPilot path drop, since paths come with our custom mime type.
    const files = Array.from(e.dataTransfer.files ?? []).filter((f) => f.type.startsWith('image/'));
    if (files.length > 0) {
      for (const f of files) await addImageFile(f);
      return;
    }
    const path = e.dataTransfer.getData('application/x-diskpilot-path');
    const name = e.dataTransfer.getData('application/x-diskpilot-name') || path.split(/[\\/]/).pop() || path;
    if (!path) return;
    // Just stage a pending pill — do NOT reset the conversation or refocus the
    // chat node. The user keeps one continuous conversation and asks across
    // multiple dropped items.
    setPendingDrops((prev) => (prev.find((p) => p.path === path) ? prev : [...prev, { path, name }]));
  };

  // 回收单个节点：TurnRow 的「回收」按钮只负责挂出确认面板（两步确认，铁律：
  // 破坏性动作禁一步即执行），真正执行在 confirmRecycle 里——用户点「确认回收」
  // 后才带 confirmed=true 调后端 recycle_paths。useCallback 稳定引用，配合
  // memo 的 TurnRow 避免每次 patch turn（AI 流式事件）都触发所有气泡重渲染。
  const recycleNode = useCallback((target: Node, reason: string) => {
    setPendingRecycle({ path: target.path, name: target.name, size: target.size, reason });
  }, []);

  const confirmRecycle = useCallback(async (run: boolean) => {
    const target = pendingRecycle;
    setPendingRecycle(null);
    if (!target || !run) return;
    try {
      await api.recyclePaths([target.path], target.reason, true);
      addReclaimed(target.size);
      pushTurn({ id: uid(), role: 'system', text: t('chat.sys.recycled', { path: target.path, size: formatBytes(target.size) }) });
    } catch (e) {
      pushTurn({ id: uid(), role: 'system', text: t('chat.sys.recycleFailed', { msg: String(e) }) });
    }
  }, [addReclaimed, pendingRecycle, pushTurn, t]);

  const empty = turns.length === 0;

  // 确认 CLI 工具（图吧工具箱中/高风险 run_cli_tool）：回填 resolveCliTool()
  // 让 agentChat 的工具调用链继续往下走——真实执行结果会作为下一轮的
  // tool_result 回给模型，由模型总结。勾选「本次会话免确认」后，会话期内
  // 其它中/高风险工具不再弹确认卡。与 confirmStress / confirmSession 同模式。
  const [cliRemember, setCliRemember] = useState(false);
  const confirmCliRun = (action: { run: boolean; remember: boolean }) => {
    if (action.remember) grantSessionAuth('cli.run');
    setPendingCli(null);
    setCliRemember(false);
    resolveCliTool(action);
  };

  // 确认硬件压测（L1 受控）：用户在确认面板里点「确认」后，回填 resolveStress()
  // 让 agentChat 的工具调用链继续往下走（工具结果会作为下一轮的 tool_result
  // 回给模型，由模型总结）。监控条由后端 hw-test-progress 事件驱动。
  const confirmStress = (action: { run: boolean; remember: boolean }) => {
    setPendingStress(null);
    resolveStress(action);
  };

  // 确认 L2 会话操作（插件管理 / 电源计划等）：回填 resolveSessionConfirm()，
  // agent 继续执行并把结果回填给模型。
  const [sessionRemember, setSessionRemember] = useState(false);
  const confirmSession = (action: { run: boolean; remember: boolean }) => {
    setPendingSession(null);
    setSessionRemember(false);
    resolveSessionConfirm(action);
  };

  return (
    <div
      className={'chat' + (dropping ? ' drop-target' : '')}
      onDragOver={(e) => { e.preventDefault(); setDropping(true); }}
      onDragLeave={() => setDropping(false)}
      onDrop={onDrop}
    >
      {/* 会话列表（多会话）：历史对话切换 / 新建。只显示有历史会话时，
          保持 AI 栏紧凑；点「+ 新对话」归档当前会话并开新会话。 */}
      {chatSessions.length > 0 && (
        <div className="chat-sessions">
          <div className="chat-sessions-head">
            <span className="muted small">{t('chat.sessions.head')}</span>
            <button className="ghost icon" onClick={() => { clearSessionAuths(); newChat(); }} title={t('chat.sessions.newTitle')}><Plus size={13} /></button>
          </div>
          <div className="chat-sessions-list">
            {chatSessions.map((sess) => (
              <div
                key={sess.id}
                className={'chat-session-item' + (sess.id === activeChatId ? ' active' : '')}
                onClick={() => { clearSessionAuths(); switchChat(sess.id); }}
                title={sess.title}
              >
                <MessageSquare size={11} className="chat-session-icon" />
                <span className="chat-session-title">{sess.title || t('chat.sessions.untitled')}</span>
                <button
                  className="ghost icon chat-session-del"
                  title={t('chat.sessions.deleteTitle')}
                  onClick={(e) => { e.stopPropagation(); deleteChat(sess.id); }}
                >
                  <X size={10} />
                </button>
              </div>
            ))}
          </div>
        </div>
      )}
      <div className="chat-head">
        <Sparkles size={15} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div className="chat-title">
            {root ? (root.name || root.path) : 'DiskPilot AI'}
          </div>
          <div className="chat-sub">
            {root
              ? t('chat.head.stats', { path: root.path, size: formatBytes(root.size), count: root.file_count.toLocaleString() })
              : t('chat.head.idle')}
          </div>
        </div>
        {turns.length > 0 && (
          <button
            className="ghost icon"
            onClick={() => { clearSessionAuths(); resetChat(); }}
            title={t('chat.head.clearTitle')}
          >
            <X size={16} />
          </button>
        )}
      </div>

      <div className="chat-scroll" ref={scrollerRef}>
        {empty && !root && (
          <div className="chat-hero">
            <MessageSquare size={32} />
            <h3>DiskPilot AI</h3>
            {aiReady ? (
              <>
                <p>{t('chat.hero.intro')}</p>
                <div className="chat-quick">
                  {/* quickAsk 的参数是发给模型的提问（prompt），按约定不译；按钮文案才走文案表 */}
                  <button className="ghost" onClick={() => quickAsk('帮我看看这台电脑的整体配置和系统当前状态')}>{t('chat.hero.pcInfo')}</button>{/* @i18n-keep */}
                  <button className="ghost" onClick={() => quickAsk('检查一下我的硬盘健康状态和温度传感器读数')}>{t('chat.hero.hwHealth')}</button>{/* @i18n-keep */}
                  <button className="ghost" onClick={() => quickAsk('你能帮我做哪些事？简单介绍一下')}>{t('chat.hero.capabilities')}</button>{/* @i18n-keep */}
                </div>
              </>
            ) : (
              // AI 未配置：不报错、不隐藏——给小白一条通往设置的引导路径
              <>
                <p>{t('chat.hero.aiNotSetup')}</p>
                <div className="chat-quick">
                  <button className="ghost" onClick={() => quickAsk('帮我看看这台电脑的整体配置和系统当前状态')}>{t('chat.hero.pcInfo')}</button>{/* @i18n-keep */}
                  <button className="ghost" onClick={() => quickAsk('检查一下我的硬盘健康状态和温度传感器读数')}>{t('chat.hero.hwHealth')}</button>{/* @i18n-keep */}
                </div>
                <button className="primary" onClick={onOpenSettings}>{t('chat.hero.aiGoSettings')}</button>
                <p className="muted small">{t('chat.hero.aiNotSetupDesc')}</p>
                <p className="muted small">{t('chat.hero.aiSamples')}</p>
              </>
            )}
            {!isTauri && <p className="muted">{t('chat.hero.browserMode')}</p>}
          </div>
        )}
        {empty && root && (
          <div className="chat-hero">
            <Sparkles size={28} />
            <p>{t('chat.overview.generating')}</p>
          </div>
        )}
        {hwCard && <HwReportCard info={hwCard.info} health={hwCard.health} />}
        {turns.map((turn) => (
          <TurnRow
            key={turn.id}
            turn={turn}
            node={node}
            recycleNode={recycleNode}
          />
        ))}
        {chatBusy && <div className="chat-typing">{t('chat.typing')}</div>}
      </div>

      {pendingCli && (
        <div className="cli-confirm">
          <div className="cli-confirm-head"><Terminal size={14} /> {t('chat.cli.head')}</div>
          <div className="cli-confirm-body">
            <div className="cli-confirm-cmd">
              <code>{pendingCli.tool}</code>
              {pendingCli.args.length > 0 && <code className="cli-confirm-args">{pendingCli.args.join(' ')}</code>}
            </div>
            <div className="cli-confirm-meta">
              {t('chat.cli.riskLabel')} <span className={'cli-risk-' + pendingCli.risk}>{pendingCli.risk}</span>
              {pendingCli.timeoutSecs ? t('chat.cli.timeout', { secs: pendingCli.timeoutSecs }) : ''}
            </div>
            {pendingCli.reason && <div className="cli-confirm-reason">{pendingCli.reason}</div>}
          </div>
          <div className="cli-confirm-actions">
            <label className="cli-confirm-remember" title={t('chat.cli.rememberTitle')}>
              <input type="checkbox" checked={cliRemember} onChange={(e) => setCliRemember(e.target.checked)} />
              {t('chat.confirm.remember')}
            </label>
            <div className="grow" />
            <button className="ghost" onClick={() => confirmCliRun({ run: false, remember: false })}>{t('chat.confirm.cancel')}</button>
            <button className="primary" onClick={() => confirmCliRun({ run: true, remember: cliRemember })}><Terminal size={13} /> {t('chat.confirm.run')}</button>
          </div>
        </div>
      )}
      {pendingStress && (
        <StressConfirmPanel
          t={pendingStress}
          onClose={() => setPendingStress(null)}
          onConfirm={confirmStress}
        />
      )}
      {pendingSession && (
        <div className="cli-confirm">
          <div className="cli-confirm-head"><ShieldCheck size={14} /> {t('chat.session.head', { title: pendingSession.title })}</div>
          <div className="cli-confirm-body">
            <div className="cli-confirm-cmd">
              <code>{pendingSession.permId}</code>
              <code className="cli-confirm-args">{pendingSession.target}</code>
            </div>
            <div className="cli-confirm-meta">
              {t('chat.session.permLabel')} <span className="cli-risk-medium">{t(permNameKey(pendingSession.permId))}</span>{t('chat.session.everyRun')}
            </div>
            <div className="cli-confirm-reason">{pendingSession.detail}</div>
          </div>
          <div className="cli-confirm-actions">
            <label className="cli-confirm-remember" title={t('chat.session.rememberTitle')}>
              <input type="checkbox" checked={sessionRemember} onChange={(e) => setSessionRemember(e.target.checked)} />
              {t('chat.confirm.remember')}
            </label>
            <div className="grow" />
            <button className="ghost" onClick={() => confirmSession({ run: false, remember: false })}>{t('chat.confirm.cancel')}</button>
            <button className="primary" onClick={() => confirmSession({ run: true, remember: sessionRemember })}>
              <ShieldCheck size={13} /> {t('chat.confirm.run')}
            </button>
          </div>
        </div>
      )}

      {pendingRecycle && (
        <div className="cli-confirm">
          <div className="cli-confirm-head"><Trash2 size={14} /> {t('chat.recycle.head')}</div>
          <div className="cli-confirm-body">
            <div className="cli-confirm-cmd">
              <code title={pendingRecycle.path}>{pendingRecycle.name || pendingRecycle.path}</code>
              <code className="cli-confirm-args">{formatBytes(pendingRecycle.size)}</code>
            </div>
            <div className="cli-confirm-meta">
              {t('chat.recycle.pathLabel')} <span className="cli-risk-medium">{pendingRecycle.path}</span>
            </div>
            <div className="cli-confirm-reason">
              {t('chat.recycle.note', { reason: pendingRecycle.reason })}
            </div>
          </div>
          <div className="cli-confirm-actions">
            <div className="grow" />
            <button className="ghost" onClick={() => confirmRecycle(false)}>{t('chat.confirm.cancel')}</button>
            <button className="primary" onClick={() => confirmRecycle(true)}><Trash2 size={13} /> {t('chat.recycle.confirm')}</button>
          </div>
        </div>
      )}

      <div className="chat-input-wrap">
        {monitor.running && (
          <HwTestMonitor progress={monitor.progress} onStop={monitor.stop} onClose={monitor.clear} />
        )}
        {pendingDrops.length > 0 && (
          <div className="chat-pills">
            {pendingDrops.map((d) => (
              <span key={d.path} className="chat-pill" title={d.path}>
                {d.path.endsWith(d.name) && d.path !== d.name ? <Folder size={11} /> : <File size={11} />}
                {d.name}
                <button onClick={() => setPendingDrops((prev) => prev.filter((p) => p.path !== d.path))}><X size={11} /></button>
              </span>
            ))}
          </div>
        )}
        {pendingImages.length > 0 && (
          <div className="chat-image-pills">
            {pendingImages.map((img) => (
              <span key={img.id} className="chat-image-pill" title={img.name}>
                <img src={img.dataUrl} alt={img.name} />
                <button
                  type="button"
                  onClick={() => setPendingImages((prev) => prev.filter((p) => p.id !== img.id))}
                ><X size={11} /></button>
              </span>
            ))}
          </div>
        )}
        <input
          ref={fileInputRef}
          type="file"
          accept="image/*"
          multiple
          style={{ display: 'none' }}
          onChange={async (e) => {
            const files = Array.from(e.target.files ?? []);
            for (const f of files) await addImageFile(f);
            if (fileInputRef.current) fileInputRef.current.value = '';
          }}
        />
        <div className="chat-input">
          <button
            type="button"
            className="ghost icon chat-attach"
            onClick={() => fileInputRef.current?.click()}
            title={t('chat.input.attachTitle')}
            disabled={chatBusy}
          >
            <ImagePlus size={15} />
          </button>
          <textarea
            rows={2}
            placeholder={root ? t('chat.input.placeholderScanned') : t('chat.input.placeholderIdle')}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onInput={(e) => {
              // 兜底：某些输入路径（无障碍 SetValue / IME / 树状输入法）可能只
              // 触发原生 input 而不触发 React onChange，这里同步一次，保证
              // 「有内容才能点发送」的状态判断始终正确。
              const v = (e.target as HTMLTextAreaElement).value;
              if (v !== input) setInput(v);
            }}
            onPaste={onPaste}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                askFollowUp();
              }
            }}
            disabled={chatBusy}
          />
          {chatBusy ? (
            <button className="primary stop" onClick={stopAi} title={t('chat.input.stopTitle')}>
              <Square size={14} /> {t('chat.input.stop')}
            </button>
          ) : (
            <button
              className="primary"
              onClick={() => askFollowUp()}
              disabled={
                (!input.trim() && pendingDrops.length === 0 && pendingImages.length === 0)
              }
            >
              <Send size={14} /> {t('chat.input.send')}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

// 折叠区标签：这里只存文案表 key，渲染时再 t() 取值（模块顶层取文案会把中文
// 烤进首次 import，切换语言不生效）。未知 kind 直显原值（后端语义化字段）。
const TRACE_CHIP: Record<string, string> = {
  thinking: 'chat.chip.thinking',
  tool_call: 'chat.chip.toolCall',
  tool_result: 'chat.chip.toolResult',
  note: 'chat.chip.note',
};

// 思考过程 / 工具调用折叠区：agent 每一步都列在这里，进行中的回答默认展开。
// 长工具（bench_disk / bench_memory / stress_test 等）可能跑数秒到数十秒，
// 期间毫无反馈容易让人以为卡死——这里对「进行中的最后一个 tool_call」显示
// 自包含的运行计时（每秒 tick，不穿透 TurnRow 的 memo 优化）。
function TraceBlock({ trace, live }: { trace: TraceItem[]; live: boolean }) {
  const t = useT();
  // 计时起步点：只在 trace 确实变化（换了工具调用）时更新，避免对象引用
  // 每渲染都变导致 effect 每秒重跑、计时器反复重建；用 ts 判等。
  const prevTs = useRef<number | null>(null);
  const [secs, setSecs] = useState(0);
  const timerRunning = useRef(false);

  const cur = lastToolCall(trace);

  useEffect(() => {
    const wants = live && cur != null;
    if (cur && cur.ts !== prevTs.current) {
      // 新工具调用开始（或首次）：重置起步点与显示
      prevTs.current = cur.ts;
      setSecs(0);
    }
    if (!wants) {
      if (cur == null) prevTs.current = null;
      setSecs(0);
      return undefined;
    }
    if (!timerRunning.current) {
      timerRunning.current = true;
      const timer = window.setInterval(() => {
        setSecs(runningSecs(prevTs.current ?? 0, Date.now()));
      }, 1000);
      return () => {
        timerRunning.current = false;
        window.clearInterval(timer);
      };
    }
    return undefined;
  }, [live, cur, cur?.ts]);

  const runningCall = live && cur ? (secs > 0 ? t('chat.trace.elapsed', { secs }) : t('chat.trace.runningNow')) : null;

  return (
    <details className="chat-trace" open={live}>
      <summary>
        {t('chat.trace.head', { n: trace.length })}{live && !runningCall ? t('chat.trace.inProgress') : ''}
        {runningCall && <span className="chat-trace-live">{runningCall}</span>}
      </summary>
      <div className="trace-list">
        {trace.map((x, i) => (
          <div key={i} className={'trace-item ' + x.kind}>
            <span className="trace-chip">{TRACE_CHIP[x.kind] ? t(TRACE_CHIP[x.kind]) : x.kind}</span>
            <span className="trace-text">{x.text}</span>
            {x.detail && <pre className="trace-detail">{x.detail}</pre>}
          </div>
        ))}
      </div>
    </details>
  );
}

function AdviceCard({ advice }: { advice: AdvisorResponse }) {
  const t = useT();
  const Icon = advice.risk === 'low' ? ShieldCheck : advice.risk === 'medium' ? ShieldAlert : ShieldX;
  const color = RISK_COLORS[advice.risk] ?? RISK_COLORS.high;
  return (
    <div className="advice-pill" style={{ borderColor: color }}>
      <Icon size={14} style={{ color }} />
      <strong>{advice.category}</strong>
      <span className="badge">{advice.action}</span>
      <span className="muted">{t('chat.advice.riskLabel')} {advice.risk}</span>
      {advice.needs_inspection && <span className="badge">{t('chat.advice.needsInspection')}</span>}
    </div>
  );
}

// 单条对话气泡：memo 化后，AI 流式事件（patchTurn 改某一条）只重渲该条，
// 其余气泡（含各自的 ReactMarkdown）不动 —— 长对话不再每事件全量重渲。
// props 全为基本类型 / 稳定引用：turn 对象替换才触发、node 变化才触发、
// recycleNode 是 useCallback 稳定引用。
interface TurnRowProps {
  turn: ChatTurn;
  node: Node | null;
  recycleNode: (target: Node, reason: string) => void;
}

const TurnRow = memo(function TurnRow({ turn, node, recycleNode }: TurnRowProps) {
  const t = useT();
  const showRecycle = turn.role === 'assistant' && turn.advice?.action === 'recycle' && !turn.advice?.needs_inspection && !!node;
  return (
    <div className={'chat-turn ' + turn.role + (turn.pending ? ' pending' : '')}>
      {turn.role === 'assistant' && turn.advice && <AdviceCard advice={turn.advice} />}
      {turn.role === 'assistant' && turn.trace && turn.trace.length > 0 && (
        <TraceBlock trace={turn.trace} live={!!turn.pending} />
      )}
      <div className="chat-bubble">
        {turn.role === 'assistant'
          ? <ReactMarkdown remarkPlugins={[remarkGfm]}>{turn.text}</ReactMarkdown>
          : turn.text}
      </div>
      {showRecycle && node && (
        <div className="chat-actions">
          <button className="primary" onClick={() => recycleNode(node, turn.advice?.reasoning ?? 'AI suggested')}>
            <Trash2 size={13} /> {t('chat.action.recycle', { size: formatBytes(node.size) })}
          </button>
        </div>
      )}
    </div>
  );
});

