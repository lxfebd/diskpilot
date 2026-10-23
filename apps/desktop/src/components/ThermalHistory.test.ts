import { describe, it, expect, beforeAll } from 'vitest';
import { setLang } from '../i18n';
import { summarizeThermalHistory, thermalSparkPath } from './AssetOverview';

// 断言按中文原文写，先钉死语言。
beforeAll(() => setLang('zh'));

const p = (temp_max_c: number, at: number) => ({ temp_max_c, at });

describe('温度采样历史汇总（R299：unread / empty / single / ok 四态分开）', () => {
  it('读失败 → unread，不能说成「还没采样」', () => {
    const s = summarizeThermalHistory(null);
    expect(s.state).toBe('unread');
    expect(s.count).toBe(0);
    expect(s.peakC).toBeNull();
  });

  it('空数组 → empty（真空：开关没开，或刚开还没到周期）', () => {
    const s = summarizeThermalHistory([]);
    expect(s.state).toBe('empty');
    expect(s.peakC).toBeNull();
  });

  it('只有一个点 → single：有当值但没有走势可画', () => {
    const s = summarizeThermalHistory([p(71.4, 1000)]);
    expect(s.state).toBe('single');
    expect(s.count).toBe(1);
    expect(s.latestC).toBeCloseTo(71.4);
    expect(s.spanMinutes).toBe(0);
    expect(thermalSparkPath([p(71.4, 1000)])).toBe('');
  });

  it('多点 → ok：峰值与最新点分开给，跨度按分钟向下取整', () => {
    const s = summarizeThermalHistory([p(62, 0), p(91, 300), p(70, 900)]);
    expect(s.state).toBe('ok');
    expect(s.count).toBe(3);
    expect(s.peakC).toBe(91);
    expect(s.latestC).toBe(70);
    expect(s.spanMinutes).toBe(15);
  });

  it('乱序输入先按时间排序：峰值不变，最新点取时间最大的那条', () => {
    const s = summarizeThermalHistory([p(80, 600), p(65, 0), p(70, 300)]);
    expect(s.latestC).toBe(80);
    expect(s.spanMinutes).toBe(10);
  });

  it('NaN/Inf 温度不参与统计——它不是温度', () => {
    const s = summarizeThermalHistory([p(Number.NaN, 0), p(60, 300), p(Number.POSITIVE_INFINITY, 600)]);
    expect(s.count).toBe(1);
    expect(s.state).toBe('single');
    expect(s.peakC).toBe(60);
  });

  it('全是非法点等同真空，不算读失败', () => {
    const s = summarizeThermalHistory([p(Number.NaN, 0)]);
    expect(s.state).toBe('empty');
  });

  it('火花线只连接真实点，不用单点编一条走势', () => {
    expect(thermalSparkPath([])).toBe('');
    const line = thermalSparkPath([p(60, 0), p(70, 300), p(65, 600)]);
    expect(line.startsWith('M')).toBe(true);
    expect((line.match(/L/g) ?? []).length).toBe(2);
  });
});