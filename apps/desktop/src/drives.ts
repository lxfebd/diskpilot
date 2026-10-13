// 盘符列表的「同一块物理盘」口径：纯函数，无 React、无后端调用。
//
// 为什么要这一层：`subst D: J:\D` 这类映射盘在 GetLogicalDrives 里是一个正经盘符，
// 但它没有自己的分区与空间（本机实测：Get-Partition / Get-Volume 里都没有 D:）。
// 不识别就会把同一块盘数两遍——「扫描全部」把 J:\D 走两遍、总占用与可回收翻倍、
// 体检里「2 块盘空间告急」其实是一块盘。

import { pathDriveLetter } from './diskHealth';

export interface DriveEntry {
  path: string;
  /** 映射盘（subst）的目标路径，普通盘为空/缺省。 */
  mapped_to?: string;
}

function mappedTarget(d: DriveEntry): string {
  return (d.mapped_to ?? '').trim();
}

/** 这个盘符真正落在哪个卷上：映射盘解析成目标卷字母，解析不出时退回自身。 */
export function volumeLetterOf(d: DriveEntry): string {
  const self = pathDriveLetter(d.path);
  if (!self) return '';
  const target = mappedTarget(d);
  if (!target) return self;
  return pathDriveLetter(target) || self;
}

/**
 * 按物理卷去重后的盘列表。
 *
 * 规则：先收「真正的卷」（没有映射的条目）——映射盘代表不了整块卷，
 * 只有当目标卷不在列表里时，映射盘才是碰到那块空间的唯一入口，才保留它。
 * 非盘符路径（UNC 等）原样保留，不去重也不猜。
 */
export function physicalDrives<T extends DriveEntry>(drives: readonly T[]): T[] {
  const listed = new Set<string>();
  for (const d of drives) {
    const l = pathDriveLetter(d.path);
    if (l) listed.add(l);
  }
  const out: T[] = [];
  const seen = new Set<string>();
  for (const d of drives) {
    if (mappedTarget(d)) continue;
    const l = pathDriveLetter(d.path);
    if (!l) {
      out.push(d);
      continue;
    }
    if (seen.has(l)) continue;
    seen.add(l);
    out.push(d);
  }
  for (const d of drives) {
    if (!mappedTarget(d)) continue;
    const self = pathDriveLetter(d.path);
    if (!self) continue;
    const real = volumeLetterOf(d);
    // 目标卷自己就在列表里：那块空间会被扫/被算一遍，映射盘不再单独占一份。
    if (real !== self && listed.has(real)) continue;
    if (seen.has(real)) continue;
    seen.add(real);
    out.push(d);
  }
  return out;
}

/** 「扫描全部」的扫描目标：同一块物理盘只扫一次。 */
export function scanTargets(drives: readonly DriveEntry[]): string[] {
  return physicalDrives(drives).map((d) => d.path);
}

/**
 * 全机已用空间：按物理卷求和。
 *
 * 直接对盘符列表求和会把 `subst` 映射盘的那份空间数两遍（它和 J: 是同一块盘），
 * 页脚「已用 X」就会虚高。缺 used_bytes 的条目按 0 计，不猜。
 */
export function physicalUsedBytes(drives: readonly (DriveEntry & { used_bytes?: number })[]): number {
  return physicalDrives(drives).reduce((s, d) => s + (d.used_bytes || 0), 0);
}
