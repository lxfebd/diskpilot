import { describe, expect, it } from 'vitest';
import { filterAgentTools, filterToolsByRisk, matchesToolQuery } from './catalog';
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

// R171：AI 分区按写入权限筛选——只读无需确认、写操作改系统状态。
describe('filterAgentTools（AI 工具权限筛选）', () => {
  const list = [
    { name: 'hw_temperature', writable: false },
    { name: 'process_kill', writable: true },
    { name: 'system_probe', writable: false },
  ];

  it('all → 原样返回（含顺序）', () => {
    expect(filterAgentTools(list, 'all').map((m) => m.name)).toEqual([
      'hw_temperature', 'process_kill', 'system_probe',
    ]);
  });

  it('read → 仅只读（writable false）', () => {
    expect(filterAgentTools(list, 'read').map((m) => m.name)).toEqual(['hw_temperature', 'system_probe']);
  });

  it('write → 仅写操作', () => {
    expect(filterAgentTools(list, 'write').map((m) => m.name)).toEqual(['process_kill']);
  });

  it('空列表 → 空数组；不改入参（all 分支复制数组）', () => {
    expect(filterAgentTools([], 'write')).toEqual([]);
    const input = [...list];
    const out = filterAgentTools(input, 'all');
    expect(input).toHaveLength(3);
    expect(out).not.toBe(input);
  });
});

// R172：工具墙按风险等级筛——卡片徽标此前无法用来筛选。
describe('filterToolsByRisk（风险等级筛选）', () => {
  const tools = [
    { name: 'A', risk: 'low' },
    { name: 'B', risk: 'medium' },
    { name: 'C', risk: 'high' },
    { name: 'D' }, // 缺 risk
  ];

  it('all → 原样（含顺序）', () => {
    expect(filterToolsByRisk(tools, 'all').map((t) => t.name)).toEqual(['A', 'B', 'C', 'D']);
  });

  it('按等级筛；缺 risk 归 low（与卡片徽标兜底同口径）', () => {
    expect(filterToolsByRisk(tools, 'low').map((t) => t.name)).toEqual(['A', 'D']);
    expect(filterToolsByRisk(tools, 'medium').map((t) => t.name)).toEqual(['B']);
    expect(filterToolsByRisk(tools, 'high').map((t) => t.name)).toEqual(['C']);
  });

  it('空列表 → 空数组；all 分支不改入参', () => {
    expect(filterToolsByRisk([], 'high')).toEqual([]);
    const input = [...tools];
    const out = filterToolsByRisk(input, 'all');
    expect(input).toHaveLength(4);
    expect(out).not.toBe(input);
  });
});