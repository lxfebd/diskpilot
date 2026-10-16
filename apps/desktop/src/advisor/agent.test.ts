import { describe, it, expect, vi } from 'vitest';
import { healthRoutineSection, toolLabel, previewSystemNote } from './agent';
import { toolRegistry } from './tools';

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

// R281：toolLabel 给所有注册工具提供中文标签——放行工具（含预览放行的
// get_fan_status）不能被 prettify 兜底成英文裸名，trace/操作历史里要
// 显示中文可读标签。
describe('toolLabel（工具名中文标签）', () => {
  it('每个 toolRegistry 工具都有非空标签（不直显 snake_case）', () => {
    const labels = toolRegistry.map((t) => toolLabel(t.name));
    for (let i = 0; i < toolRegistry.length; i++) {
      const l = labels[i];
      expect(l, toolRegistry[i].name).toBeTruthy();
      expect(l.toLowerCase(), toolRegistry[i].name).not.toContain('_');
    }
  });

  it('预览放行的 get_fan_status 有专门中文标签', () => {
    expect(toolLabel('get_fan_status')).toBe('风扇控制状态读取');
  });

  it('未见过的工具名优雅降级为可读短语', () => {
    expect(toolLabel('get_foo_bar')).toBe('FOO BAR');
  });
});

// R282：预览模式数据诚实声明——node 测试环境无 window → isTauri=false，
// previewSystemNote 必须返回演示数据声明；桌面端（window.__TAURI_INTERNALS__
// 存在）返回空（真实数据无需声明）。
describe('previewSystemNote（预览模式数据声明）', () => {
  it('非 Tauri（node 环境）返回演示数据声明', () => {
    const note = previewSystemNote();
    expect(note).toContain('浏览器预览模式');
    expect(note).toContain('演示数据');
    expect(note).toContain('不得说成');
  });

  it('Tauri 环境返回空（真实数据无需声明）', () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: { dummy: true } });
    try {
      expect(previewSystemNote()).toBe('');
    } finally {
      vi.unstubAllGlobals();
    }
  });
});