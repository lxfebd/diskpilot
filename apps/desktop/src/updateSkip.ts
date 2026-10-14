// R158：「跳过此版本」——用户不想装某个版本时，每次打开设置都被提示很烦；
// 但跳过必须是**版本相关**的：出现更新的版本仍需提示（不能永久静音）。
// 纯函数 + localStorage 键，便于单测。

export const UPDATE_SKIP_KEY = 'settings.updateSkipped';

/** R161：跳过的有效期多久（天）——跳过不该是永久静音：同版本躺了 90 天
 *  还没新版，仍应再提醒一次（安全补丁可能重要，用户也可能改主意了）。 */
export const UPDATE_SKIP_DAYS = 90;

/** R161：跳过记录的存储格式（版本 + 跳过时刻 ms）。旧格式（纯版本串）仍能读。 */
export function parseSkipRecord(raw: string | null): { version: string; atMs: number } | null {
  if (!raw) return null;
  const i = raw.indexOf('@');
  if (i < 0) return { version: raw, atMs: 0 }; // 旧格式：无时刻 → 视为不过期
  const version = raw.slice(0, i);
  const atMs = Number.parseInt(raw.slice(i + 1), 10);
  if (!version) return null;
  return { version, atMs: Number.isFinite(atMs) && atMs > 0 ? atMs : 0 };
}

/** R161：序列化跳过记录。 */
export function formatSkipRecord(version: string, atMs: number): string {
  return `${version}@${atMs}`;
}

/** 版本号比较：只比较点分数字段，缺位补 0；非数字段按 0 处理（不抛）。
 *  返回 -1 / 0 / 1。用于判断「跳过的版本是否已被更新版本超过」。 */
export function compareVersions(a: string, b: string): number {
  const pa = a.split('.').map((s) => Number.parseInt(s, 10));
  const pb = b.split('.').map((s) => Number.parseInt(s, 10));
  const n = Math.max(pa.length, pb.length);
  for (let i = 0; i < n; i += 1) {
    const x = Number.isFinite(pa[i]) ? pa[i] : 0;
    const y = Number.isFinite(pb[i]) ? pb[i] : 0;
    if (x !== y) return x > y ? 1 : -1;
  }
  return 0;
}

/** 是否还该提示这个版本：
 *  - 没跳过 → 提示；
 *  - latest 严格新于跳过版本 → 提示（有更新版了）；
 *  - 跳过已超过 UPDATE_SKIP_DAYS 天 → 提示（跳过不是永久静音）；
 *  - 否则不提示（用户已明确跳过这个版本）。
 *  skippedAtMs 缺省/0 视为不过期（兼容 R158 的旧记录）。 */
export function shouldPromptUpdate(
  skipped: string | null,
  latest: string,
  skippedAtMs: number = 0,
  nowMs: number = Date.now(),
): boolean {
  if (!skipped) return true;
  if (compareVersions(latest, skipped) > 0) return true;
  if (skippedAtMs > 0 && nowMs - skippedAtMs >= UPDATE_SKIP_DAYS * 86_400_000) return true;
  return false;
}

/** R159：社区条目是否有比已装版本更新的版本——按 id 匹配已装清单，
 *  远端版本严格更新才算「有新版」（相等/更旧都不提示）。
 *  纯函数：未装、缺版本号、id 对不上 → false（不误报更新）。 */
export function hasRegistryUpdate(
  plugin: { id: string; version?: string | null },
  installed: readonly { id: string; version: string }[],
): boolean {
  const ver = plugin.version;
  if (!ver) return false;
  const cur = installed.find((p) => p.id === plugin.id);
  if (!cur) return false;
  return compareVersions(ver, cur.version) > 0;
}