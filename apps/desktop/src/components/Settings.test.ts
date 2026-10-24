// R352：AI 配置草稿的「有没有改动」判定——关窗二次确认与「未保存」提示都靠它。
import { describe, it, expect } from 'vitest';
import { aiDraftDirty, type AiDraft } from './Settings';

const base: AiDraft = {
  baseUrl: 'https://api.example.com/v1',
  apiKey: 'sk-abc',
  model: 'gpt-4o-mini',
  providerOverride: 'openai',
};

describe('AI 配置草稿是否未保存（R352 aiDraftDirty）', () => {
  it('四个字段全一样时不算脏', () => {
    expect(aiDraftDirty({ ...base }, base)).toBe(false);
  });

  it('任一字段不同就算脏', () => {
    expect(aiDraftDirty({ ...base, baseUrl: 'https://api.other.com/v1' }, base)).toBe(true);
    expect(aiDraftDirty({ ...base, apiKey: 'sk-xyz' }, base)).toBe(true);
    expect(aiDraftDirty({ ...base, model: 'deepseek-chat' }, base)).toBe(true);
    expect(aiDraftDirty({ ...base, providerOverride: 'ollama' }, base)).toBe(true);
  });

  it('清空输入也算改动（不是「没填」）', () => {
    expect(aiDraftDirty({ ...base, model: '' }, base)).toBe(true);
  });

  it('providerOverride 从 undefined 变成具体值算脏', () => {
    const noOverride: AiDraft = { baseUrl: '', apiKey: '', model: '', providerOverride: undefined };
    expect(aiDraftDirty({ ...noOverride, providerOverride: 'anthropic' }, noOverride)).toBe(true);
  });

  it('没有已保存配置时以全空为基线', () => {
    const empty: AiDraft = { baseUrl: '', apiKey: '', model: '', providerOverride: undefined };
    expect(aiDraftDirty(empty, null)).toBe(false);
    expect(aiDraftDirty({ ...empty, model: 'x' }, null)).toBe(true);
    expect(aiDraftDirty({ ...empty, baseUrl: '  ' }, null)).toBe(true);
  });
});