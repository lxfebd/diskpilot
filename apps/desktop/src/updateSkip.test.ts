import { describe, expect, it } from 'vitest';
import { compareVersions, hasRegistryUpdate, shouldPromptUpdate } from './updateSkip';

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