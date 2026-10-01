import { describe, expect, it } from 'vitest';
import { communityComingSoon, emptiedAllDedupe } from './market';
import type { RegistryPlugin } from '../../api';

function p(partial: Partial<RegistryPlugin>): RegistryPlugin {
  return {
    id: 'x',
    name: 'X',
    version: '1.0.0',
    author: '',
    description: '',
    category: '',
    risk: '',
    tags: [],
    url: '',
    sha256: '',
    signer: '',
    downloads: 0,
    ...partial,
  };
}

// 社区索引「全占位」判断：有条目但无一提供下载地址（等待上架引导）。
describe('communityComingSoon（索引全占位判断）', () => {
  it('索引已接入且全部条目 url 空 → true', () => {
    const c = [p({ url: '' }), p({ url: '' })];
    expect(communityComingSoon(c, true)).toBe(true);
  });

  it('有可装条目（url 非空）→ false', () => {
    const c = [p({ url: 'https://example.com/a.zip' }), p({ url: '' })];
    expect(communityComingSoon(c, true)).toBe(false);
  });

  it('空列表 → false（空市场走 communityEmpty 文案）', () => {
    expect(communityComingSoon([], true)).toBe(false);
  });

  it('未接入索引 → false', () => {
    const c = [p({ url: '' })];
    expect(communityComingSoon(c, false)).toBe(false);
  });
});

// 空列表是否源于「内置去重」：索引接入成功但条目全被 filter_builtin_dupes 隐藏。
// 此情形应给"索引接入成功但全内置"引导，而非冷冰冰的「社区注册表为空」。
describe('emptiedAllDedupe（空列表源于内置去重判断）', () => {
  it('索引接入 + 空列表 + 有隐藏 → true（全去重）', () => {
    expect(emptiedAllDedupe([], 30, true)).toBe(true);
  });

  it('索引接入 + 空列表 + 无隐藏 → false（真没条目）', () => {
    expect(emptiedAllDedupe([], 0, true)).toBe(false);
  });

  it('未接入索引 → false', () => {
    expect(emptiedAllDedupe([], 30, false)).toBe(false);
  });

  it('列表非空 → false（有展示条目不算全去重）', () => {
    expect(emptiedAllDedupe([p({ url: '' })], 5, true)).toBe(false);
  });
});
