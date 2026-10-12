// 硬盘健康原值分级 + 「物理盘 ↔ 盘符」映射：纯函数，无 React、无后端调用。
//
// 为什么要分级而不是直接显示原值：WMI `Get-PhysicalDisk` 的 HealthStatus 实测直出
// `Healthy` / `Warning` / `Unhealthy` / `Unknown`（本机 5 块盘全是 `Healthy`），只有
// CrystalDiskInfo 的 cdi_report 才用 `Good` / `Caution`。两套口径都认，认不出的一律
// 归 unknown——「没读到」不能画成「坏了」（HwPanels 曾把 Healthy 画成红色坏盘）。
//
// 为什么要盘符映射：用户问的是「我要清的这块 C 盘是哪块盘、它健康吗」。物理盘只有
// 型号名时对不上号，清理确认里也就无法提醒「这块盘正在坏」。

import { formatBytes } from './format';

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

/**
 * 映射盘（`subst D: J:\D` 这类）：它不是独立物理盘，空间和目标卷是同一份。
 * 后端 hw_disk_health 的 `substs` 原值形如 `{ letter: 'D', target: 'J:\\D' }`。
 */
export interface DriveAlias {
  letter: string;
  target: string;
}

/** 健康对象里的 substs → 归一化别名表（字母大写、去重、缺 target 的丢掉）。 */
export function driveAliasesOf(health: unknown): DriveAlias[] {
  const raw = (health as Record<string, unknown> | null)?.substs;
  if (!Array.isArray(raw)) return [];
  const out: DriveAlias[] = [];
  const seen = new Set<string>();
  for (const s of raw) {
    const letter = String((s as Record<string, unknown>)?.letter ?? '').trim().toUpperCase();
    const target = String((s as Record<string, unknown>)?.target ?? '').trim();
    if (letter.length !== 1 || !target) continue;
    if (seen.has(letter)) continue;
    seen.add(letter);
    out.push({ letter, target });
  }
  return out;
}

/**
 * 盘符 → 真正承载它的盘符：映射盘（`D:` 是 `J:\D` 的 subst）解析成目标卷的字母。
 * 解析不出目标卷（target 不是盘符路径）时退回原字母，不猜。
 */
export function resolveLetter(letter: string, aliases: DriveAlias[]): string {
  const up = letter.trim().toUpperCase();
  if (up.length !== 1) return '';
  const alias = aliases.find((a) => a.letter === up);
  if (!alias) return up;
  return pathDriveLetter(alias.target) || up;
}

/**
 * 路径 → 真正承载它的盘符。
 *
 * 映射盘必须解析：`D:` 若是 `J:\D` 的 subst，文件其实躺在 J: 那块盘上——
 * 不解析就会「D 没有映射到任何物理盘」而放过一次对坏盘的批量清理。
 */
export function resolveDriveLetter(p: string, aliases: DriveAlias[]): string {
  return resolveLetter(pathDriveLetter(p), aliases);
}

/** 这批路径落在哪些可疑盘上（空 = 没有可疑盘，或可疑盘映射不到盘符）。 */
export function pathsOnRiskyDrives(
  disks: unknown[],
  paths: string[],
  aliases: DriveAlias[] = [],
): RiskyDrive[] {
  const risky = riskyDrives(disks);
  if (risky.length === 0) return [];
  const hit = new Set<string>();
  for (const p of paths) {
    const l = resolveDriveLetter(p, aliases);
    if (l) hit.add(l);
  }
  if (hit.size === 0) return [];
  return risky.filter((d) => d.letters.some((l) => hit.has(l)));
}

const GRADE_LABELS: Record<DiskHealthGrade, string> = {
  good: '健康',
  caution: '注意：SMART 有告警',
  bad: '不良：可能正在坏，先提醒备份',
  unknown: '未上报：不等于坏，别当成坏盘',
};

/**
 * 把 hw_disk_health 的原始 JSON 渲染成给 AI 的文本。
 *
 * 为什么不让 AI 直接吃 JSON：JSON 里 `health: "Unknown"` 和一堆 `null` 很容易被模型
 * 读成「这块盘坏了」，或者凭常识补一个通电小时数（UI 侧已经犯过一次这个错）。
 * 这里统一成「分级 + 原值 + 盘符」，读不到的字段一律写「未读到」，模型照抄即可。
 */
export function renderDiskHealthText(health: unknown): string {
  const h = (health ?? {}) as Record<string, unknown>;
  const disks = Array.isArray(h.disks) ? h.disks : [];
  if (disks.length === 0) {
    return '未读到任何物理盘健康信息（Get-PhysicalDisk 不可用）。不要凭猜测回答硬盘状况。';
  }
  const out: string[] = [`物理盘 ${disks.length} 块（读不到的字段都写了「未读到」，不要猜）：`];
  for (const d of disks) {
    const rec = (d ?? {}) as Record<string, unknown>;
    const letters = driveLettersOf(rec);
    const raw = String(rec.health ?? '').trim();
    const grade = gradeDiskHealth(raw);
    const kind = [String(rec.media ?? '').trim(), String(rec.bus ?? '').trim()].filter(Boolean).join(' / ');
    const size = typeof rec.size === 'number' && rec.size > 0 ? formatBytes(rec.size) : '容量未读到';
    const smart = [
      typeof rec.power_on_hours === 'number' ? `通电 ${rec.power_on_hours} 小时` : '通电时间未读到',
      typeof rec.temperature_c === 'number' ? `${rec.temperature_c}°C` : '温度未读到',
      typeof rec.wear_pct === 'number' ? `磨损 ${rec.wear_pct}%` : '磨损未读到',
      typeof rec.read_errors === 'number' || typeof rec.write_errors === 'number'
        ? `读错误 ${typeof rec.read_errors === 'number' ? rec.read_errors : '未读到'} / 写错误 ${typeof rec.write_errors === 'number' ? rec.write_errors : '未读到'}`
        : '读写错误未读到',
    ];
    out.push(
      `- ${letters.length ? letters.join(', ') + ': ' : ''}${String(rec.friendly ?? '') || '未知型号'}：` +
        `${kind || '类型未读到'}，${size}，健康状态 ${raw || '未上报'}（${GRADE_LABELS[grade]}）`,
    );
    out.push(`    ${smart.join(' · ')}`);
  }
  const aliases = driveAliasesOf(h);
  if (aliases.length > 0) {
    out.push(
      `映射盘：${aliases.map((a) => `${a.letter}: → ${a.target}`).join('，')}` +
        '（不是独立物理盘，空间与目标卷同一份，别按两块盘算）',
    );
  }
  if (h.reliability === false) {
    out.push('SMART 可靠性计数器读不到（非管理员权限常见）：通电/温度/磨损/读写错误都会是「未读到」。');
  }
  const cdi = Array.isArray(h.cdi_report) ? h.cdi_report : [];
  if (cdi.length > 0) {
    out.push('CrystalDiskInfo 补充（口径与 WMI 不同，它用 Good/Caution）：');
    for (const c of cdi) {
      const rec = (c ?? {}) as Record<string, unknown>;
      const kv = Object.entries(rec)
        .filter(([k, v]) => k !== 'device' && typeof v === 'string' && v !== '')
        .map(([k, v]) => `${k} ${v}`);
      out.push(`- ${String(rec.device ?? '') || '未知设备'}${kv.length ? '：' + kv.join(' · ') : '：没有可读字段'}`);
    }
  }
  if (typeof h.note === 'string' && h.note !== '') out.push(h.note);
  return out.join('\n');
}
