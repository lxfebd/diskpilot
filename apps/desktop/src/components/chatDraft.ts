// ── 会话级聊天草稿（R454）────────────────────────────────────────────
// 聊天框的「正在输入」分两块：输入框文字 + 待发附件（拖入的路径清单 / 图片）。
// 这两块以前是组件级单例状态——切到另一个会话，半句话和附件会跟着人走：
// 用户以为发在本会话，实际进了新会话的上下文（发错对话）。
// 这里把草稿按 activeChatId 各存一份：切走时存档、切回时原样取回。
export interface ChatDropDraft {
  path: string;
  name: string;
}

export interface ChatImageDraft {
  id: string;
  name: string;
  dataUrl: string;
  mimeType: string;
}

export interface ChatDraft {
  text: string;
  drops: ChatDropDraft[];
  images: ChatImageDraft[];
}

export const EMPTY_CHAT_DRAFT: ChatDraft = { text: '', drops: [], images: [] };

export function isChatDraftEmpty(d: ChatDraft): boolean {
  return d.text.trim() === '' && d.drops.length === 0 && d.images.length === 0;
}

export type ChatDraftStore = Record<string, ChatDraft>;

// 切换会话：把当前草稿归到 fromId 名下（空草稿直接删除该条目，不残留），
// 再返回 toId 名下的草稿（没有则空草稿）。fromId === toId 时不搬动。
export function swapChatDraft(
  store: ChatDraftStore,
  from: string | null,
  to: string | null,
  current: ChatDraft,
): { store: ChatDraftStore; next: ChatDraft } {
  if (from === to) return { store, next: current };
  const nextStore: ChatDraftStore = { ...store };
  if (from != null) {
    if (isChatDraftEmpty(current)) delete nextStore[from];
    else nextStore[from] = current;
  }
  const saved = to != null ? nextStore[to] : undefined;
  const next = saved
    ? {
      text: saved.text,
      drops: saved.drops.map((d) => ({ ...d })),
      images: saved.images.map((i) => ({ ...i })),
    }
    : EMPTY_CHAT_DRAFT;
  return { store: nextStore, next };
}
