import { describe, expect, it } from 'vitest';
import { compareVersions, shouldPromptUpdate } from './updateSkip';

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