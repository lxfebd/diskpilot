// 事件源会话日志的「派生上下文」实现（dsh session-log 模式的轻量落地）。
//
// DiskPilot 的 AI 上下文不再是「手动拼最近 N 条消息」，而是从完整会话 turns
// 派生：正文回合（user/assistant 实际说的话）全量保留，工具过程（trace 里的
// tool_call/tool_result/thinking）压缩成一行概要，超过 token 预算时把最老的
// 完整回合压成摘要而不是直接丢——早期关键信息尽量不丢。
//
// 纯函数、无副作用，便于单元测试；ChatPanel 每轮调用取最新上下文。

import type { ChatTurn } from '../store';
import type { TraceItem } from '../store';
import { common } from '../i18n/namespaces/common';

/** 取「喂给模型的中文模板」（common.chatModel.*，属不译清单）：恒读中文表原文，
 *  与 UI 语言无关——切语言不该改变发给模型的上下文。{占位符} 在此本地替换。 */
function zhTpl(key: keyof typeof common.zh, params?: Record<string, string | number>): string {
  let v: string = common.zh[key];
  if (params) {
    for (const [k, val] of Object.entries(params)) v = v.split(`{${k}}`).join(String(val));
  }
  return v;
}

/** 文本长度的粗略 token 估计：CJK 每字约 1 token，拉丁每 4 字符约 1 token。 */
export function roughTokens(s: string): number {
  if (!s) return 0;
  let cjk = 0;
  let other = 0;
  for (const ch of s) {
    if (/[\u4e00-\u9fff\u3040-\u30ff\uac00-\ud7af]/.test(ch)) cjk += 1;
    else other += 1;
  }
  return Math.ceil(cjk * 1.2 + other / 4);
}

/** 把一条 assistant 回合的工具过程（trace）压成一行概要。AI 回放历史时
 *  不需要重看思考/工具调用的全过程，只要结论和关键动作。 */
export function summarizeTrace(trace: TraceItem[] | undefined): string | null {
  if (!trace || trace.length === 0) return null;
  const calls = trace.filter((t) => t.kind === 'tool_call').length;
  const results = trace.filter((t) => t.kind === 'tool_result').length;
  const notes = trace.filter((t) => t.kind === 'note').map((t) => t.text).slice(-2);
  const parts: string[] = [];
  if (calls > 0) parts.push(zhTpl('common.chatModel.procCalls', { n: calls }));
  if (results > 0) parts.push(zhTpl('common.chatModel.procResults', { n: results }));
  if (notes.length > 0) parts.push(notes.join(zhTpl('common.chatModel.noteJoin')));
  return parts.length > 0 ? zhTpl('common.chatModel.procWrap', { text: parts.join(zhTpl('common.chatModel.procJoin')) }) : null;
}

export interface DerivedTurn {
  role: 'user' | 'assistant';
  content: string;
}

const DEFAULT_BUDGET = 6000; // 默认 token 预算：约 4500 中文字 / 24000 英文

/**
 * 从完整会话 turns 派生 AI 上下文（对标 dsh 的 deriveMessages）。
 *
 * 规则：
 * 1. 只取 user / assistant 的已完成回合（跳过 system / hw / pending）。
 * 2. assistant 回合的 trace 压缩成一行概要，拼在正文前。
 * 3. 全部回合先按「正文 + 概要」估算 token；超预算则从最老开始，
 *    把单个完整回合压成一行摘要（首句 + 要点），直到不超。
 * 4. 最近 MAX_TAIL 条永远完整保留（短程记忆不动）。
 */
export function deriveChatHistory(
  turns: ChatTurn[],
  opts?: { budget?: number; maxTail?: number },
): DerivedTurn[] {
  const budget = opts?.budget ?? DEFAULT_BUDGET;
  const maxTail = opts?.maxTail ?? 8;

  const done = turns.filter(
    (t): t is ChatTurn & { role: 'user' | 'assistant' } =>
      (t.role === 'user' || t.role === 'assistant') && !t.pending,
  );
  if (done.length === 0) return [];

  // 每条正文 + 可选的 trace 概要
  const entries: { turn: ChatTurn & { role: 'user' | 'assistant' }; body: string }[] = done.map((t) => {
    const body = t.text.trim();
    if (t.role === 'assistant') {
      const summary = summarizeTrace(t.trace);
      return { turn: t, body: summary ? `${summary}\n${body}` : body };
    }
    return { turn: t, body };
  });

  const total = entries.reduce((acc, e) => acc + roughTokens(e.body), 0);
  if (total <= budget) {
    return entries.map((e) => ({ role: e.turn.role, content: e.body }));
  }

  // 超预算：最近 maxTail 条永远完整保留（短程记忆不动）；
  // 其余头部回合从【最新】开始贪心保留完整，放不下的（更老的）压成一行
  // 摘要，直到总 token 落到预算内——保证早期信息不丢、近的尽量完整。
  const protectedTail = Math.min(maxTail, entries.length);
  const headCount = entries.length - protectedTail;
  const tailTokens = entries.slice(headCount).reduce((acc, x) => acc + roughTokens(x.body), 0);
  const headMode: ('full' | 'summary')[] = new Array(headCount).fill('summary');
  let used = 0;
  for (let i = headCount - 1; i >= 0; i--) {
    const t = roughTokens(entries[i].body);
    if (used + t <= budget - tailTokens) {
      headMode[i] = 'full';
      used += t;
    }
  }
  const out: DerivedTurn[] = [];
  for (let i = 0; i < headCount; i++) {
    const e = entries[i];
    if (headMode[i] === 'summary') {
      const firstLine = e.body.split('\n')[0]?.slice(0, 60) ?? '';
      out.push({
        role: e.turn.role,
        content: zhTpl(
          e.turn.role === 'user' ? 'common.chatModel.earlierUser' : 'common.chatModel.earlierAssistant',
          { text: firstLine },
        ),
      });
    } else {
      out.push({ role: e.turn.role, content: e.body });
    }
  }
  // 尾部（最近）完整保留
  for (let i = headCount; i < entries.length; i++) {
    const e = entries[i];
    out.push({ role: e.turn.role, content: e.body });
  }
  return out;
}
