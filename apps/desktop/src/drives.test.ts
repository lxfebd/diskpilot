import { describe, expect, it } from 'vitest';
import { physicalDrives, scanTargets, volumeLetterOf } from './drives';

// 数据形状取自本机 list_drives 的真实结果：D: 不在 Get-Partition 里，
// 它是 `subst D: J:\D`——和 J: 是同一块 KIOXIA、同一份空间。
const DRIVES = [
  { path: 'C:\\', total_bytes: 100, used_bytes: 60, mapped_to: '' },
  { path: 'D:\\', total_bytes: 200, used_bytes: 180, mapped_to: 'J:\\D' },
  { path: 'E:\\', total_bytes: 100, used_bytes: 50, mapped_to: '' },
  { path: 'F:\\', total_bytes: 900, used_bytes: 300, mapped_to: '' },
  { path: 'G:\\', total_bytes: 200, used_bytes: 30, mapped_to: '' },
  { path: 'I:\\', total_bytes: 180, used_bytes: 60, mapped_to: '' },
  { path: 'J:\\', total_bytes: 200, used_bytes: 180, mapped_to: '' },
];

describe('volumeLetterOf（盘符 → 真正承载它的卷）', () => {
  it('映射盘解析成目标卷，普通盘原样', () => {
    expect(volumeLetterOf({ path: 'D:\\', mapped_to: 'J:\\D' })).toBe('J');
    expect(volumeLetterOf({ path: 'C:\\', mapped_to: '' })).toBe('C');
    expect(volumeLetterOf({ path: 'C:\\' })).toBe('C');
  });

  it('映射目标不是盘符路径时退回自身（不猜）', () => {
    expect(volumeLetterOf({ path: 'D:\\', mapped_to: '\\\\nas\\share' })).toBe('D');
  });

  it('非盘符路径返回空', () => {
    expect(volumeLetterOf({ path: '\\\\nas\\share' })).toBe('');
  });
});

describe('physicalDrives / scanTargets（同一块盘不数两遍）', () => {
  it('映射盘的目标卷在列表里时不再单独占一份', () => {
    expect(scanTargets(DRIVES)).toEqual(['C:\\', 'E:\\', 'F:\\', 'G:\\', 'I:\\', 'J:\\']);
  });

  it('去重保留真正的卷而不是映射盘（D 排在 J 前面也不能顶掉 J）', () => {
    const kept = physicalDrives(DRIVES);
    expect(kept.map((d) => d.path)).not.toContain('D:\\');
    expect(kept.map((d) => d.path)).toContain('J:\\');
  });

  it('目标卷不在列表里时保留映射盘（那是碰到那块空间的唯一入口）', () => {
    const partial = [
      { path: 'D:\\', total_bytes: 200, used_bytes: 180, mapped_to: 'K:\\D' },
      { path: 'C:\\', total_bytes: 100, used_bytes: 60, mapped_to: '' },
    ];
    expect(scanTargets(partial)).toEqual(['C:\\', 'D:\\']);
  });

  it('映射到 UNC 的盘保留自己（没有本地目标卷）', () => {
    const withUnc = [
      { path: 'D:\\', total_bytes: 200, used_bytes: 180, mapped_to: '\\\\nas\\share' },
    ];
    expect(scanTargets(withUnc)).toEqual(['D:\\']);
  });

  it('没有映射盘时列表原样（顺序不变、不去重非盘符路径）', () => {
    const plain = [
      { path: 'C:\\', total_bytes: 1, used_bytes: 1 },
      { path: 'E:\\', total_bytes: 1, used_bytes: 1 },
      { path: '\\\\nas\\share', total_bytes: 1, used_bytes: 1 },
      { path: '\\\\nas\\other', total_bytes: 1, used_bytes: 1 },
    ];
    expect(scanTargets(plain)).toEqual(['C:\\', 'E:\\', '\\\\nas\\share', '\\\\nas\\other']);
  });

  it('空列表给空结果', () => {
    expect(scanTargets([])).toEqual([]);
    expect(physicalDrives([])).toEqual([]);
  });
});
