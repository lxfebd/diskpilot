import { create } from 'zustand';
import type { Node, Scaffold, AdvisorResponse } from './types';
import type { TraceKind } from './advisorClient';
import { api } from './api';
import type { HwInfo } from './api';
import { pruneMany, treeMayContain } from './tooltree';
import { isPermEnabled } from './permissions';
import { formatBytes } from './format';
import { t } from './i18n';
import { common } from './i18n/namespaces/common';

// 盘符/路径归一化，作为 scanCache 的 key：C:\\ → C:、统一大小写。
function normKey(p: string): string {
  return p.replace(/[\\/]+$/, '').toUpperCase();
}

const CHAT_SESSIONS_KEY = 'diskpilot.chatSessions';
const ACTIVE_CHAT_KEY = 'diskpilot.activeChatId';

// 历史会话持久化：localStorage 存元数据+turns（有容量上限，防无限增长撑爆）。
function loadChatSessions(): ChatSessionMeta[] {
  try {
    const raw = localStorage.getItem(CHAT_SESSIONS_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw) as ChatSessionMeta[];
    return Array.isArray(arr) ? arr.filter((s) => s && typeof s.id === 'string' && Array.isArray(s.turns)) : [];
  } catch {
    return [];
  }
}

function persistChatSessions(list: ChatSessionMeta[]) {
  try {
    // 每会话 turns 最多保留 200 条（与 UI 渲染上限一致），列表最多 20 个
    // 会话，超出丢最旧的——避免 localStorage 被对话历史塞满。
    const capped = list.slice(0, 20).map((s) => ({ ...s, turns: s.turns.slice(-200) }));
    localStorage.setItem(CHAT_SESSIONS_KEY, JSON.stringify(capped));
  } catch {
    /* localStorage 满/不可用时静默丢弃，不影响主流程 */
  }
}

function loadActiveChatId(): string | null {
  try {
    return localStorage.getItem(ACTIVE_CHAT_KEY);
  } catch {
    return null;
  }
}

function persistActiveChatId(id: string | null) {
  try {
    if (id) localStorage.setItem(ACTIVE_CHAT_KEY, id);
    else localStorage.removeItem(ACTIVE_CHAT_KEY);
  } catch {
    /* ignore */
  }
}

// 整盘扫描树在内存里是最大的数据结构，盘符很多（含多分区/虚拟盘/移动盘）
// 时不能无限缓存。用「最近使用」序做 FIFO 淘汰：cacheDrives 写入把 key 提到
// 最前，takeDrive 命中同样提升；超出上限就把最久没用的逐出。
const MAX_CACHED_DRIVES = 6;
const cacheOrder: string[] = [];

function touchCacheKey(key: string) {
  const i = cacheOrder.indexOf(key);
  if (i >= 0) cacheOrder.splice(i, 1);
  cacheOrder.unshift(key);
}

// AI agent 每一步（思考 / 工具调用 / 工具结果 / 提示）沉淀在 turn.trace，
// ChatPanel 渲染成可折叠的「思考过程」区。
export interface TraceItem {
  kind: TraceKind;
  text: string;
  detail?: string;
  ts: number;
}

export interface ChatTurn {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'hw';
  text: string;
  // optional structured advice that goes with the turn
  advice?: AdvisorResponse;
  // optional scaffold suggestion the user can act on inline
  scaffoldId?: string | null;
  // 硬件检测报告卡片：当 AI 调用 get_hardware_info / generate_hw_report 时，
  // 前端把结构化数据挂在这里渲染为可视化卡片，不塞进正文。
  hw?: HwInfo;
  pending?: boolean;
  trace?: TraceItem[];
}

export interface ChatSession {
  // the node currently being discussed in the chat panel
  node: Node | null;
  scaffoldId: string | null;
  turns: ChatTurn[];
  busy: boolean;
}

/** 历史会话条目（多会话列表）：标题取首条 user 消息截断，turns 完整保存。 */
export interface ChatSessionMeta {
  id: string;
  title: string;
  ts: number;
  turns: ChatTurn[];
}

// AI 生成的清理提案：路径由 AI 给出，字节数以执行前的实测为准，
export type CleanupRisk = 'safe' | 'caution' | 'danger';

// AI 清理清单里的单个条目。reason 为旧字段（保留兼容），新 UI 优先用
// 三段式 what/purpose/impact 展示：这是什么 / 干什么用的 / 删了会怎样。
// risk 标注风险等级：safe 可放心清，caution 谨慎，danger 高危需打字「确认」。
export interface CleanupProposalItem {
  path: string;
  reason: string;
  size_bytes: number | null;
  risk: CleanupRisk;
  what: string;
  purpose: string;
  impact: string;
}

export interface CleanupProposal {
  id: string;
  title: string;
  items: CleanupProposalItem[];
}

export interface ToastItem {
  id: string;
  text: string;
  kind: 'ok' | 'err';
}

interface AppState {
  root: Node | null;
  // 按盘符缓存的扫描结果树（key = 大写去尾斜杠的盘符/路径）。全盘扫完后再点
  // 盘符直接秒开缓存树，不重新遍历；回收文件时这里也会被同步剪枝，保证
  // 「删完再点开」不会复活已删文件。
  scanCache: Record<string, Node>;
  // 每次「真实扫描」（非缓存命中打开）递增。ChatPanel 用它区分"同路径的
  // 新一次扫描"与"切页导致的组件重挂载"，避免同一个 root 反复触发 AI 解析。
  scanSeq: number;
  scaffolds: Scaffold[];
  selectedPath: string | null;
  selectedNode: Node | null;
  reclaimedBytes: number;
  chat: ChatSession;
  studioRequest: { scaffoldId: string; ts: number } | null;
  toasts: ToastItem[];
  proposal: CleanupProposal | null;

  setRoot: (n: Node | null) => void;
  bumpScanSeq: () => void;
  cacheDrives: (entries: Record<string, Node>) => void;
  takeDrive: (key: string) => Node | null;
  setScaffolds: (s: Scaffold[]) => void;
  selectPath: (p: string | null, node?: Node | null) => void;
  addReclaimed: (n: number) => void;

  focusChatOn: (node: Node, scaffoldId: string | null) => void;
  pushChatTurn: (t: ChatTurn) => void;
  patchChatTurn: (id: string, patch: Partial<ChatTurn>) => void;
  setChatBusy: (b: boolean) => void;
  resetChat: () => void;
  requestStudio: (scaffoldId: string) => void;
  consumeStudio: () => void;

  // ── 多会话：历史会话列表（持久化）+ 活动会话切换 ──
  chatSessions: ChatSessionMeta[];
  activeChatId: string | null;
  newChat: () => void;
  switchChat: (id: string) => void;
  deleteChat: (id: string) => void;

  toast: (text: string, kind?: 'ok' | 'err') => void;
  dismissToast: (id: string) => void;
  setProposal: (p: CleanupProposal | null) => void;
  recyclePaths: (
    paths: { path: string; size_hint?: number }[],
    reason: string,
  ) => Promise<{ done: number; failed: number; bytes: number }>;
  // AI 清理清单确认后专用：走受控的 executeAiPlan（user_confirmed=true），
  // 成功后同样即时剪树 + 记 reclaimed + toast。只回收用户勾选项。
  aiRecyclePaths: (
    paths: { path: string; size_hint?: number }[],
  ) => Promise<{ done: number; failed: number; bytes: number }>;
}

export const useStore = create<AppState>((set, get) => {
  // 批量回收成功后的即时剪枝：一次 pruneMany 把「已成功的全部路径」同时
  // 从 root 和受影响的缓存树里剪掉，替代旧版「每项 × 每棵缓存树」的全树
  // 重建。root 可能是「全部磁盘」虚拟根（跨盘），无法按前缀过滤，命中即剪；
  // 缓存树只剪真正可能包含目标的那几棵（通常就一两个盘）。
  const applyRecycledPrune = (succeeded: string[]) => {
    if (succeeded.length === 0) return;
    const st = get();
    const root = st.root;
    const rootAffected =
      root === null ||
      // 多盘合并扫描的虚拟根（App.tsx 写死的 path，数据字面量，恒取中文）
      root.path === common.zh['common.data.allDrivesRoot'] ||
      succeeded.some((p) => treeMayContain(root.path, p));
    let cacheChanged = false;
    const prunedCache: Record<string, Node> = {};
    for (const [k, v] of Object.entries(st.scanCache)) {
      if (succeeded.some((p) => treeMayContain(k, p))) {
        prunedCache[k] = pruneMany(v, succeeded);
        cacheChanged = true;
      } else {
        prunedCache[k] = v;
      }
    }
    set({
      root: root && rootAffected ? pruneMany(root, succeeded) : root,
      scanCache: cacheChanged ? prunedCache : st.scanCache,
      selectedPath: null,
      selectedNode: null,
    });
  };

  return {
  root: null,
  scanCache: {},
  scanSeq: 0,
  scaffolds: [],
  selectedPath: null,
  selectedNode: null,
  reclaimedBytes: 0,
  // 启动时恢复上次活动会话（turns 同步）；无则空会话
  chat: (() => {
    const saved = loadChatSessions();
    const active = loadActiveChatId();
    const hit = active ? saved.find((s) => s.id === active) : undefined;
    return hit
      ? { node: null, scaffoldId: null, turns: hit.turns, busy: false }
      : { node: null, scaffoldId: null, turns: [], busy: false };
  })(),
  chatSessions: loadChatSessions(),
  activeChatId: loadActiveChatId(),
  studioRequest: null,
  toasts: [],
  proposal: null,

  setRoot: (root) => set({ root }),
  bumpScanSeq: () => set((s) => ({ scanSeq: s.scanSeq + 1 })),
  cacheDrives: (entries) =>
    set((s) => {
      const next = { ...s.scanCache, ...entries };
      for (const k of Object.keys(entries)) touchCacheKey(k);
      while (cacheOrder.length > MAX_CACHED_DRIVES) {
        const evict = cacheOrder.pop();
        if (evict !== undefined) delete next[evict];
      }
      return { scanCache: next };
    }),
  takeDrive: (key) => {
    const k = normKey(key);
    const node = get().scanCache[k] ?? null;
    // 命中说明用户刚打开过这棵树，把它提到缓存序最前，留更久。
    if (node) touchCacheKey(k);
    return node;
  },
  setScaffolds: (scaffolds) => set({ scaffolds }),
  selectPath: (selectedPath, node) =>
    // 带 node 引用时整树 DFS 免了：FileDetailPanel 直接读 selectedNode。
    // 只传 path（无 node）的旧调用点仍兼容——它们多来自 FileView（path
    // 可能来自截断树/过滤结果，节点引用不在 root 上，回退查找由组件自己兜）。
    // node ?? null：path-only 选中必须把旧引用清成 null（undefined 会破掉
    // "selectedNode is Node | null" 这一不变量，切选后可能读到上一路径的节点）。
    set({ selectedPath, selectedNode: node ?? null }),
  addReclaimed: (n) => set((s) => ({ reclaimedBytes: s.reclaimedBytes + n })),

  toast: (text, kind = 'ok') => {
    const id = Math.random().toString(36).slice(2);
    set((s) => ({ toasts: [...s.toasts, { id, text, kind }] }));
    setTimeout(() => get().dismissToast(id), 5000);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
  setProposal: (proposal) => set({ proposal }),

  // 批量移入回收站：逐个执行（后端 recycle_paths，强制 Recycle + undo.jsonl），
  // 成功的节点即时从当前树里剪掉，并按同样规则从按盘缓存里剪掉——删完
  // 立刻看得见，且点盘符缓存命中时不会复活已删文件。
  recyclePaths: async (paths, reason) => {
    let done = 0;
    let failed = 0;
    let bytes = 0;
    const succeeded: string[] = [];
    for (const p of paths) {
      try {
        await api.recyclePaths([p.path], reason, true);
        done += 1;
        bytes += p.size_hint ?? 0;
        succeeded.push(p.path);
      } catch {
        failed += 1;
      }
    }
    // 全部执行完再一次性剪树：applyRecycledPrune 本身就是批量语义
    // （一次 pruneMany 处理全部成功路径），循环内每项调一次等于
    // 每项都把 root + 受影响缓存树全走一遍，N 项就是 N 次全树重建。
    if (succeeded.length > 0) applyRecycledPrune(succeeded);
    if (done > 0) {
      get().addReclaimed(bytes);
      get().toast(t('common.recycle.done', { n: done, size: formatBytes(bytes) }), 'ok');
    }
    if (failed > 0) {
      get().toast(t('errors.recycleFailed', { n: failed }), 'err');
    }
    return { done, failed, bytes };
  },

  aiRecyclePaths: async (paths) => {
    // 权限门（前端唯一清理执行入口）：cleanup.execute 未开启 → 拒绝并指引
    // 到权限中心。确认窗本身不弹权限提示，这里必须拦截。
    if (!isPermEnabled('cleanup.execute')) {
      get().toast(t('errors.permExecuteMissing'), 'err');
      return { done: 0, failed: paths.length, bytes: 0 };
    }
    let done = 0;
    let failed = 0;
    let bytes = 0;
    const succeeded: string[] = [];
    // 整批优先走一次 executeAiPlan（user_confirmed=true；后端二次校验 + 强制
    // 回收站）：先 dry-run 探测，整批都可执行才批量——避免批量执行中途一条
    // 失败就整体中止、其余项静默没动（后端 execute 是 `?` 提前返回语义）。
    const batch = paths.map((p) => p.path);
    try {
      await api.executeAiPlan(batch, true, true);
      await api.executeAiPlan(batch, true, false);
      done = paths.length;
      bytes = paths.reduce((s, p) => s + (p.size_hint ?? 0), 0);
      succeeded.push(...batch);
    } catch {
      // 批量不可行（含 dry-run 拒绝 / 保护路径命中）：回退逐项，保持
      // 「失败的不计入、其余照常」的既有语义，已成功的仍一次性剪树。
      for (const p of paths) {
        try {
          // 用户已在确认窗逐项确认 → user_confirmed=true；后端二次校验 + 强制回收站。
          await api.executeAiPlan([p.path], true, false);
          done += 1;
          bytes += p.size_hint ?? 0;
          succeeded.push(p.path);
        } catch {
          failed += 1;
        }
      }
    }
    // 全部执行完一次性剪树，避免 N 次全树重建。
    if (succeeded.length > 0) applyRecycledPrune(succeeded);
    if (done > 0) {
      get().addReclaimed(bytes);
      get().toast(t('common.recycle.aiDone', { n: done, size: formatBytes(bytes) }), 'ok');
    }
    if (failed > 0) {
      get().toast(t('errors.recycleFailed', { n: failed }), 'err');
    }
    return { done, failed, bytes };
  },

  focusChatOn: (node, scaffoldId) =>
    // Keep prior turns — the user wants ONE running conversation. We just
    // update the "focused" node/scaffold so any inline scaffold chips line
    // up with whatever was most recently dropped.
    set((s) => ({ chat: { ...s.chat, node, scaffoldId } })),
  pushChatTurn: (t) => set((s) => ({ chat: { ...s.chat, turns: [...s.chat.turns, t] } })),
  patchChatTurn: (id, patch) =>
    set((s) => ({
      chat: {
        ...s.chat,
        turns: s.chat.turns.map((t) => (t.id === id ? { ...t, ...patch } : t)),
      },
    })),
  setChatBusy: (b) => set((s) => ({ chat: { ...s.chat, busy: b } })),
  resetChat: () =>
    set((s) => {
      // 清空活动会话：同时从列表移除对应条目，避免切走再切回「复活」旧对话。
      if (!s.activeChatId) return { chat: { node: null, scaffoldId: null, turns: [], busy: false } };
      const list = s.chatSessions.filter((x) => x.id !== s.activeChatId);
      persistChatSessions(list);
      return { chatSessions: list, chat: { node: null, scaffoldId: null, turns: [], busy: false } };
    }),
  requestStudio: (scaffoldId) => set({ studioRequest: { scaffoldId, ts: Date.now() } }),
  consumeStudio: () => set({ studioRequest: null }),

  // ── 多会话 ──
  // 活动会话的 turns 改动是 ChatPanel 高频路径，这里不逐条同步到
  // chatSessions（避免每次 set 都重写 localStorage）；只在新会话/切换/
  // 删除时落盘。chat.turns 本身是活动会话的实时镜像。
  newChat: () =>
    set((s) => {
      const id = `chat_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 7)}`;
      // 把当前活动会话（非空）归档进列表
      let list = s.chatSessions;
      if (s.chat.turns.length > 0) {
        const title = s.chat.turns.find((turn) => turn.role === 'user')?.text.replace(/\s+/g, ' ').slice(0, 24) || t('common.chat.untitled');
        const meta: ChatSessionMeta = { id: s.activeChatId ?? id, title, ts: Date.now(), turns: s.chat.turns };
        list = [meta, ...list.filter((x) => x.id !== meta.id)];
      }
      persistChatSessions(list);
      persistActiveChatId(id);
      return { chatSessions: list, activeChatId: id, chat: { node: null, scaffoldId: null, turns: [], busy: false } };
    }),
  switchChat: (id) =>
    set((s) => {
      const meta = s.chatSessions.find((x) => x.id === id);
      if (!meta) return {};
      persistActiveChatId(id);
      return {
        activeChatId: id,
        chat: { node: null, scaffoldId: null, turns: meta.turns, busy: false },
      };
    }),
  deleteChat: (id) =>
    set((s) => {
      const list = s.chatSessions.filter((x) => x.id !== id);
      persistChatSessions(list);
      // 删除的是活动会话：回到空会话（不自动切别的，避免误读他人上下文）
      const wasActive = s.activeChatId === id;
      const nextActive = wasActive ? null : s.activeChatId;
      if (wasActive) persistActiveChatId(null);
      return {
        chatSessions: list,
        activeChatId: nextActive,
        chat: wasActive ? { node: null, scaffoldId: null, turns: [], busy: false } : s.chat,
      };
    }),
  };
});

export function buildWalkQueue(root: Node, thresholdBytes: number): { node: Node; scaffoldId: string | null }[] {
  const out: { node: Node; scaffoldId: string | null }[] = [];
  const visit = (n: Node, depth: number) => {
    if (!n.is_dir) return;
    if (n.size >= thresholdBytes && depth > 0) {
      out.push({ node: n, scaffoldId: n.scaffold_id ?? null });
      return;
    }
    if (depth < 4) {
      for (const c of n.children) visit(c, depth + 1);
    }
  };
  visit(root, 0);
  out.sort((a, b) => b.node.size - a.node.size);
  return out;
}
