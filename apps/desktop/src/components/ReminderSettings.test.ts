import { describe, it, expect } from 'vitest';
import { withRule } from './ReminderSettings';
import type { AutoListRule } from '../api';

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
