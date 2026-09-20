import { describe, it, expect } from 'vitest';
import { lastToolCall, runningSecs } from './traceTimer';
import type { TraceItem } from '../store';

const call = (ts: number, text = '调用 x'): TraceItem => ({ kind: 'tool_call', text, ts });
const res = (ts: number): TraceItem => ({ kind: 'tool_result', text: 'x 返回', ts });
const think = (ts: number): TraceItem => ({ kind: 'thinking', text: '思考中', ts });

describe('lastToolCall（工具调用计时定位）', () => {
  it('空 trace → null', () => {
    expect(lastToolCall(null)).toBeNull();
    expect(lastToolCall([])).toBeNull();
    expect(lastToolCall(undefined)).toBeNull();
  });

  it('只有思考事件 → null', () => {
    expect(lastToolCall([think(1), think(2)])).toBeNull();
  });

  it('返回最后一条 tool_call（即使后面还有 thinking）', () => {
    const t = [think(1), call(2, '调用 bench_disk'), think(3)];
    expect(lastToolCall(t)).toEqual(call(2, '调用 bench_disk'));
  });

  it('多工具链：取最后一条（当前正在运行的那个）', () => {
    const t = [call(1, '调用 bench_cpu'), res(2), call(3, '调用 bench_disk')];
    expect(lastToolCall(t)?.text).toBe('调用 bench_disk');
    expect(lastToolCall(t)?.ts).toBe(3);
  });
});

describe('runningSecs（已运行秒数）', () => {
  it('基本换算', () => {
    expect(runningSecs(1000, 5000)).toBe(4);
    expect(runningSecs(1000, 1999)).toBe(0);
    expect(runningSecs(1000, 1000)).toBe(0);
  });

  it('异常输入 → 0（时钟回拨 / 无效时间戳）', () => {
    expect(runningSecs(0, 5000)).toBe(0);
    expect(runningSecs(5000, 1000)).toBe(0);
    expect(runningSecs(0, 0)).toBe(0);
  });
});