import { describe, it, expect } from 'vitest';
import { winapp2Stage } from './Studio';

describe('winapp2 导入面板的阶段（R393 winapp2Stage）', () => {
  it('没粘内容、没解析 → idle', () => {
    expect(winapp2Stage('', null, '')).toBe('idle');
    expect(winapp2Stage('   \n\t', null, '')).toBe('idle');
  });

  it('有内容但还没解析 → ready', () => {
    expect(winapp2Stage('[SomeApp]\nFileKey1=stuff', null, '')).toBe('ready');
  });

  it('解析后输入没动 → preview（可以勾选安装）', () => {
    const ini = '[SomeApp]\nFileKey1=stuff';
    expect(winapp2Stage(ini, [{ id: 'a' }], ini)).toBe('preview');
  });

  it('解析后又改了输入 → stale（旧预览不作数）', () => {
    expect(winapp2Stage('[A]\nFileKey1=changed', [{ id: 'a' }], '[A]\nFileKey1=orig')).toBe('stale');
    expect(winapp2Stage('', [{ id: 'a' }], '[A]\nFileKey1=orig')).toBe('stale');
  });

  it('解析出 0 条也算 preview——空结论由 R309 的空态条说明', () => {
    expect(winapp2Stage(';; only registry', [], ';; only registry')).toBe('preview');
  });
});
