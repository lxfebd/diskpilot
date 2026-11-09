// R537 持久化通知中心（纯前端 localStorage，cap 50，最新在前）。
// 周报/空间预警/扫描陈旧/自动导出都会入列，历史页展示；与 toast 不互斥（toast 是瞬时，这里是留档）。

export interface Notice {
  id: string;
  at: number;
  kind: 'weekly' | 'space-full' | 'scan-stale' | 'report-exported';
  title: string;
  body: string;
}

export const NOTICES_KEY = 'diskpilot:notices-v1';
export const NOTICES_CAP = 50;
/** R542：已读水位（时间戳），at > 水位的都是未读 */
export const NOTICES_READ_KEY = 'diskpilot:notices-read-at';

/** R542：未读数（纯函数）。 */
export function unreadCount(list: readonly Notice[], readAt: number): number {
  return list.filter((n) => n.at > readAt).length;
}

export function loadReadAt(): number {
  try {
    return Number(localStorage.getItem(NOTICES_READ_KEY)) || 0;
  } catch {
    return 0;
  }
}

/** R542：进入历史页时把水位推到现在（标已读）。 */
export function markNoticesRead(now = Date.now()): void {
  try {
    localStorage.setItem(NOTICES_READ_KEY, String(now));
  } catch { /* ignore */ }
}

const NOTICE_KINDS: readonly Notice['kind'][] = ['weekly', 'space-full', 'scan-stale', 'report-exported'];

export function loadNotices(): Notice[] {
  try {
    const raw = localStorage.getItem(NOTICES_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw);
    if (!Array.isArray(arr)) return [];
    // R549 审查 low-3：字段级校验——at 必须是有限数字、kind 必须在联合类型内、body 为 string，
    // 防止人为篡改 localStorage 产生 Invalid Date / KIND_KEY[undefined] / 隐式比较
    return arr.filter(
      (n): n is Notice =>
        !!n &&
        typeof n.id === 'string' &&
        typeof n.at === 'number' &&
        Number.isFinite(n.at) &&
        typeof n.kind === 'string' &&
        (NOTICE_KINDS as readonly string[]).includes(n.kind) &&
        typeof n.title === 'string' &&
        typeof n.body === 'string',
    );
  } catch {
    return [];
  }
}

export function saveNotices(list: Notice[]): void {
  try {
    localStorage.setItem(NOTICES_KEY, JSON.stringify(list.slice(0, NOTICES_CAP)));
  } catch { /* 存储满则静默放弃 */ }
}

/** 追加一条通知并持久化，返回新列表。纯函数部分拆出便于单测。 */
export function pushNotice(list: readonly Notice[], n: Omit<Notice, 'id' | 'at'>, now: number): Notice[] {
  const item: Notice = { ...n, id: `n${now}-${Math.random().toString(36).slice(2, 8)}`, at: now };
  return [item, ...list].slice(0, NOTICES_CAP);
}

/** R549 审查 low-6：同文档内通知事件——storage 事件只发给其他同源文档，
 *  历史页已挂载时收不到新通知；addNotice 额外派发该自定义事件，同文档监听即可即时刷新。 */
export const NOTICE_EVENT = 'diskpilot:notice-added';

export function addNotice(n: Omit<Notice, 'id' | 'at'>): Notice[] {
  const next = pushNotice(loadNotices(), n, Date.now());
  saveNotices(next);
  try {
    window.dispatchEvent(new CustomEvent(NOTICE_EVENT, { detail: next }));
  } catch { /* ignore */ }
  return next;
}

export function clearNotices(): void {
  try {
    localStorage.removeItem(NOTICES_KEY);
  } catch { /* ignore */ }
}

/** R547：通知类型筛选（纯函数）。'all' 原样返回；其余按 kind 过滤，保持原顺序。 */
export type NoticeFilter = 'all' | Notice['kind'];
export function filterNotices(list: readonly Notice[], f: NoticeFilter): Notice[] {
  if (f === 'all') return [...list];
  return list.filter((n) => n.kind === f);
}
