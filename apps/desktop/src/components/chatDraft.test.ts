// ── 会话级聊天草稿守卫（R454）───────────────────────────────────────
// 聊天框的输入草稿与待发附件按会话各存一份：切会话不再串台。
import { describe, it, expect } from 'vitest';
import { EMPTY_CHAT_DRAFT, isChatDraftEmpty, swapChatDraft, type ChatDraft, type ChatDraftStore } from './chatDraft';

const RAW = import.meta.glob('./ChatPanel.tsx', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

const src = RAW['./ChatPanel.tsx'];

function draft(over: Partial<ChatDraft> = {}): ChatDraft {
  return { text: '', drops: [], images: [], ...over };
}

describe('会话级草稿的搬运（R454 swapChatDraft）', () => {
  it('切走时非空草稿归到离开的会话，目标会话取回自己的草稿', () => {
    const store: ChatDraftStore = {
      chat_b: draft({ text: 'B 的话' }),
    };
    const cur = draft({ text: 'A 的话', drops: [{ path: 'C:/a.txt', name: 'a.txt' }] });
    const out = swapChatDraft(store, 'chat_a', 'chat_b', cur);
    expect(out.store.chat_a).toBeDefined();
    expect(out.store.chat_a.text).toBe('A 的话');
    expect(out.store.chat_a.drops).toHaveLength(1);
    expect(out.next.text).toBe('B 的话');
    expect(out.next.drops).toHaveLength(0);
  });

  it('空草稿不落条目：切走再切回是空的，不残留上次输入', () => {
    const out = swapChatDraft({}, 'chat_a', 'chat_b', draft());
    expect('chat_a' in out.store).toBe(false);
    expect(out.next).toBe(EMPTY_CHAT_DRAFT);
  });

  it('目标会话没有草稿时返回空草稿（输入框被清空）', () => {
    const out = swapChatDraft({}, 'chat_a', 'chat_b', draft({ text: 'a' }));
    expect(out.next).toBe(EMPTY_CHAT_DRAFT);
  });

  it('同一会话不搬动（from === to）', () => {
    const out = swapChatDraft({}, 'chat_a', 'chat_a', draft({ text: 'x' }));
    expect(out.next.text).toBe('x');
    expect(out.store).toEqual({});
  });

  it('只有附件（文字为空）也算非空草稿', () => {
    expect(isChatDraftEmpty(draft())).toBe(true);
    expect(isChatDraftEmpty(draft({ images: [{ id: 'i1', name: 'p.png', dataUrl: 'data:image/png;base64,abc', mimeType: 'image/png' }] }))).toBe(false);
    expect(isChatDraftEmpty(draft({ drops: [{ path: 'D:/x', name: 'x' }] }))).toBe(false);
  });

  it('返回的 next 数组与库内拷贝互不影响', () => {
    const out = swapChatDraft({ chat_b: draft({ drops: [{ path: 'E:/b', name: 'b' }] }) }, 'chat_a', 'chat_b', draft({ text: 'a' }));
    out.next.drops[0].path = 'mutated';
    expect(out.store.chat_b.drops[0].path).toBe('E:/b');
  });
});

describe('会话级草稿在聊天框的接线（R454）', () => {
  it('ChatPanel 持有会话级草稿表并在切换会话时搬运三样状态', () => {
    expect(src).toContain('const draftsRef = useRef<ChatDraftStore>({});');
    expect(src).toContain('const prevChatIdRef = useRef<string | null>(activeChatId);');
    expect(src).toContain('swapChatDraft(draftsRef.current, from, activeChatId, {');
    expect(src).toContain('setInput(swapped.next.text);');
    expect(src).toContain('setPendingDrops(swapped.next.drops);');
    expect(src).toContain('setPendingImages(swapped.next.images);');
  });
});
