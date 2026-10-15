import { describe, expect, it } from 'vitest';
import {
  fanAdviceDelta,
  formatFanSnapshot,
  parseFanSnapshot,
} from './fanHistory';

// R173：风扇建议的历史对比——「这次和上次比变了没」一眼可见。
describe('parseFanSnapshot / formatFanSnapshot（快照读写）', () => {
  const snap = { level: 'balanced', levelLabel: '均衡', tempMaxC: 68, atMs: 1_700_000_000_000 };

  it('往返一致', () => {
    expect(parseFanSnapshot(formatFanSnapshot(snap))).toEqual(snap);
  });

  it('坏 JSON / 非对象 / 缺 level → null（不编造）', () => {
    expect(parseFanSnapshot('not-json')).toBeNull();
    expect(parseFanSnapshot('null')).toBeNull();
    expect(parseFanSnapshot('"str"')).toBeNull();
    expect(parseFanSnapshot('{"tempMaxC":70}')).toBeNull();
  });

  it('空串 → null', () => {
    expect(parseFanSnapshot('')).toBeNull();
    expect(parseFanSnapshot(null)).toBeNull();
  });

  it('tempMaxC 非法（非数/NaN）→ null（不编造温度）', () => {
    expect(parseFanSnapshot('{"level":"high","tempMaxC":"x"}')).toEqual({
      level: 'high', levelLabel: '', tempMaxC: null, atMs: 0,
    });
  });
});

describe('fanAdviceDelta（与上次对比）', () => {
  const cur = { level: 'balanced', levelLabel: '均衡', tempMaxC: 68 };

  it('无历史 → null（不比较）', () => {
    expect(fanAdviceDelta(cur, null)).toBeNull();
  });

  it('档位变了 + 温度差', () => {
    expect(fanAdviceDelta(cur, { level: 'quiet', levelLabel: '静音', tempMaxC: 60, atMs: 1 })).toEqual({
      levelChanged: true,
      prevLevel: 'quiet',
      prevLevelLabel: '静音',
      tempDeltaC: 8,
    });
  });

  it('档位相同 + 温度持平（delta 0）', () => {
    expect(fanAdviceDelta(cur, { level: 'balanced', levelLabel: '均衡', tempMaxC: 68, atMs: 1 })).toEqual({
      levelChanged: false,
      prevLevel: 'balanced',
      prevLevelLabel: '均衡',
      tempDeltaC: 0,
    });
  });

  it('任一方温度读不到 → tempDeltaC null（不编造温差）', () => {
    expect(fanAdviceDelta({ ...cur, tempMaxC: null }, { level: 'quiet', levelLabel: '', tempMaxC: 60, atMs: 1 })!.tempDeltaC).toBeNull();
    expect(fanAdviceDelta(cur, { level: 'quiet', levelLabel: '', tempMaxC: null, atMs: 1 })!.tempDeltaC).toBeNull();
  });

  it('prevLevelLabel 缺省回退 prevLevel', () => {
    expect(fanAdviceDelta(cur, { level: 'quiet', levelLabel: '', tempMaxC: null, atMs: 1 })!.prevLevelLabel).toBe('quiet');
  });
});