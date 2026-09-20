import { invoke } from '@tauri-apps/api/core';
import type { Node } from '../types';
import { isTauri } from '../env';
import * as mocks from '../mocks';

/**
 * 扫描域：扫描 / 取消 / 按需取子树 / 磁盘信息。
 * 由 api.ts 聚合进 `api` 对象，保持 `api.scan()` 等调用面不变。
 */
export const scanApi = {
  /**
   * 扫描一个路径（通常是盘根）。优先走 USN 增量（`scan_path_usn`）：后端在
   * 有缓存树 + 有效 journal 游标时只回放 Change Journal 合并进缓存树（亚秒级），
   * 增量不可用（非 NTFS / journal 未启用 / 已回绕 / 无缓存）时后端自动回退全量，
   * 前端无需感知。非 Tauri 环境直接走 mock。
   */
  scan: async (path: string, keepFilesPerDir?: number): Promise<Node> => {
    const args = { path, keepFilesPerDir: keepFilesPerDir ?? null };
    if (!isTauri) return mocks.scan(path);
    try {
      // 增量命令注册在 Windows 上；非 Windows 后端没有这个命令会报「命令未找到」，
      // 属预期，回退全量 scan_path。
      return await invoke<Node>('scan_path_usn', args);
    } catch {
      return invoke<Node>('scan_path', args);
    }
  },

  cancelScan: () =>
    isTauri ? invoke<void>('cancel_scan') : Promise.resolve(),

  /**
   * 按需加载子树：从前端可见的截断树里展开某目录时，从后端内存完整树
   * （scan_tree）按路径取回该目录的完整未截断子树。纯内存查找零磁盘 IO。
   * 路径不在扫描范围（如已被回收剪除）时返回 null。
   */
  treeSubtree: (path: string): Promise<Node | null> =>
    isTauri ? invoke<Node | null>('tree_subtree', { path }) : Promise.resolve(null),

  volumeInfo: (path: string) =>
    isTauri
      ? invoke<{ total_bytes: number; used_bytes: number; free_bytes: number }>('volume_info', { path })
      : Promise.resolve(null),

  listDrives: () =>
    isTauri
      ? invoke<{ total_bytes: number; used_bytes: number; free_bytes: number; path: string }[]>('list_drives')
      : Promise.resolve([]),

  estimateSize: (path: string) =>
    isTauri ? invoke<number>('estimate_size', { path }) : Promise.resolve(0),
};
