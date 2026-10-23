import { describe, it, expect } from 'vitest';
import { withRule, normalizeReminderCfg } from './ReminderSettings';
import type { AutoListRule } from '../api';

// R298：热管理与启动项监听的 5 个旋钮此前只有后端、前端零入口。
// 归一化函数的职责是「补缺不覆盖」——旧后端配置缺字段时不能把
// undefined 传给受控 checkbox，也不能抹掉用户已经配好的值。
describe('normalizeReminderCfg（配置归一化）', () => {
  it('旧后端配置缺 thermal/startup 字段时补后端同口径默认值', () => {
    const out = normalizeReminderCfg({
      enabled: true,
      interval_hours: 12,
      min_bytes: 100 * 1024 * 1024,
      weekly_report_enabled: false,
      weekly_interval_days: 7,
      full_warn_days: 0,
      scan_stale_days: 21,
    });
    expect(out).not.toBeNull();
    expect(out!.thermal_sample_enabled).toBe(false);
    expect(out!.thermal_sustained_c).toBe(85);
    expect(out!.thermal_sustained_samples).toBe(5);
    expect(out!.startup_watch_enabled).toBe(false);
    expect(out!.startup_watch_minutes).toBe(60);
  });

  it('已有字段一律保留，绝不用默认值覆盖用户配置', () => {
    const out = normalizeReminderCfg({
      enabled: true,
      interval_hours: 6,
      min_bytes: 50 * 1024 * 1024,
      weekly_report_enabled: true,
      weekly_interval_days: 14,
      full_warn_days: 3,
      scan_stale_days: 7,
      thermal_sample_enabled: true,
      thermal_sustained_c: 90,
      thermal_sustained_samples: 10,
      startup_watch_enabled: true,
      startup_watch_minutes: 240,
    });
    expect(out).toEqual({
      enabled: true,
      interval_hours: 6,
      min_bytes: 50 * 1024 * 1024,
      weekly_report_enabled: true,
      weekly_interval_days: 14,
      full_warn_days: 3,
      scan_stale_days: 7,
      thermal_sample_enabled: true,
      thermal_sustained_c: 90,
      thermal_sustained_samples: 10,
      startup_watch_enabled: true,
      startup_watch_minutes: 240,
    });
  });

  it('null/undefined 返回 null（读失败 ≠ 零配置）', () => {
    expect(normalizeReminderCfg(null)).toBeNull();
    expect(normalizeReminderCfg(undefined)).toBeNull();
  });

  it('0 和 false 是用户显式选择，不能被 ?? 当成缺省吞掉', () => {
    const out = normalizeReminderCfg({
      enabled: false,
      interval_hours: 0,
      min_bytes: 0,
      weekly_report_enabled: false,
      weekly_interval_days: 0,
      full_warn_days: 0,
      scan_stale_days: 0,
      thermal_sample_enabled: false,
      thermal_sustained_c: 0,
      thermal_sustained_samples: 0,
      startup_watch_enabled: false,
      startup_watch_minutes: 0,
    });
    expect(out!.interval_hours).toBe(0);
    expect(out!.scan_stale_days).toBe(0);
    expect(out!.thermal_sustained_c).toBe(0);
    expect(out!.startup_watch_minutes).toBe(0);
  });
});

// health-butler P2-1 规则变换的纯函数单测：与后端 auto_list.rs 的
// should_auto_list（同 scaffold 取最低阈值）语义对齐，防前端与后端分叉。
function r(id: string, mb: number): AutoListRule {
  return { scaffold_id: id, min_bytes: mb * 1024 * 1024 };
}

describe('withRule（按项授权规则变换）', () => {
  it('add 插入新规则并按 scaffold_id 排序', () => {
    const out = withRule([r('b', 100)], 'add', { scaffold_id: 'a', min_bytes: 50 * 1024 * 1024 });
    expect(out.map((x) => x.scaffold_id)).toEqual(['a', 'b']);
    expect(out[0].min_bytes).toBe(50 * 1024 * 1024);
  });

  it('add 同 id 重复授权 = 替换阈值（保留两条会让后端按最小阈值命中，新填的大值被静默吞掉）', () => {
    const out = withRule([r('a', 500)], 'add', { scaffold_id: 'a', min_bytes: 1000 * 1024 * 1024 });
    expect(out).toHaveLength(1);
    expect(out[0].min_bytes).toBe(1000 * 1024 * 1024);
  });

  it('add 同 id 降阈值也生效（改小）', () => {
    const out = withRule([r('a', 1000)], 'add', { scaffold_id: 'a', min_bytes: 50 * 1024 * 1024 });
    expect(out).toHaveLength(1);
    expect(out[0].min_bytes).toBe(50 * 1024 * 1024);
  });

  it('remove 按 scaffold_id 过滤，其余保持原样', () => {
    const out = withRule([r('a', 100), r('b', 200)], 'remove', { scaffold_id: 'a' });
    expect(out).toEqual([r('b', 200)]);
  });

  it('remove 不存在的 id 幂等（空操作）', () => {
    const out = withRule([r('a', 100)], 'remove', { scaffold_id: 'zzz' });
    expect(out).toEqual([r('a', 100)]);
  });

  it('add 缺 min_bytes 时原样返回（防御：不能造出无阈值规则）', () => {
    const out = withRule([r('a', 100)], 'add', { scaffold_id: 'b' });
    expect(out).toEqual([r('a', 100)]);
  });

  it('空规则集上 add 产生单条（阈值原样保留，含 1 字节这种边界）', () => {
    const out = withRule([], 'add', { scaffold_id: 'a', min_bytes: 1 });
    expect(out).toEqual([{ scaffold_id: 'a', min_bytes: 1 }]);
  });
});
