import { memo, useMemo } from 'react';
import { hierarchy, treemap, treemapSquarify } from 'd3-hierarchy';
import type { Node } from '../types';
import { formatBytes } from '../format';
import { useT } from '../i18n';

type Props = {
  node: Node;
  width: number;
  height: number;
  onSelect: (path: string) => void;
  selectedPath: string | null;
};

// 文件类型 → 色相。颜色语义对齐 WizTree/WinDirStat：视频/图片/文档/压缩包一眼可辨，
// 而不是按文件夹名随机哈希 —— 这样用户能瞬间看出「空间杀手」是什么类型的文件。
const COLOR_BY_FAMILY: Array<[RegExp, string]> = [
  [/^(mp4|mov|mkv|avi|webm|flv|wmv|ts|m4v|rmvb)$/i, '#ff6d8f'],   // 视频 粉红
  [/^(png|jpg|jpeg|gif|bmp|webp|svg|ico|heic|raw|cr2|nef|tif|tiff)$/i, '#ff9d5c'], // 图片 橙
  [/^(mp3|wav|flac|m4a|ogg|aac|wma|ape|opus)$/i, '#7ee2a8'],       // 音频 绿
  [/^(zip|rar|7z|tar|gz|xz|bz2|iso|dmg|cab|pkg)$/i, '#c9a0ff'],    // 压缩/镜像 紫
  [/^(doc|docx|pdf|ppt|pptx|xls|xlsx|txt|md|odt|rtf)$/i, '#6db5ff'],// 文档 蓝
  [/^(exe|msi|dll|sys|bat|cmd|com|apk|deb|rpm)$/i, '#8fd3ff'],     // 程序 浅蓝
  [/^(json|toml|yaml|yml|xml|ini|conf|log|sql|db|sqlite|py|js|ts|rs|go|java|c|cpp|h|hpp)$/i, '#ffd166'], // 代码/配置 黄
  [/^(node_modules|__pycache__|\.git|\.cache|\.tmp|\.temp)$/i, '#a8998c'], // 工程垃圾 灰
];

function colorForFile(name: string): string {
  for (const [re, color] of COLOR_BY_FAMILY) {
    if (re.test(name)) return color;
  }
  return '#c9c5c0'; // 其他文件 中性灰
}

// 文件夹色：按名称哈希从暖色系选，保持目录块之间可区分
const DIR_COLORS = ['#ffb3a7', '#ffd1a1', '#ffe8a3', '#b3e5c8', '#a8d8ff', '#d3bfff', '#f8c8e0', '#d9d4c8'];
// 工程垃圾目录（node_modules/.git/.cache 等）一律用灰，即便按目录渲染也不走暖色哈希
const JUNK_DIR_RE = /^(node_modules|__pycache__|\.git|\.cache|\.tmp|\.temp)$/i;
function colorForDir(name: string): string {
  if (JUNK_DIR_RE.test(name)) return '#a8998c';
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) & 0xffffffff;
  return DIR_COLORS[Math.abs(h) % DIR_COLORS.length];
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
            onClick={() => onSelect(d.data.path)}
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
    </svg>
  );
});