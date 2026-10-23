import { describe, expect, it } from 'vitest';
import {
  physicalDrives,
  physicalUsedBytes,
  scanTargets,
  volumeLetterOf,
  driveUsagePct,
  DRIVE_WARN_PCT,
  DRIVE_CAUTION_PCT,
  DRIVE_LOW_PCT,
} from './drives';

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

describe('physicalUsedBytes（全机已用按物理卷求和）', () => {
  it('映射盘不再把同一块盘的空间数两遍', () => {
    // 本机口径：D: 是 subst 到 J:\D，J: 自己也在列表里 → D: 不占一份。
    expect(physicalUsedBytes(DRIVES)).toBe(60 + 50 + 300 + 30 + 60 + 180);
    // 对照：直接对盘符求和就是那个虚高的数字（页脚此前显示的正是它）。
    expect(DRIVES.reduce((s, d) => s + d.used_bytes, 0)).toBe(860);
  });

  it('目标卷不在列表里时，映射盘是碰到那块空间的唯一入口，要算它', () => {
    const onlyMapped = [
      { path: 'C:\\', total_bytes: 100, used_bytes: 60, mapped_to: '' },
      { path: 'D:\\', total_bytes: 200, used_bytes: 180, mapped_to: 'J:\\D' },
    ];
    expect(physicalUsedBytes(onlyMapped)).toBe(240);
  });

  it('缺 used_bytes 按 0 计，不猜', () => {
    expect(physicalUsedBytes([{ path: 'C:\\' }, { path: 'E:\\', used_bytes: 7 }])).toBe(7);
  });
});

// R326：体检条「红盘 N 块」、总览环形图、盘卡片那条进度色必须同一套口径。
// 旧实现里盘卡片先 Math.round 再比 85，84.6% 的盘被画成红的，而体检条按未取整算是 0 块。
describe('盘使用率阈值口径（R326）', () => {
  it('84.6% 取整是 85 但不是红盘——判定不吃取整', () => {
    const pct = driveUsagePct(846, 1000);
    expect(pct).toBeCloseTo(84.6, 5);
    expect(Math.round(pct)).toBe(85); // 取整后就会误判，正是旧 bug
    expect(pct).toBeLessThan(DRIVE_WARN_PCT);
  });

  it('阈值边界：恰好 85% 才算红盘，84.9% 不算', () => {
    expect(driveUsagePct(850, 1000)).toBeGreaterThanOrEqual(DRIVE_WARN_PCT);
    expect(driveUsagePct(849, 1000)).toBeLessThan(DRIVE_WARN_PCT);
  });

  it('caution / low 档沿用 60 / 95，与盘卡片一致', () => {
    expect(DRIVE_WARN_PCT).toBe(85);
    expect(DRIVE_CAUTION_PCT).toBe(60);
    expect(DRIVE_LOW_PCT).toBe(95);
  });

  it('容量没读到时返回 0（调用方另有 capacityKnown 判断，不当作 0% 健康）', () => {
    expect(driveUsagePct(5, 0)).toBe(0);
    expect(driveUsagePct(0, 0)).toBe(0);
  });
});

