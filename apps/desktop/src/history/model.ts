// 操作历史纯逻辑（P2c）：动作过滤 / 关键词过滤 / 汇总 / 按天分组。
// 无 React、无模块级 t()——文案键由组件在渲染点求值（i18n 铁律）。
import type { UndoEntry } from '../types';

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
  for (const e of entries) {
    byAction[e.action] += 1;
    if (e.bytes_freed != null) {
      bytes += e.bytes_freed;
      bytesKnown = true;
    }
  }
  return { total: entries.length, bytesFreed: bytesKnown ? bytes : null, byAction };
}

export interface UndoDayGroup {
  /** YYYY-MM-DD（本地时区）；时间戳解析失败时为空串 */
  key: string;
  items: UndoEntry[];
}

function dayKeyOf(ts: string): string {
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
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
