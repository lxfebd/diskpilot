// 操作历史纯逻辑（P2c）：动作过滤 / 关键词过滤 / 汇总 / 按天分组。
// 无 React、无模块级 t()——文案键由组件在渲染点求值（i18n 铁律）。
import type { UndoEntry } from '../types';
import { formatDateKey } from '../format';

export type UndoActionFilter = 'all' | UndoEntry['action'];

export interface UndoFilter {
  action: UndoActionFilter;
  /** 路径 / 原因关键词，大小写不敏感；trim 后为空 = 不过滤 */
  query: string;
}

export interface UndoSummary {
  total: number;
  /** 后端实测释放字节之和；所有条目都缺实测值时为 null（界面不显示累计） */
  bytesFreed: number | null;
  byAction: Record<UndoEntry['action'], number>;
  /** R149：台账覆盖的时间范围（最新 / 最早一条的时间戳）；空台账或全部时间戳
   *  不可解析时为 null——界面据此显示「最早 …」，给累计数字一个时间尺度。 */
  newestAt: string | null;
  oldestAt: string | null;
}

/** 动作过滤 + 关键词过滤（匹配 source / destination / reason）。 */
export function filterUndoEntries(entries: UndoEntry[], filter: UndoFilter): UndoEntry[] {
  const q = filter.query.trim().toLowerCase();
  return entries.filter((e) => {
    if (filter.action !== 'all' && e.action !== filter.action) return false;
    if (!q) return true;
    return (
      e.source.toLowerCase().includes(q) ||
      (e.destination ?? '').toLowerCase().includes(q) ||
      e.reason.toLowerCase().includes(q)
    );
  });
}

export function summarizeUndo(entries: UndoEntry[]): UndoSummary {
  const byAction: Record<UndoEntry['action'], number> = { recycle: 0, quarantine: 0, delete: 0 };
  let bytes = 0;
  let bytesKnown = false;
  // R149：时间范围按可解析时间戳取 min/max（不依赖列表顺序——调用方可能传
  // 过滤后的子集或另一种顺序）。
  let newest: number | null = null;
  let oldest: number | null = null;
  for (const e of entries) {
    byAction[e.action] += 1;
    if (e.bytes_freed != null) {
      bytes += e.bytes_freed;
      bytesKnown = true;
    }
    const ts = Date.parse(e.timestamp);
    if (Number.isFinite(ts)) {
      if (newest === null || ts > newest) newest = ts;
      if (oldest === null || ts < oldest) oldest = ts;
    }
  }
  return {
    total: entries.length,
    bytesFreed: bytesKnown ? bytes : null,
    byAction,
    newestAt: newest === null ? null : new Date(newest).toISOString(),
    oldestAt: oldest === null ? null : new Date(oldest).toISOString(),
  };
}

export interface UndoDayGroup {
  /** YYYY-MM-DD（本地时区）；时间戳解析失败时为空串 */
  key: string;
  items: UndoEntry[];
}

function dayKeyOf(ts: string): string {
  return formatDateKey(new Date(ts));
}

/** 按本地日期分组；输入须是「最新在前」的列表（后端 list_undo 已倒序），组保持出现顺序。 */
export function groupUndoByDay(entries: UndoEntry[]): UndoDayGroup[] {
  const groups: UndoDayGroup[] = [];
  const byKey = new Map<string, UndoDayGroup>();
  for (const e of entries) {
    const key = dayKeyOf(e.timestamp);
    let g = byKey.get(key);
    if (!g) {
      g = { key, items: [] };
      byKey.set(key, g);
      groups.push(g);
    }
    g.items.push(e);
  }
  return groups;
}

/** 分组头标签：今天 / 昨天走 i18n 键，其余直接显示 YYYY-MM-DD（地区中立）。 */
export function dayGroupLabel(key: string, now: Date, t: (k: string) => string): string {
  if (!key) return t('history.unknownDay');
  if (key === dayKeyOf(now.toISOString())) return t('history.today');
  const yesterday = new Date(now.getTime() - 86_400_000);
  if (key === dayKeyOf(yesterday.toISOString())) return t('history.yesterday');
  return key;
}

/** R155：台账里最近一次操作的年龄档位——预览窗在用户决定删除前告知
 *  「上次清理是多久前」，避免刚清完又清一遍。纯函数：返回 i18n 键（文案由
 *  组件求值，符合 i18n 铁律）；无有效时间戳 → null（不编造）。 */
export type LastCleanKey = 'history.ageToday' | 'history.ageDays' | 'history.ageOld';

export function lastCleanAgeKey(
  entries: readonly { timestamp: string }[],
  nowMs: number = Date.now(),
): { key: LastCleanKey; n?: number } | null {
  let newest: number | null = null;
  for (const e of entries) {
    const ts = Date.parse(e.timestamp);
    if (Number.isFinite(ts) && (newest === null || ts > newest)) newest = ts;
  }
  if (newest === null || newest <= 0) return null;
  const days = Math.floor((nowMs - newest) / 86_400_000);
  if (days <= 0) return { key: 'history.ageToday' };
  if (days <= 30) return { key: 'history.ageDays', n: days };
  return { key: 'history.ageOld' };
}

/**
 * 还原出口口径（与后端一致，别在组件里各写各的）：
 * - `inline`：quarantine —— 东西还在隔离区，应用内一键还原（`api.undo`）；
 * - `osRecycleBin`：recycle —— 东西在**系统回收站**，应用内还原不了
 *   （后端 `restore_quarantined` 明确拒绝非隔离区条目），只能去系统回收站还原；
 * - `none`：delete —— 按设计不可还原。
 *
 * 为什么要有这个函数：台账里写着「可撤销」，但界面上 recycle 条目以前什么都不说，
 * 用户看不到出口就等于没出口。
 */
export type UndoRestoreKind = 'inline' | 'osRecycleBin' | 'none';

export function restoreKind(e: UndoEntry): UndoRestoreKind {
  if (e.action === 'quarantine') return 'inline';
  if (e.action === 'recycle') return 'osRecycleBin';
  return 'none';
}
