import { describe, it, expect } from 'vitest';
import { healthRoutineSection } from './agent';

describe('healthRoutineSection（周巡检编排）', () => {
  it('包含巡检工具顺序与只读声明', () => {
    expect(healthRoutineSection).toContain('整机健康巡检');
    expect(healthRoutineSection).toContain('disk_health');
    expect(healthRoutineSection).toContain('hw_sensors');
    expect(healthRoutineSection).toContain('sys_boot_items');
    expect(healthRoutineSection).toContain('cleanup_suggestions');
    expect(healthRoutineSection).toContain('只读');
  });

  it('绝不引导调用写类/高负载工具（列在「绝不调用」名单里）', () => {
    for (const banned of [
      'process_kill',
      'file_recycle',
      'fan_control',
      'service_control',
      'toolbelt_run',
      'stress_test',
      'adjust_fan_curve',
      'propose_cleanup_plan',
    ]) {
      // 出现在禁止名单里（不在「可以调用」段），保证巡检路径零写操作
      expect(healthRoutineSection.toLowerCase()).toContain(banned.toLowerCase());
    }
    expect(healthRoutineSection).toContain('绝不调用');
  });

  it('要求结论标注数据是当前时刻实时采集', () => {
    expect(healthRoutineSection).toContain('当前时刻');
    expect(healthRoutineSection).toContain('不是预计算报表');
  });
});