import { describe, expect, it } from 'vitest';
import { matchesToolQuery } from './catalog';
import type { ToolbeltCatalogItem } from '../../api';

// R170：工具墙搜索匹配——此前只搜 name/description/tags，按厂商/版本/架构
// 找工具都搜不到。
const mk = (over: Partial<ToolbeltCatalogItem>): ToolbeltCatalogItem => ({
  name: 'CPU-Z',
  category: '处理器工具',
  dir_rel: 'CPU-Z',
  exe_rel: 'cpuz_x64.exe',
  extension: 'exe',
  icon: null,
  description: 'CPU/主板/内存详细参数',
  tags: ['检测'],
  is_linked: false,
  is_builtin_link: false,
  arch: 'x64',
  arch_variants: [],
  linked_from: [],
  risk: 'low',
  launch_target: null,
  publisher: 'CPUID',
  version: '2.09',
  tutorial_url: null,
  download_url: null,
  is_promotion: false,
  ...over,
});

describe('matchesToolQuery（工具墙搜索）', () => {
  const tool = mk({});

  it('空词 → true（调用方只在有词时调，等同不过滤）', () => {
    expect(matchesToolQuery(tool, '')).toBe(true);
  });

  it('名称 / 描述 / 标签仍能命中（原口径不回归）', () => {
    expect(matchesToolQuery(tool, 'cpu-z')).toBe(true);
    expect(matchesToolQuery(tool, '内存')).toBe(true);
    expect(matchesToolQuery(tool, '检测')).toBe(true);
  });

  it('厂商 / 版本 / 架构 / 分类 / 目录名可命中（R170 新增）', () => {
    expect(matchesToolQuery(tool, 'cpuid')).toBe(true);
    expect(matchesToolQuery(tool, '2.09')).toBe(true);
    expect(matchesToolQuery(tool, 'x64')).toBe(true);
    expect(matchesToolQuery(tool, '处理器')).toBe(true);
    expect(matchesToolQuery(tool, 'cpu-z')).toBe(true); // dir_rel
  });

  it('缺失字段不炸（null 安全）；不匹配 → false', () => {
    const bare = mk({ publisher: null, version: null, description: '', tags: [] });
    expect(matchesToolQuery(bare, 'nvidia')).toBe(false);
    expect(matchesToolQuery(bare, 'cpu')).toBe(true); // name 兜底
  });
});