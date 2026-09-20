import { describe, expect, it } from 'vitest';
import { deriveChatHistory, roughTokens, summarizeTrace } from './chatHistory';
import type { ChatTurn } from '../store';

function turn(role: 'user' | 'assistant', text: string, extra?: Partial<ChatTurn>): ChatTurn {
  return { id: `t_${Math.random().toString(36).slice(2)}`, role, text, ...extra };
}

describe('roughTokens', () => {
  it('CJK 与拉丁混排估算合理', () => {
    expect(roughTokens('')).toBe(0);
    expect(roughTokens('你好')).toBeGreaterThan(0);
    expect(roughTokens('hello world')).toBeGreaterThan(0);
  });
});

describe('summarizeTrace', () => {
  it('无 trace 返回 null', () => {
    expect(summarizeTrace(undefined)).toBeNull();
    expect(summarizeTrace([])).toBeNull();
  });
  it('压缩工具调用过程为一行概要', () => {
    const out = summarizeTrace([
      { kind: 'tool_call', text: '调用 path_size', ts: 1 },
      { kind: 'tool_result', text: 'path_size 返回', ts: 2 },
      { kind: 'tool_call', text: '调用 get_disk_health', ts: 3 },
      { kind: 'tool_result', text: 'get_disk_health 返回', ts: 4 },
    ]);
    expect(out).toContain('2');
    expect(out).toContain('调用了 2 个工具');
    expect(out).toContain('2 次工具返回');
  });
});

describe('deriveChatHistory', () => {
  it('空/全 pending 返回空', () => {
    expect(deriveChatHistory([])).toEqual([]);
    expect(deriveChatHistory([turn('user', 'hi', { pending: true })])).toEqual([]);
  });

  it('简短对话全量保留（不压缩）', () => {
    const turns = [
      turn('user', 'C 盘满了怎么办'),
      turn('assistant', '先用 WizTree 扫一下', { trace: [{ kind: 'tool_call', text: '调用 get_cli_tool_usage', ts: 1 }] }),
      turn('user', '好的'),
    ];
    const out = deriveChatHistory(turns);
    expect(out.length).toBe(3);
    // assistant 的 trace 被压成概要拼在正文前
    expect(out[1].content).toContain('[过程：');
    expect(out[1].content).toContain('先用 WizTree 扫一下');
    // 结果只含 user/assistant
    expect(out.every((t) => t.role === 'user' || t.role === 'assistant')).toBe(true);
  });

  it('超预算时压缩最老回合，最近 tail 完整保留', () => {
    const turns = [
      turn('user', 'A'.repeat(800)),           // 最老回合
      turn('assistant', 'B'.repeat(800)),
      turn('user', 'C'.repeat(800)),
      turn('assistant', 'D'.repeat(800)),
      turn('user', '最近的问题'),
      turn('assistant', '最近的回答'),
    ];
    const out = deriveChatHistory(turns, { budget: 500, maxTail: 2 });
    // 6 条都被保留（压缩成摘要也算），最后 2 条完整
    expect(out.length).toBe(6);
    const tail = out.slice(-2);
    expect(tail[0].content).toBe('最近的问题');
    expect(tail[1].content).toBe('最近的回答');
    // 最老回合被压成「早前…」摘要（贪心从新到旧保留，老的放不下就压）
    expect(out[0].content).toContain('早前');
  });

  it('预算充足时一段超长对话也不丢正文', () => {
    const longBody = '详细内容 '.repeat(50);
    const turns = [turn('user', longBody), turn('assistant', '回答')];
    const out = deriveChatHistory(turns, { budget: 100000 });
    expect(out[0].content).toContain('详细内容');
  });
});