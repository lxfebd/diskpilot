import { describe, expect, it } from 'vitest';
import { agentDescQuality, descOverlap, descTokens, similarAgentTools } from './similar';

describe('descTokens（描述词袋）', () => {
  it('拉丁词拆分小写', () => {
    expect(descTokens('Get CPU Temperature')).toEqual(new Set(['cpu', 'temperature']));
  });
  it('CJK 双字组词（去停用字窗口）', () => {
    expect(descTokens('获取CPU温度')).toEqual(new Set(['cpu', '获取', '取温', '温度']));
  });
  it('混合：中文+英文+数字', () => {
    expect(descTokens('查询 Windows 服务的 状态 status')).toEqual(
      new Set(['windows', 'status', '查询', '询服', '服务', '状态']),
    );
  });
});

describe('descOverlap（词袋交集）', () => {
  it('按目标词袋计数', () => {
    const a = descTokens('cpu 温度');
    const b = descTokens('gpu 温度');
    expect(descOverlap(a, b)).toBe(1); // 温度
  });
});

describe('similarAgentTools（同类 AI 工具推荐）', () => {
  it('按重叠降序、排除自身、取前 limit；同分按 name 升序', () => {
    const target = { name: 'cpu_temp', description: 'Get CPU temperature' };
    const all = [
      { name: 'gpu_temp', description: 'Get GPU temperature' },
      { name: 'cpu_load', description: 'Read CPU load percentage' },
      { name: 'cpu_temp', description: 'Get CPU temperature' }, // 自身
      { name: 'disk_usage', description: 'Disk space usage report' },
    ];
    expect(similarAgentTools(target, all, 2).map((t) => t.name)).toEqual(['cpu_load', 'gpu_temp']);
  });
  it('重叠数高者优先（多词命中）', () => {
    const target = { name: 't', description: 'Get CPU temperature sensor' };
    const all = [
      { name: 'loose', description: 'Get GPU temperature' },
      { name: 'tight', description: 'Read CPU temperature sensor value' },
    ];
    expect(similarAgentTools(target, all, 2).map((t) => t.name)).toEqual(['tight', 'loose']);
  });
  it('无重叠返回空', () => {
    expect(
      similarAgentTools({ name: 'a', description: 'CPU temp' }, [{ name: 'b', description: 'disk io speed' }]),
    ).toEqual([]);
  });
  it('目标描述为空词袋（全停用词）返回空', () => {
    const target = { name: 'a', description: '的 和 或 的' };
    expect(similarAgentTools(target, [{ name: 'b', description: 'CPU temp' }])).toEqual([]);
  });
  it('不改入参（纯函数）', () => {
    const all = [{ name: 'b', description: 'Get GPU temperature' }];
    similarAgentTools({ name: 'a', description: 'Get CPU temperature' }, all);
    expect(all).toEqual([{ name: 'b', description: 'Get GPU temperature' }]);
  });
});

// R153：AI 工具描述充分度——空/全虚词描述显式标注，解释为何无同类推荐。
describe('agentDescQuality（描述充分度）', () => {
  it('空串 / 纯停用词 / 纯标点 → none', () => {
    expect(agentDescQuality('')).toBe('none');
    expect(agentDescQuality('的 和 或 一 个')).toBe('none');
    expect(agentDescQuality('... !!')).toBe('none');
  });
  it('只有 1 个有效词 → sparse', () => {
    expect(agentDescQuality('temperature')).toBe('sparse');
    expect(agentDescQuality('温度')).toBe('sparse'); // 双字窗内含停用字被滤后剩 0？不——「温」不在停用表，实际产出「温度」一词
  });
  it('≥2 个有效词 → rich', () => {
    expect(agentDescQuality('Get CPU temperature')).toBe('rich'); // get 是停用词，剩 cpu/temperature
    expect(agentDescQuality('获取磁盘占用')).toBe('rich'); // 获取/取磁/磁盘/占用 ≥2
  });
});