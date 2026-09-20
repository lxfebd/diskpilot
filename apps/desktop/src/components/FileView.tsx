import { memo, useMemo, useRef, useState } from 'react';
import { Search, ArrowDown, ArrowUp, Folder, Copy, Trash2 } from 'lucide-react';
import type { Node } from '../types';
import { formatBytes } from '../format';
import { ContextMenu, type ContextMenuState } from './ContextMenu';
import { ConfirmDialog } from './ConfirmDialog';
import { api } from '../api';
import { useStore } from '../store';
import { useT } from '../i18n';

type Props = {
  /** 从 root 出发选中的目录（不含该目录自身，只收集其子树内的文件节点） */
  root: Node;
  selectedPath: string | null;
  onSelect: (p: string) => void;
};

type SortKey = 'size' | 'name' | 'count';

interface FlatFile {
  path: string;
  name: string;
  size: number;
  rel: string; // 相对当前聚焦目录的显示路径
}

const EXT_PALETTE = [
  '#ff6d8f', '#ff9d5c', '#7ee2a8', '#6db5ff', '#c9a0ff', '#ffd166', '#8fd3ff', '#f0a0c0',
];

/** 无扩展名文件的分组哨兵值：语言无关（它同时当 Map key 与过滤条件用），
 *  渲染时经 extLabel() 换成本地文案，避免切语言后 key 对不上。 */
const NO_EXT = '(none)';

function extOf(name: string): string {
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(i + 1).toLowerCase() : NO_EXT;
}

function fillForExt(ext: string): string {
  let h = 0;
  for (let i = 0; i < ext.length; i++) h = (h * 31 + ext.charCodeAt(i)) & 0xffffffff;
  return EXT_PALETTE[Math.abs(h) % EXT_PALETTE.length];
}

function matchesQuery(name: string, q: string): boolean {
  if (!q) return true;
  const lower = q.toLowerCase();
  // 支持 * 与 ? 通配符，如 *.mp4  /  instal?er  /  node*
  if (!lower.includes('*') && !lower.includes('?')) return name.toLowerCase().includes(lower);
  const re = new RegExp('^' + lower.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.') + '$', 'i');
  return re.test(name);
}

export function FileView({ root, selectedPath, onSelect }: Props) {
  const t = useT();
  const [ctx, setCtx] = useState<ContextMenuState | null>(null);
  // 两步确认：右键「移入回收站」只挂出确认面板，真正执行在 doRecycle。
  const [pendingRecycle, setPendingRecycle] = useState<FlatFile | null>(null);
  // 搜索输入防抖：query 是「即时回显」的输入值，filteredQuery 是「真正参与
  // 过滤/排序」的已防抖值。大目录（几万文件）每敲一个键全量过滤排序会卡，
  // 延迟到停止输入后一次算完；交互上搜索框即时显示输入，结果列表滞后一拍。
  const [query, setQuery] = useState('');
  const [filteredQuery, setFilteredQuery] = useState('');
  const queryTimer = useRef<number | null>(null);
  const onQueryChange = (v: string) => {
    setQuery(v);
    if (queryTimer.current !== null) window.clearTimeout(queryTimer.current);
    queryTimer.current = window.setTimeout(() => setFilteredQuery(v), 150);
  };
  const [sortKey, setSortKey] = useState<SortKey>('size');
  const [sortDesc, setSortDesc] = useState(true);
  const [topN, setTopN] = useState(200);
  const [extFilter, setExtFilter] = useState<string | null>(null);
  const recycle = useStore((s) => s.recyclePaths);

  const focusNode = useMemo(() => {
    if (!selectedPath) return root;
    const parts = selectedPath.split(/[\\/]/).filter(Boolean);
    let cur: Node | null = root;
    for (const p of parts) {
      if (!cur) break;
      cur = cur.children?.find((c) => c.path === selectedPath || c.name === p) ?? null;
      if (cur && cur.path === selectedPath) return cur;
    }
    return cur ?? root;
  }, [root, selectedPath]);

  const files = useMemo(() => {
    const out: FlatFile[] = [];
    // 预览模式限制收集数量，避免几十万文件把 UI 卡死；truncated 提示「文件过多被截断」
    const MAX = 60_000;
    const walkLimited = (n: Node, base: string) => {
      for (const c of n.children) {
        if (out.length >= MAX) return;
        if (c.is_dir) walkLimited(c, base ? `${base}/${c.name}` : c.name);
        else out.push({ path: c.path, name: c.name, size: c.size ?? 0, rel: base ? `${base}/${c.name}` : c.name });
      }
    };
    walkLimited(focusNode, '');
    return { list: out, truncated: out.length >= MAX };
  }, [focusNode]);

  const filtered = useMemo(() => {
    let list = files.list.filter((f) => matchesQuery(f.name, filteredQuery));
    if (extFilter) list = list.filter((f) => extOf(f.name) === extFilter);
    if (sortKey === 'size') list.sort((a, b) => (sortDesc ? b.size - a.size : a.size - b.size));
    else if (sortKey === 'name') list.sort((a, b) => (sortDesc ? b.name.localeCompare(a.name) : a.name.localeCompare(b.name)));
    else list.sort((a, b) => (sortDesc ? b.path.length - a.path.length : a.path.length - b.path.length));
    return list;
  }, [files, filteredQuery, extFilter, sortKey, sortDesc]);

  const top = filtered.slice(0, topN);

  const extStats = useMemo(() => {
    const m = new Map<string, { bytes: number; count: number }>();
    for (const f of files.list) {
      const e = extOf(f.name);
      const cur = m.get(e) ?? { bytes: 0, count: 0 };
      cur.bytes += f.size;
      cur.count += 1;
      m.set(e, cur);
    }
    return [...m.entries()].sort((a, b) => b[1].bytes - a[1].bytes).slice(0, 12);
  }, [files.list]);

  const totalBytes = useMemo(() => files.list.reduce((s, f) => s + f.size, 0), [files.list]);

  const toggleSort = (key: SortKey) => {
    if (sortKey === key) setSortDesc((v) => !v);
    else { setSortKey(key); setSortDesc(key === 'size'); }
  };

  // 分组哨兵值（NO_EXT）只在内部当 key 用，对外显示一律走这里换文案。
  const extLabel = (e: string) => (e === NO_EXT ? t('overview.fileview.extNone') : e);

  return (
    <div className="fileview">
      <div className="fileview-search">
        <Search size={13} />
        <input
          placeholder={t('overview.fileview.search')}
          value={query}
          onChange={(e) => onQueryChange(e.target.value)}
        />
      </div>

      <div className="fileview-stats">
        <span>{t('overview.fileview.count', { count: files.list.length.toLocaleString() })}{files.truncated ? t('overview.fileview.truncated') : ''}</span>
        <span>·</span>
        <span>{formatBytes(totalBytes)}</span>
        <span>·</span>
        <select value={topN} onChange={(e) => setTopN(Number(e.target.value))} title={t('overview.fileview.showTitle')}>
          <option value={100}>{t('overview.fileview.showN', { n: 100 })}</option>
          <option value={200}>{t('overview.fileview.showN', { n: 200 })}</option>
          <option value={500}>{t('overview.fileview.showN', { n: 500 })}</option>
          <option value={1000}>{t('overview.fileview.showN', { n: 1000 })}</option>
        </select>
      </div>

      <div className="fileview-head">
        <button className={'sortable' + (sortKey === 'name' ? ' active' : '')} onClick={() => toggleSort('name')}>
          {t('overview.fileview.colName')} {sortKey === 'name' && (sortDesc ? <ArrowDown size={11} /> : <ArrowUp size={11} />)}
        </button>
        <button className={'sortable' + (sortKey === 'size' ? ' active' : '')} onClick={() => toggleSort('size')}>
          {t('overview.fileview.colSize')} {sortKey === 'size' && (sortDesc ? <ArrowDown size={11} /> : <ArrowUp size={11} />)}
        </button>
      </div>

      <div className="fileview-body">
        {top.length === 0 && <div className="fileview-empty">{t('overview.fileview.empty')}</div>}
        {top.map((f) => (
          <FileRow
            key={f.path}
            f={f}
            selected={f.path === selectedPath}
            onSelect={onSelect}
            onCtx={(x, y) => {
              setCtx({
                x,
                y,
                items: [
                  {
                    label: t('overview.fileview.menuReveal'),
                    icon: <Folder size={12} />,
                    onClick: () => { api.revealInExplorer(f.path).catch(() => { /* 路径可能已不存在 */ }); },
                  },
                  {
                    label: t('overview.fileview.menuCopy'),
                    icon: <Copy size={12} />,
                    onClick: () => { navigator.clipboard?.writeText(f.path).catch(() => { /* ignore */ }); },
                  },
                  {
                    label: t('overview.fileview.menuRecycle'),
                    icon: <Trash2 size={12} />,
                    danger: true,
                    onClick: () => setPendingRecycle(f),
                  },
                ],
              });
            }}
          />
        ))}
      </div>
      <ContextMenu state={ctx} onClose={() => setCtx(null)} />

      {pendingRecycle && (
        <ConfirmDialog
          title={t('overview.fileview.menuRecycle')}
          body={t('overview.fileview.recycleConfirm', { name: pendingRecycle.name, size: formatBytes(pendingRecycle.size) })}
          confirmLabel={t('overview.fileview.menuRecycle')}
          onConfirm={() => {
            // '手动回收' 是写进回收记录/undo 日志的后端参数值，不随界面语言变。
            void recycle([{ path: pendingRecycle.path, size_hint: pendingRecycle.size }], '手动回收'); // @i18n-keep 后端 reason 参数值
            setPendingRecycle(null);
          }}
          onCancel={() => setPendingRecycle(null)}
        />
      )}

      {extStats.length > 0 && (
        <div className="fileview-exts">
          <div className="fv-exts-title">{t('overview.fileview.extTitle', { n: extStats.length })}</div>
          {extStats.map(([ext, s]) => {
            const pct = totalBytes > 0 ? (s.bytes / totalBytes) * 100 : 0;
            return (
              <div
                key={ext}
                className={'fv-ext-row' + (extFilter === ext ? ' active' : '')}
                onClick={() => setExtFilter(extFilter === ext ? null : ext)}
                title={t('overview.fileview.clickFilter', { ext: extLabel(ext) })}
              >
                <span className="fv-ext-name" style={{ color: fillForExt(ext) }}>{extLabel(ext)}</span>
                <span className="fv-ext-bar"><span style={{ width: `${Math.min(100, pct)}%`, background: fillForExt(ext) }} /></span>
                <span className="fv-ext-val">{pct.toFixed(1)}% · {s.count.toLocaleString()}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
// 文件行：memo 化后，搜索/排序/选中态变化只重渲变化行，不整表重渲。
// props 均为基本类型 / 稳定引用（onSelect 来自 App 的 useStore action，
// onCtx 闭包每次变化会重建——行渲染本身轻，主要是避开大列表整表重渲）。

const FileRow = memo(function FileRow({
  f,
  selected,
  onSelect,
  onCtx,
}: {
  f: FlatFile;
  selected: boolean;
  onSelect: (p: string) => void;
  onCtx: (x: number, y: number) => void;
}) {
  const t = useT();
  return (
    <div
      className={'fileview-row' + (selected ? ' selected' : '')}
      onClick={() => onSelect(f.path)}
      onContextMenu={(e) => {
        e.preventDefault();
        onCtx(e.clientX, e.clientY);
      }}
      title={t('overview.rightClickHint', { path: f.path })}
    >
      <span className="fv-name" style={{ paddingLeft: 6 }}>
        <span className="fv-dot" style={{ background: fillForExt(extOf(f.name)) }} />
        {f.name}
      </span>
      <span className="fv-rel">{f.rel !== f.name ? f.rel.slice(0, Math.max(0, f.rel.length - f.name.length - 1)) : ''}</span>
      <span className="fv-size">{formatBytes(f.size)}</span>
    </div>
  );
});
