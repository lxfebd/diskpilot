import { memo, useMemo } from 'react';
import { hierarchy, treemap, treemapSquarify } from 'd3-hierarchy';
import type { Node } from '../types';
import { formatBytes } from '../format';
import { useT } from '../i18n';
import { FAMILY_COLORS, FILE_FALLBACK, DIR_COLORS, JUNK_DIR_RE, JUNK_COLOR } from '../colors';

type Props = {
  node: Node;
  width: number;
  height: number;
  onSelect: (path: string, node?: Node) => void;
  selectedPath: string | null;
};

// 文件类型 → 色相。颜色语义对齐 WizTree/WinDirStat：视频/图片/文档/压缩包一眼可辨，
// 而不是按文件夹名随机哈希 —— 这样用户能瞬间看出「空间杀手」是什么类型的文件。
// R108：本文件曾与 colors.ts 整套同值重复（FAMILY_COLORS / DIR_COLORS /
// JUNK_DIR_RE / 两个 fallback），正是 W4 色板收敛要消灭的漂移源——改回单源 import。
const COLOR_BY_FAMILY = FAMILY_COLORS;

function colorForFile(name: string): string {
  for (const [re, color] of COLOR_BY_FAMILY) {
    if (re.test(name)) return color;
  }
  return FILE_FALLBACK; // 其他文件 中性灰
}

// 文件夹色：按名称哈希从暖色系选，保持目录块之间可区分
function colorForDir(name: string): string {
  if (JUNK_DIR_RE.test(name)) return JUNK_COLOR;
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) & 0xffffffff;
  return DIR_COLORS[Math.abs(h) % DIR_COLORS.length];
}

// R312：空间图不是全量视图——根只布局前 300 个子项，后端还会把过深目录折叠
// （children_truncated>0 表示这层 children 不完整）。两处都会让用户以为「图里没有 =
// 不存在」，所以在图上落一行说明，把「去看树视图」的出口点出来。
export function treeIsTruncated(node: Node | null | undefined, maxChildren = 300): boolean {
  if (!node) return false;
  if ((node.children?.length ?? 0) > maxChildren) return true;
  const stack: (Node | undefined)[] = [node];
  while (stack.length > 0) {
    const cur = stack.pop();
    if (!cur) continue;
    if ((cur.children_truncated ?? 0) > 0) return true;
    if (cur.children) for (const c of cur.children) stack.push(c);
  }
  return false;
}

export const Treemap = memo(function Treemap({ node, width, height, onSelect, selectedPath }: Props) {
  const t = useT();
  // layout 只依赖树数据与像素尺寸：选中态（selectedPath）变化不该重算
  // d3 布局（几百个矩形位置不变，只是描边换色）。选中态在渲染期读取，
  // 不放进 deps，点击节点时只重渲颜色层，省掉整棵 d3 squarify。
  const layout = useMemo(() => {
    // 一个盘可能几百个子目录，全部渲染既慢又碎 —— 取前 300 大项即可覆盖 99% 空间
    const root = hierarchy<Node>({ ...node, children: (node.children ?? []).slice(0, 300) } as Node, (d) =>
      d.children?.filter((c) => c.size > 0),
    )
      .sum((d) => (d.children && d.children.length ? 0 : Math.max(1, d.size)))
      .sort((a, b) => (b.value ?? 0) - (a.value ?? 0));
    const laidOut = treemap<Node>()
      .size([width, height])
      .tile(treemapSquarify)
      .paddingOuter(2)
      .paddingInner(1)(root);
    // 渲染两层：根的直接子层（depth 1）+ 大子项的孙层（depth 2），块大小正好可读
    return laidOut.descendants().filter((n) => n.depth >= 1 && n.depth <= 2);
  }, [node, width, height]);

  return (
    <svg width={width} height={height} className="treemap">
      {layout.map((d, i) => {
        const x = d.x0;
        const y = d.y0;
        const w = d.x1 - d.x0;
        const h = d.y1 - d.y0;
        if (w < 3 || h < 3) return null; // 碎块不画，省渲染
        const isSelected = d.data.path === selectedPath;
        const isDir = d.data.is_dir;
        const fill = isDir ? colorForDir(d.data.name) : colorForFile(d.data.name);
        const parentVal = d.parent?.value || 1;
        const share = ((d.data.size ?? 0) / parentVal) * 100;
        return (
          <g
            key={i}
            onClick={() => onSelect(d.data.path, d.data)}
            style={{ cursor: 'pointer' }}
          >
            <title>{t('overview.treemap.tip', { path: d.data.path, size: formatBytes(d.data.size), share: share.toFixed(1) })}</title>
            <rect
              x={x}
              y={y}
              width={w}
              height={h}
              fill={fill}
              fillOpacity={isDir ? 0.72 : 0.85}
              stroke={isSelected ? '#ffffff' : 'rgba(28,18,26,0.55)'}
              strokeWidth={isSelected ? 2 : 1}
            />
            {w > 70 && h > 22 && (
              <text x={x + 6} y={y + 16} fill="rgba(22,12,18,0.95)" fontSize={11} fontWeight={700} style={{ pointerEvents: 'none' }}>
                {d.data.name.length > 28 ? d.data.name.slice(0, 27) + '…' : d.data.name}
              </text>
            )}
            {w > 70 && h > 36 && (
              <text x={x + 6} y={y + 30} fill="rgba(22,12,18,0.75)" fontSize={10} style={{ pointerEvents: 'none' }}>
                {formatBytes(d.data.size)}
              </text>
            )}
          </g>
        );
      })}
      {treeIsTruncated(node) && (
        <text
          x={8}
          y={height - 6}
          fill="rgba(22,12,18,0.75)"
          fontSize={10}
          style={{ pointerEvents: 'none' }}
        >
          {t('overview.treemap.folded')}
        </text>
      )}
    </svg>
  );
});