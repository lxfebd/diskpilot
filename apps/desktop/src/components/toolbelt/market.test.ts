import { describe, expect, it } from 'vitest';
import { communityComingSoon } from './market';
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
