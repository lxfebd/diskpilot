import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ChevronRight, ChevronDown, Loader2 } from 'lucide-react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { Node } from '../types';
import { formatBytes, formatCount } from '../format';
import { ContextMenu, type ContextMenuState } from './ContextMenu';
import { ConfirmDialog } from './ConfirmDialog';
import { openDiskpilotMenu } from '../tooltree';
import { useStore } from '../store';
import { api } from '../api';
import { useT } from '../i18n';

type Props = {
  root: Node;
  selectedPath: string | null;
  onSelect: (p: string) => void;
};

const ROW_H = 22; // must match .tree-row height in styles.css

export type FlatRow = {
  node: Node;
  depth: number;
  parentSize: number;
  loading: boolean;
};

/** 目录被深度 cap 截断（children_truncated 有值）且完整子项尚未取回时才显示 +N 徽标。 */
export function isTruncated(node: Node, fullLoaded: boolean): boolean {
  return node.children_truncated != null && !fullLoaded;
}

export function flattenTree(
  root: Node,
  openPaths: Set<string>,
  lazy: ReadonlyMap<string, Node[]>,
  loadingPaths: ReadonlySet<string>,
): FlatRow[] {
  const rows: FlatRow[] = [];
  const walk = (node: Node, depth: number, parentSize: number) => {
    const isLoading = loadingPaths.has(node.path);
    rows.push({ node, depth, parentSize, loading: isLoading });
    if (!node.is_dir || !openPaths.has(node.path) || isLoading) return;
    // 已按需取回的完整子项优先，其次节点自带的 children（可能是截断后的）。
    const kids = lazy.get(node.path) ?? node.children ?? [];
    // 同步展平的上限：虚拟滚动只挂载可见行，几千项没问题；但 flattenTree
    // 是同步递归，单目录数万子项时会阻塞主线程，留一个安全阀。
    for (let i = 0; i < Math.min(kids.length, 5000); i++) {
      walk(kids[i], depth + 1, node.size || 1);
    }
  };
  walk(root, 0, root.size || 1);
  return rows;
}

export function TreeView({ root, selectedPath, onSelect }: Props) {
  const t = useT();
  const [ctx, setCtx] = useState<ContextMenuState | null>(null);
  const [openPaths, setOpenPaths] = useState<Set<string>>(() => new Set([root.path]));
  // 按需加载的完整子项覆盖：path → 完整 children（来自后端内存树）。
  const [lazy, setLazy] = useState<Map<string, Node[]>>(() => new Map());
  const [loadingPaths, setLoadingPaths] = useState<Set<string>>(() => new Set());
  const scrollRef = useRef<HTMLDivElement>(null);
  // 防止已卸载/新 root 后的迟到响应覆盖新树状态。
  const rootPathRef = useRef(root.path);
  rootPathRef.current = root.path;

  // Reset expansion when a brand-new root arrives (new scan).
  useEffect(() => {
    setOpenPaths(new Set([root.path]));
    setLazy(new Map());
    setLoadingPaths(new Set());
  }, [root.path]);

  const rows = useMemo(
    () => flattenTree(root, openPaths, lazy, loadingPaths),
    [root, openPaths, lazy, loadingPaths],
  );

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 14,
  });

  const toggle = useCallback((path: string) => {
    setOpenPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  // 展开被深度 cap 截断的目录时，从后端内存完整树按需取回全部子项
  // （零磁盘 IO）。取回后覆盖该目录的 children 渲染来源；树已加载过
  // 就不再重复请求。
  const ensureFullChildren = useCallback((node: Node) => {
    if (!node.is_dir || node.children_truncated == null) return;
    if (lazy.has(node.path) || loadingPaths.has(node.path)) return;
    const rootAtFetch = rootPathRef.current;
    setLoadingPaths((prev) => new Set(prev).add(node.path));
    void api
      .treeSubtree(node.path)
      .then((full) => {
        // 迟到响应只在 root 未更换时落地（新扫描/切盘会换 root 并清空 lazy）。
        if (!full || rootPathRef.current !== rootAtFetch) return;
        setLazy((prev) => {
          const next = new Map(prev);
          next.set(node.path, full.children ?? []);
          return next;
        });
      })
      .finally(() => {
        setLoadingPaths((prev) => {
          const next = new Set(prev);
          next.delete(node.path);
          return next;
        });
      });
  }, [lazy, loadingPaths]);

  const handleToggle = useCallback((node: Node) => {
    ensureFullChildren(node);
    toggle(node.path);
  }, [ensureFullChildren, toggle]);

  const recycle = useStore((s) => s.recyclePaths);
  // 两步确认：右键「移入回收站」只挂确认面板（openDiskpilotMenu 的
  // onRequestRecycle），用户点「确认」才真正 recycle。
  const [pendingRecycle, setPendingRecycle] = useState<{ path: string; name: string; size: number } | null>(null);

  const openCtx = useCallback((e: React.MouseEvent, node: Node) => {
    openDiskpilotMenu(
      e,
      { path: node.path, name: node.name, size: node.size },
      (size) => setPendingRecycle({ path: node.path, name: node.name, size }),
      setCtx,
    );
  }, [setPendingRecycle]);

  return (
    <div className="treeview">
      <div className="tree-headrow">
        <div className="col-name">{t('overview.tree.colName')}</div>
        <div className="col-pct">{t('overview.tree.colParentPct')}</div>
        <div className="col-size">{t('overview.tree.colSize')}</div>
        <div className="col-count">{t('overview.tree.colCount')}</div>
      </div>
      <div className="tree-body" ref={scrollRef}>
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative', width: '100%' }}>
          {virtualizer.getVirtualItems().map((vi) => {
            const row = rows[vi.index];
            return (
              <div
                key={row.node.path}
                style={{
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  width: '100%',
                  height: ROW_H,
                  transform: `translateY(${vi.start}px)`,
                }}
              >
                <Row
                  node={row.node}
                  parentSize={row.parentSize}
                  depth={row.depth}
                  selectedPath={selectedPath}
                  onSelect={onSelect}
                  onCtx={openCtx}
                  open={openPaths.has(row.node.path)}
                  loading={row.loading}
                  fullLoaded={lazy.has(row.node.path)}
                  onToggle={handleToggle}
                />
              </div>
            );
          })}
        </div>
      </div>
      <ContextMenu state={ctx} onClose={() => setCtx(null)} />

      {pendingRecycle && (
        <ConfirmDialog
          title={t('shell.tooltree.recycle')}
          body={t('shell.tooltree.recycleConfirm', {
            name: pendingRecycle.name || pendingRecycle.path,
            size: formatBytes(pendingRecycle.size),
          })}
          confirmLabel={t('shell.tooltree.recycle')}
          onConfirm={() => {
            // '手动回收' 是写进回收记录/undo 日志的后端参数值，不随界面语言变。
            void recycle([{ path: pendingRecycle.path, size_hint: pendingRecycle.size }], '手动回收'); // @i18n-keep 后端 reason 参数值
            setPendingRecycle(null);
          }}
          onCancel={() => setPendingRecycle(null)}
        />
      )}
    </div>
  );
}

function FolderGlyph({ open }: { open: boolean }) {
  // Windows-style yellow folder (closed/open variants).
  if (open) {
    return (
      <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden>
        <path d="M1.5 4.5 A1 1 0 0 1 2.5 3.5 H6 L7.5 5 H13.5 A1 1 0 0 1 14.5 6 V6.8 H3.6 L1.5 12.5 Z" fill="#f5c75e" stroke="#9c7c2a" strokeWidth="0.7" />
        <path d="M3.6 6.8 H15.2 L13.2 12.5 H1.5 Z" fill="#ffd97a" stroke="#9c7c2a" strokeWidth="0.7" strokeLinejoin="round" />
      </svg>
    );
  }
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden>
      <path d="M1.5 4.5 A1 1 0 0 1 2.5 3.5 H6 L7.5 5 H13.5 A1 1 0 0 1 14.5 6 V12.5 A1 1 0 0 1 13.5 13.5 H2.5 A1 1 0 0 1 1.5 12.5 Z" fill="#f5c75e" stroke="#9c7c2a" strokeWidth="0.8" strokeLinejoin="round" />
      <path d="M1.5 6 H14.5" stroke="#9c7c2a" strokeWidth="0.5" opacity="0.5" />
    </svg>
  );
}

function FileGlyph({ ext }: { ext: string }) {
  // Pick a tint by file family — keeps the tree visually grep-able like Explorer.
  const fill =
    /^(exe|msi|cmd|bat|com)$/i.test(ext) ? '#cfe6ff' :
    /^(dll|sys|drv|ocx)$/i.test(ext) ? '#dfd6f7' :
    /^(zip|rar|7z|tar|gz|xz)$/i.test(ext) ? '#ffd6c0' :
    /^(png|jpg|jpeg|gif|bmp|webp|svg|ico)$/i.test(ext) ? '#ffd0e6' :
    /^(mp3|wav|flac|m4a|ogg)$/i.test(ext) ? '#d0f0d8' :
    /^(mp4|mov|mkv|avi|webm)$/i.test(ext) ? '#c8eaef' :
    /^(txt|md|log)$/i.test(ext) ? '#fff1bd' :
    /^(json|toml|yaml|yml|xml|ini|conf)$/i.test(ext) ? '#e6f0ff' :
    /^(pdf)$/i.test(ext) ? '#ffc5c5' :
    '#ffffff';
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden>
      <path d="M3.5 2 H10 L13 5 V13.5 A0.5 0.5 0 0 1 12.5 14 H3.5 A0.5 0.5 0 0 1 3 13.5 V2.5 A0.5 0.5 0 0 1 3.5 2 Z"
        fill={fill} stroke="#5b4d57" strokeWidth="0.7" strokeLinejoin="round" />
      <path d="M10 2 V5 H13" fill="none" stroke="#5b4d57" strokeWidth="0.7" strokeLinejoin="round" />
    </svg>
  );
}

function extOf(name: string): string {
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(i + 1).toLowerCase() : '';
}

const Row = memo(function Row({
  node,
  parentSize,
  depth,
  selectedPath,
  onSelect,
  onCtx,
  open,
  loading,
  fullLoaded,
  onToggle,
}: {
  node: Node;
  parentSize: number;
  depth: number;
  selectedPath: string | null;
  onSelect: (p: string) => void;
  onCtx: (e: React.MouseEvent, node: Node) => void;
  open: boolean;
  loading: boolean;
  fullLoaded: boolean;
  onToggle: (node: Node) => void;
}) {
  const t = useT();
  const hasKids = (node.children?.length ?? 0) > 0;
  const sel = node.path === selectedPath;
  const pct = parentSize > 0 ? (node.size / parentSize) * 100 : 0;
  // 被深度 cap 截断且完整子项尚未取回（fullLoaded 后徽标消失）。
  const truncated = isTruncated(node, fullLoaded);

  return (
    <div
      className={'tree-row' + (sel ? ' selected' : '') + (node.is_dir ? '' : ' is-file')}
      onClick={() => onSelect(node.path)}
      onContextMenu={(e) => onCtx(e, node)}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData('application/x-diskpilot-path', node.path);
        e.dataTransfer.setData('application/x-diskpilot-name', node.name);
        e.dataTransfer.effectAllowed = 'copy';
      }}
      title={t('overview.rightClickHint', { path: node.path })}
    >
      <div className="col-name" style={{ paddingLeft: 4 + depth * 14 }}>
        <span
          className="caret"
          onClick={(e) => { e.stopPropagation(); if (hasKids) onToggle(node); }}
        >
          {loading
            ? <Loader2 size={11} className="spin" />
            : hasKids
              ? (open ? <ChevronDown size={11} /> : <ChevronRight size={11} />)
              : <span className="caret-stub" />}
        </span>
        <span className="glyph">
          {node.is_dir ? <FolderGlyph open={open} /> : <FileGlyph ext={extOf(node.name)} />}
        </span>
        <span className="name">{node.name || node.path}</span>
        {truncated && (
          <span className="badge badge-more" title={t('overview.tree.truncatedTip', { n: node.children_truncated ?? 0 })}>
            {loading ? t('overview.tree.loading') : `+${(node.children_truncated ?? 0) - (node.children?.length ?? 0)}`}
          </span>
        )}
        {node.scaffold_id && <span className="badge">{node.scaffold_id}</span>}
      </div>
      <div className="col-pct">
        <span className="pct-bar"><span style={{ width: `${Math.min(100, pct)}%` }} /></span>
        <span className="pct-num">{pct.toFixed(1)}%</span>
      </div>
      <div className="col-size">{formatBytes(node.size)}</div>
      <div className="col-count">{formatCount(node.file_count)}</div>
    </div>
  );
});
