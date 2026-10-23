import { describe, it, expect } from 'vitest';
import { TEMP_WARN_C, SCAN_STALE_WARN_DAYS, DEFAULT_THERMAL_SUSTAINED_C } from './thresholds';
import { isTempHigh } from './components/AssetOverview';

// R327：健康阈值必须收口在 thresholds.ts 一处，且与后端 hw_report/fan.rs 的
// `t >= 80.0`、collect_scan_stale 默认 21 天、temp_breaker 默认 85℃ 同口径。
// 下面的源码守卫防止任何人把数字写回组件里去。
describe('健康阈值唯一来源（R327）', () => {
  it('80℃ / 21 天 / 85℃ 三个默认值', () => {
    expect(TEMP_WARN_C).toBe(80);
    expect(SCAN_STALE_WARN_DAYS).toBe(21);
    expect(DEFAULT_THERMAL_SUSTAINED_C).toBe(85);
  });

  it('体检条/周报的温度判定走同一阈值（默认参数即 80）', () => {
    expect(isTempHigh(80)).toBe(true);
    expect(isTempHigh(79.9)).toBe(false);
    expect(isTempHigh(null)).toBe(false);
  });

  it('源码里不许再写死温度/时效/持续超温字面量（用常量）', () => {
    const RAW = import.meta.glob('./**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
    const bad: string[] = [];
    for (const [file, src] of Object.entries(RAW)) {
      if (file.includes('.test.') || file.includes('thresholds.ts') || file.includes('mocks')) continue;
      const lines = src.split('\n');
      lines.forEach((ln, i) => {
        if (/temp_max_c\s*>=\s*\d/.test(ln)) bad.push(file + ':' + (i + 1));
        if (/scan_stale_days\s*\?\?\s*\d/.test(ln)) bad.push(file + ':' + (i + 1));
        if (/thermal_sustained_c\s*\?\?\s*\d/.test(ln)) bad.push(file + ':' + (i + 1));
        if (/(?:peakC|tempC|temp)\s*>=\s*80(?!\d)/.test(ln) && !file.includes('thresholds')) bad.push(file + ':' + (i + 1) + ' (>= 80 literal)');
      });
    }
    expect(bad).toEqual([]);
  });
});
