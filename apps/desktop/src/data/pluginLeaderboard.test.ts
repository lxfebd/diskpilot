import { describe, expect, it } from 'vitest';
import { PLUGIN_DL_SNAPSHOT, leaderboardTop } from './pluginLeaderboard';

describe('leaderboardTop（R546 插件下载排行）', () => {
  it('按累计下载降序取前 n，bigfile-finder 领跑', () => {
    const top = leaderboardTop(PLUGIN_DL_SNAPSHOT, 5);
    expect(top.map((s) => s.id)).toEqual(['bigfile-finder', 'dup-finder', 'startup-audit', 'temp-hunter', 'log-hunter']);
    expect(top[0].totalDl).toBe(16);
  });
  it('n 超过条目数时返回全部，n=0 返回空', () => {
    expect(leaderboardTop(PLUGIN_DL_SNAPSHOT, 99).length).toBe(PLUGIN_DL_SNAPSHOT.items.length);
    expect(leaderboardTop(PLUGIN_DL_SNAPSHOT, 0)).toEqual([]);
  });
  it('并列时按 latestDl 再排、再并列按 id 稳定（schtask/reg 都 7，schtask latestDl 小但 total 相同——total 优先后 latestDl 决胜）', () => {
    const all = leaderboardTop(PLUGIN_DL_SNAPSHOT, 11);
    const i9 = all.findIndex((s) => s.id === 'schtask-hunter');
    const i10 = all.findIndex((s) => s.id === 'reg-hunter');
    // total 都是 7：reg-hunter latestDl=3 > schtask latestDl=1，所以 reg 在前
    expect(i10).toBeLessThan(i9);
  });
  it('快照元数据：采集日期 + 合计 125', () => {
    expect(PLUGIN_DL_SNAPSHOT.collectedAt).toBe('2026-11-08');
    expect(PLUGIN_DL_SNAPSHOT.total).toBe(125);
    expect(PLUGIN_DL_SNAPSHOT.items.length).toBe(11);
  });
});
