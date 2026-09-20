import { useMemo, useState } from 'react';
import { ListTree, LayoutGrid, Files } from 'lucide-react';
import type { Node } from '../types';
import { TreeView } from './TreeView';
import { Treemap } from './Treemap';
import { FileView } from './FileView';
import { useT } from '../i18n';

type View = 'tree' | 'treemap' | 'files';

type Props = {
  root: Node;
  selectedPath: string | null;
  onSelect: (p: string) => void;
};

// 视图页签表：写成函数而非常量，标签在渲染处现取——模块顶层求值会把中文
// 烤死在首次 import，切语言就再也不生效。
const viewTabs = (t: (key: string) => string) => [
  { id: 'tree' as View, label: t('shell.leftpanel.viewTree'), icon: <ListTree size={13} /> },
  { id: 'treemap' as View, label: t('shell.leftpanel.viewTreemap'), icon: <LayoutGrid size={13} /> },
  { id: 'files' as View, label: t('shell.leftpanel.viewFiles'), icon: <Files size={13} /> },
];

export function LeftPanel({ root, selectedPath, onSelect }: Props) {
  const t = useT();
  const [view, setView] = useState<View>(() => {
    const saved = localStorage.getItem('diskpilot.leftView');
    return saved === 'treemap' || saved === 'files' ? saved : 'tree';
  });

  const persist = (v: View) => {
    setView(v);
    try { localStorage.setItem('diskpilot.leftView', v); } catch { /* ignore */ }
  };

  // 树状图需要实际像素尺寸：用 ref 在挂载后量取容器尺寸，避免用 0 尺寸渲染
  const [mapSize, setMapSize] = useState<{ w: number; h: number } | null>(null);

  const treemapEl = useMemo(
    () => (
      <div
        ref={(el) => {
          if (el && !mapSize) {
            const r = el.getBoundingClientRect();
            if (r.width > 0 && r.height > 0) setMapSize({ w: r.width, h: r.height });
          }
        }}
        style={{ width: '100%', height: '100%', minHeight: 200 }}
      >
        {mapSize ? (
          <Treemap node={root} width={mapSize.w} height={mapSize.h} onSelect={onSelect} selectedPath={selectedPath} />
        ) : null}
      </div>
    ),
    [root, mapSize, selectedPath, onSelect],
  );

  return (
    <div className="leftpanel">
      <div className="leftpanel-tabs">
        {viewTabs(t).map((v) => (
          <button
            key={v.id}
            className={'lefttab' + (view === v.id ? ' active' : '')}
            onClick={() => persist(v.id)}
          >
            {v.icon} {v.label}
          </button>
        ))}
      </div>
      <div className="leftpanel-body">
        {view === 'tree' && <TreeView root={root} selectedPath={selectedPath} onSelect={onSelect} />}
        {view === 'treemap' && treemapEl}
        {view === 'files' && <FileView root={root} selectedPath={selectedPath} onSelect={onSelect} />}
      </div>
    </div>
  );
}