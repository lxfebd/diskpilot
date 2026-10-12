// 硬盘健康原值分级 + 「物理盘 ↔ 盘符」映射：纯函数，无 React、无后端调用。
//
// 为什么要分级而不是直接显示原值：WMI `Get-PhysicalDisk` 的 HealthStatus 实测直出
// `Healthy` / `Warning` / `Unhealthy` / `Unknown`（本机 5 块盘全是 `Healthy`），只有
// CrystalDiskInfo 的 cdi_report 才用 `Good` / `Caution`。两套口径都认，认不出的一律
// 归 unknown——「没读到」不能画成「坏了」（HwPanels 曾把 Healthy 画成红色坏盘）。
//
// 为什么要盘符映射：用户问的是「我要清的这块 C 盘是哪块盘、它健康吗」。物理盘只有
// 型号名时对不上号，清理确认里也就无法提醒「这块盘正在坏」。

export type DiskHealthGrade = 'good' | 'caution' | 'bad' | 'unknown';

const HEALTH_GOOD_WORDS = ['healthy', 'ok', 'good', '良好', '正常']; // @i18n-keep 后端原值比对
const HEALTH_CAUTION_WORDS = ['warning', 'caution', '注意']; // @i18n-keep 后端原值比对
const HEALTH_BAD_WORDS = [
  'unhealthy',
  'nonrecoverable',
  'unrecoverable',
  'bad',
  'failed',
  'failure',
  '不良', // @i18n-keep 后端原值比对
  '损坏', // @i18n-keep 后端原值比对
];

/** 健康原值 → 分级；空值 / Unknown / 没读到一律 unknown。 */
export function gradeDiskHealth(raw: unknown): DiskHealthGrade {
  const v = String(raw ?? '').trim().toLowerCase();
  if (!v) return 'unknown';
  if (HEALTH_GOOD_WORDS.includes(v)) return 'good';
  if (HEALTH_CAUTION_WORDS.includes(v)) return 'caution';
  if (HEALTH_BAD_WORDS.includes(v)) return 'bad';
  return 'unknown';
}

/** 一块物理盘覆盖的盘符（后端 `drive_letters` 原值形如 `"F,G"`；映射不到为空）。 */
export function driveLettersOf(disk: unknown): string[] {
  const raw = String((disk as Record<string, unknown>)?.drive_letters ?? '');
  return [...new Set(raw.split(',').map((s) => s.trim().toUpperCase()).filter((s) => s.length === 1))];
}

/** 健康状态可疑（caution / bad）的物理盘。unknown 不算可疑：没读到不等于坏。 */
export interface RiskyDrive {
  /** 型号名（FriendlyName），用于告诉用户「是哪块盘」。 */
  friendly: string;
  /** 该盘覆盖的盘符（大写单字母）。映射不到时为空，此时无法据此拦截。 */
  letters: string[];
  grade: 'caution' | 'bad';
}

export function riskyDrives(disks: unknown[]): RiskyDrive[] {
  const out: RiskyDrive[] = [];
  for (const d of disks) {
    const grade = gradeDiskHealth((d as Record<string, unknown>)?.health);
    if (grade !== 'caution' && grade !== 'bad') continue;
    out.push({
      friendly: String((d as Record<string, unknown>)?.friendly ?? '') || '?',
      letters: driveLettersOf(d),
      grade,
    });
  }
  return out;
}

/**
 * 路径 → 盘符单字母。
 *
 * 不用 format.ts 的 `driveLetter`：它只剥尾部 `\` / `:`（给「盘符卡片」那种
 * 已经是盘符的输入用），`C:\Users\x` 原样返回整条路径，判不出落在哪块盘。
 * 这里只认 `X:\` / `X:/` 开头（含裸 `X:`），UNC（`\\nas\share`）一律不认——
 * 网络共享没有本地物理盘，不能凭型号名瞎拦。
 */
export function pathDriveLetter(p: string): string {
  const m = /^([A-Za-z]):(?:[\\/]|$)/.exec(p);
  return m ? m[1].toUpperCase() : '';
}

/** 这批路径落在哪些可疑盘上（空 = 没有可疑盘，或可疑盘映射不到盘符）。 */
export function pathsOnRiskyDrives(disks: unknown[], paths: string[]): RiskyDrive[] {
  const risky = riskyDrives(disks);
  if (risky.length === 0) return [];
  const hit = new Set<string>();
  for (const p of paths) {
    const l = pathDriveLetter(p);
    if (l) hit.add(l);
  }
  if (hit.size === 0) return [];
  return risky.filter((d) => d.letters.some((l) => hit.has(l)));
}
