import { describe, expect, it } from 'vitest';
import {
  UPDATE_SKIP_DAYS,
  compareVersions,
  formatSkipRecord,
  hasRegistryUpdate,
  meetsMinApp,
  parseSkipRecord,
  shouldPromptUpdate,
} from './updateSkip';

// R158：版本比较——「跳过此版本」必须版本相关，不能永久静音。
describe('compareVersions（点分版本比较）', () => {
  it('数字段大小', () => {
    expect(compareVersions('1.2.3', '1.2.4')).toBe(-1);
    expect(compareVersions('1.2.4', '1.2.3')).toBe(1);
    expect(compareVersions('2.0.0', '1.9.9')).toBe(1);
  });

  it('相同版本 → 0', () => {
    expect(compareVersions('1.2.3', '1.2.3')).toBe(0);
  });

  it('缺位补 0（1.2 == 1.2.0）', () => {
    expect(compareVersions('1.2', '1.2.0')).toBe(0);
    expect(compareVersions('1.2', '1.2.1')).toBe(-1);
  });

  it('非数字段按 0（不抛，预发布后缀不炸）', () => {
    expect(compareVersions('1.2.3-beta', '1.2.3')).toBe(0);
    expect(compareVersions('1.x.0', '1.0.0')).toBe(0);
  });
});

describe('shouldPromptUpdate（是否仍提示该版本）', () => {
  it('没跳过任何版本 → 提示', () => {
    expect(shouldPromptUpdate(null, '1.5.0')).toBe(true);
  });

  it('跳过同一版本 → 不提示', () => {
    expect(shouldPromptUpdate('1.5.0', '1.5.0')).toBe(false);
  });

  it('出现更新的版本 → 重新提示（跳过不是永久静音）', () => {
    expect(shouldPromptUpdate('1.5.0', '1.6.0')).toBe(true);
    expect(shouldPromptUpdate('1.5.0', '2.0.0')).toBe(true);
  });

  it('远端版本反而更旧（回滚/误报）→ 不提示', () => {
    expect(shouldPromptUpdate('1.5.0', '1.4.9')).toBe(false);
  });
});

// R161：跳过记录带时刻 + 过期重提示——跳过不是永久静音。
describe('R161 parseSkipRecord / formatSkipRecord', () => {
  it('往返一致', () => {
    const raw = formatSkipRecord('1.5.0', 1_700_000_000_000);
    expect(raw).toBe('1.5.0@1700000000000');
    expect(parseSkipRecord(raw)).toEqual({ version: '1.5.0', atMs: 1_700_000_000_000 });
  });

  it('旧格式（纯版本串）→ atMs 0（不过期，兼容 R158 记录）', () => {
    expect(parseSkipRecord('1.5.0')).toEqual({ version: '1.5.0', atMs: 0 });
  });

  it('空 / 版本为空 → null', () => {
    expect(parseSkipRecord(null)).toBeNull();
    expect(parseSkipRecord('')).toBeNull();
    expect(parseSkipRecord('@123')).toBeNull();
  });

  it('时刻非法 → atMs 0（不编造时间）', () => {
    expect(parseSkipRecord('1.5.0@abc')).toEqual({ version: '1.5.0', atMs: 0 });
    expect(parseSkipRecord('1.5.0@-5')).toEqual({ version: '1.5.0', atMs: 0 });
  });
});

describe('R161 跳过的有效期（UPDATE_SKIP_DAYS）', () => {
  const DAY = 86_400_000;
  const now = 1_800_000_000_000;
  const ago = (days: number) => now - days * DAY;

  it('未到期且无新版 → 不提示', () => {
    expect(shouldPromptUpdate('1.5.0', '1.5.0', ago(UPDATE_SKIP_DAYS - 1), now)).toBe(false);
  });

  it('跳过满 UPDATE_SKIP_DAYS 天 → 重新提示（同版本也提示）', () => {
    expect(shouldPromptUpdate('1.5.0', '1.5.0', ago(UPDATE_SKIP_DAYS), now)).toBe(true);
    expect(shouldPromptUpdate('1.5.0', '1.5.0', ago(UPDATE_SKIP_DAYS + 10), now)).toBe(true);
  });

  it('无时刻（旧记录 / 0）→ 视为不过期', () => {
    expect(shouldPromptUpdate('1.5.0', '1.5.0', 0, now)).toBe(false);
  });

  it('有新版时优先级更高（未到期也提示）', () => {
    expect(shouldPromptUpdate('1.5.0', '1.6.0', ago(1), now)).toBe(true);
  });
});

// R159：社区条目「有新版本」判定——按 id 匹配已装清单，只有严格更新才算。
describe('hasRegistryUpdate（插件是否有新版）', () => {
  const installed = [
    { id: 'wiztree-helper', version: '1.2.0' },
    { id: 'disk-cleaner', version: '2.0.0' },
  ];

  it('远端版本更新 → true', () => {
    expect(hasRegistryUpdate({ id: 'wiztree-helper', version: '1.3.0' }, installed)).toBe(true);
    expect(hasRegistryUpdate({ id: 'wiztree-helper', version: '2.0.0' }, installed)).toBe(true);
  });

  it('版本相同 / 远端更旧 → false（不误报更新）', () => {
    expect(hasRegistryUpdate({ id: 'wiztree-helper', version: '1.2.0' }, installed)).toBe(false);
    expect(hasRegistryUpdate({ id: 'wiztree-helper', version: '1.1.9' }, installed)).toBe(false);
  });

  it('未安装 / 缺版本号 → false', () => {
    expect(hasRegistryUpdate({ id: 'not-installed', version: '9.9.9' }, installed)).toBe(false);
    expect(hasRegistryUpdate({ id: 'wiztree-helper', version: null }, installed)).toBe(false);
    expect(hasRegistryUpdate({ id: 'wiztree-helper' }, installed)).toBe(false);
  });
});

// R168：插件声明的 min_app 校验——min_app 此前两端都没用。
describe('meetsMinApp（应用版本是否满足插件要求）', () => {
  it('当前版本 ≥ 要求 → true', () => {
    expect(meetsMinApp('0.2.3', '0.2.0')).toBe(true);
    expect(meetsMinApp('0.2.3', '0.2.3')).toBe(true);
    expect(meetsMinApp('1.0.0', '0.9.9')).toBe(true);
  });

  it('当前版本 < 要求 → false（该拦下）', () => {
    expect(meetsMinApp('0.2.3', '0.3.0')).toBe(false);
    expect(meetsMinApp('0.2.3', '1.0.0')).toBe(false);
  });

  it('未声明 min_app → true（不拦）', () => {
    expect(meetsMinApp('0.2.3', null)).toBe(true);
    expect(meetsMinApp('0.2.3', undefined)).toBe(true);
    expect(meetsMinApp('0.2.3', '   ')).toBe(true);
  });

  it('当前版本为空 → true（无法判断就不拦，别锁死可用功能）', () => {
    expect(meetsMinApp('', '9.9.9')).toBe(true);
  });
});