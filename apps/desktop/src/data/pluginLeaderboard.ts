/**
 * R546：官方插件下载排行快照（静态数据，采集自 lxfebd/diskpilot-plugin-index
 * 各 release zip asset 的 download_count，GitHub API 2026-11-08 采集，合计 125 次；
 * 详见 docs/plugin-downloads.md）。数字随时间增长，UI 只做展示，不做实时拉取。
 */

export interface PluginDlStat {
  /** 插件 id（与索引/安装后的 id 一致） */
  id: string;
  /** 采集时最新发布版本 */
  latest: string;
  /** 最新版本 zip 的下载数 */
  latestDl: number;
  /** 该插件全部历史版本累计下载数 */
  totalDl: number;
  /** 分类（硬盘工具 / 综合检测） */
  category: 'harddisk' | 'audit';
}

export interface PluginDlSnapshot {
  collectedAt: string;
  total: number;
  items: readonly PluginDlStat[];
}

export const PLUGIN_DL_SNAPSHOT: PluginDlSnapshot = {
  collectedAt: '2026-11-08',
  total: 125,
  items: [
    { id: 'bigfile-finder', latest: '1.0.0', latestDl: 16, totalDl: 16, category: 'harddisk' },
    { id: 'dup-finder', latest: '1.0.0', latestDl: 15, totalDl: 15, category: 'harddisk' },
    { id: 'startup-audit', latest: '1.0.0', latestDl: 15, totalDl: 15, category: 'audit' },
    { id: 'temp-hunter', latest: '1.1.0', latestDl: 15, totalDl: 15, category: 'harddisk' },
    { id: 'log-hunter', latest: '1.1.0', latestDl: 14, totalDl: 14, category: 'harddisk' },
    { id: 'port-hunter', latest: '1.0.0', latestDl: 11, totalDl: 11, category: 'audit' },
    { id: 'drive-health', latest: '1.0.0', latestDl: 10, totalDl: 10, category: 'audit' },
    { id: 'browser-cache-hunter', latest: '1.0.0', latestDl: 9, totalDl: 9, category: 'audit' },
    { id: 'schtask-hunter', latest: '1.1.0', latestDl: 1, totalDl: 7, category: 'audit' },
    { id: 'reg-hunter', latest: '1.1.0', latestDl: 3, totalDl: 7, category: 'audit' },
    { id: 'svc-hunter', latest: '1.1.0', latestDl: 3, totalDl: 6, category: 'audit' },
  ],
};

/** R546：按累计下载数降序取前 n（纯函数，可单测；并列时按 latestDl 再排，再并列按 id 保证稳定）。 */
export function leaderboardTop(snapshot: PluginDlSnapshot, n: number): PluginDlStat[] {
  return [...snapshot.items]
    .sort((a, b) => b.totalDl - a.totalDl || b.latestDl - a.latestDl || a.id.localeCompare(b.id))
    .slice(0, Math.max(0, n));
}
