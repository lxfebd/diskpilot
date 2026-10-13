import { describe, expect, it } from 'vitest';
import { fanChannelState } from './SystemTools';
import type { FanStatus, FanProbe } from '../api';

// R71 诚实性回归：风扇可调通道三态——「状态探测失败」绝不能被渲染成
// 「当前无可用调速通道」。状态未读到 ≠ 不存在（同一族：未读到 ≠ 空）。
const PROBE: FanProbe = {
  channel: 'cli',
  writable: false,
  mode: 'cli',
  reason: '',
  cli: { name: 'fancmd', exe: 'fancmd.exe', exe_path: 'C:\\fancmd.exe', subcmd_tpl: '' },
};

function status(partial: Partial<FanStatus> = {}): FanStatus {
  return {
    probe: PROBE,
    temp_c: null,
    writable: false,
    has_snapshot: false,
    guard_running: false,
    note: '',
    ...partial,
  };
}

describe('fanChannelState（风扇可调通道三态）', () => {
  it('可写通道在位 → ok（无论 err 是否残留）', () => {
    expect(fanChannelState(status({ writable: true }), null)).toBe('ok');
    // 刷新成功后旧错误要能翻回 ok（新读取覆盖旧错误）
    expect(fanChannelState(status({ writable: true }), '旧错误')).toBe('ok');
  });

  it('探测失败（有 err）→ unread，绝不落到 none', () => {
    expect(fanChannelState(null, 'backend down')).toBe('unread');
    expect(fanChannelState(status({ writable: false }), 'no sensor')).toBe('unread');
  });

  it('探测成功但无通道（无 err、不可写）→ none（真空语义）', () => {
    expect(fanChannelState(null, null)).toBe('none');
    expect(fanChannelState(status({ writable: false }), null)).toBe('none');
  });
});