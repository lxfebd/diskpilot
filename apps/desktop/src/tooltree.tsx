// 文件树本地操作工具：删除/回收后在前端树上真实移除节点，并把被删节点的
// 大小/文件数从各级祖先里减掉，让「清理完世界就变」，不必重新扫描。
//
// 注意：这里用「减法」而不是「按子节点重算」——后端扫描对每层 children 做了
// top-K 截断，目录的 size 是完整聚合值而非可见子节点之和；若重算会把被截断
// 的部分凭空丢掉，导致父级大小骤降。纯函数、不可变更新，保持 Zustand 引用
// 比较能触发重渲染。
import { FolderOpen, Copy, Trash2 } from 'lucide-react';
import type { Node } from './types';
import type { ContextMenuItem, ContextMenuState } from './components/ContextMenu';
import { api } from './api';
// 非组件模块：用模块级 t()，且只在打开菜单 / 点项的调用点求值，
// 绝不在模块顶层把文案求成常量（那样中文会烤死在首次 import）。
import { t } from './i18n';

// pruneMany 多路径版：一次整树遍历同时移除 N 个目标，而不是每删一项
// 全树走一遍。批量回收用它把「N 项 × M 棵缓存树」次重建压成「M 棵」次。
// 目标互为祖先/后代时天然安全：祖先被移除后不再下钻，后代的扣减已被祖先
// 的 size 覆盖（目录 size 是完整聚合值，包含所有后代）。
export function pruneMany(root: Node, paths: string[]): Node {
  if (paths.length === 0) return root;
  const norm = (p: string) => p.replace(/[\\/]+$/, '').toLowerCase();
  const targets = new Set(paths.map(norm));
  const walk = (n: Node): Node => {
    let removedSize = 0;
    let removedFiles = 0;
    const kids: Node[] = [];
    for (const c of n.children ?? []) {
      if (targets.has(norm(c.path))) {
        removedSize += c.size || 0;
        removedFiles += c.file_count || 0;
        continue;
      }
      const nc = walk(c);
      removedSize += Math.max(0, (c.size || 0) - (nc.size || 0));
      removedFiles += Math.max(0, (c.file_count || 0) - (nc.file_count || 0));
      kids.push(nc);
    }
    if (removedSize === 0 && removedFiles === 0) return n;
    return {
      ...n,
      children: kids,
      size: Math.max(0, (n.size || 0) - removedSize),
      file_count: Math.max(0, (n.file_count || 0) - removedFiles),
    };
  };
  return walk(root);
}

// 某棵缓存树（key = 归一化后的扫描目标）是否可能包含 path：批量剪枝时只动
// 受影响的树，而不是把每棵缓存盘都重建一遍。
export function treeMayContain(treeKey: string, path: string): boolean {
  const k = treeKey.replace(/[\\/]+$/, '').toUpperCase();
  const p = path.replace(/[\\/]+$/, '').toUpperCase();
  return p === k || p.startsWith(k + '\\');
}

// 文件树 / 文件列表共用的右键菜单：打开位置、复制路径、移入回收站。
// 「移入回收站」只负责挂出确认面板（铁律 6：破坏性动作禁 window.confirm 一步
// 执行），真正执行交给调用方：TreeView 收到 onRequestRecycle 后弹 ConfirmDialog，
// 用户点「确认」才经 store.recyclePaths（执行 + 前端剪枝 + toast 一条链完成），
// 避免同一路径被重复执行两次。
export function openDiskpilotMenu(
  e: React.MouseEvent,
  node: { path: string; name: string; size: number },
  onRequestRecycle: (size: number) => void,
  setCtx: (s: ContextMenuState | null) => void,
) {
  e.preventDefault();
  const items: ContextMenuItem[] = [
    {
      label: t('shell.tooltree.openInExplorer'),
      icon: <FolderOpen size={12} />,
      onClick: () => { api.revealInExplorer(node.path).catch(() => { /* 路径可能已不存在 */ }); },
    },
    {
      label: t('shell.tooltree.copyPath'),
      icon: <Copy size={12} />,
      onClick: () => { navigator.clipboard?.writeText(node.path).catch(() => { /* ignore */ }); },
    },
    {
      label: t('shell.tooltree.recycle'),
      icon: <Trash2 size={12} />,
      danger: true,
      onClick: () => onRequestRecycle(node.size),
    },
  ];
  setCtx({ x: e.clientX, y: e.clientY, items });
}
