// ── 单一色值源（W4 色板收敛）──────────────────────────────────────────
// 此前 5 套散 palette（CAT_COLORS / EXT_PALETTE / COLOR_BY_FAMILY /
// CATEGORY_COLOR / DIR_COLORS）+ 3 处风险色各自硬编码同一组色值，
// 换主题/改品牌色要改 5 个地方还容易漏。现在收敛到这一处导出，
// 组件只从这里取色；语义 token（--risk-*）依然是第一优先（主题可换）。
//
// 风险/语义色与 theme.ts 的 --risk-safe/--risk-caution/--risk-danger
// 对应（neutral 是 L2 专用「高危」区分度不足时的补充）。

import { common } from './i18n/namespaces/common';

// 风险等级 → 颜色（clash 于 --risk-* token；JS 内联样式用，CSS 用 token）
export const RISK_COLORS: Record<string, string> = {
  low: '#5fcf95',
  medium: '#ffb37a',
  high: '#ff5d7a',
};
// 权限级别 L0-L3 → 颜色（权限中心 / 详情卡）
export const LEVEL_COLORS: Record<string, string> = {
  L0: '#5fcf95',
  L1: '#ffb37a',
  L2: '#ff9f5e',
  L3: '#ff5d7a',
};

// 工具分类 → 主题色（工具墙图标/卡片的「一眼可辨」色）
// 键是后端 toolbelt manifest / 目录扫描给出的中文分类名（数据，用于匹配，不随 UI
// 语言变），故恒取中文表原文；分类名的英文显示由消费方在渲染处按 common.category.*
// 键求值。
const zhCategory = (slug: keyof typeof common.zh) => common.zh[slug];
export const CATEGORY_COLORS: Record<string, string> = {
  [zhCategory('common.category.cpu')]: '#f59e0b',
  [zhCategory('common.category.gpu')]: '#8b5cf6',
  [zhCategory('common.category.disk')]: '#0ea5e9',
  [zhCategory('common.category.suite')]: '#10b981',
  [zhCategory('common.category.other')]: '#64748b',
  [zhCategory('common.category.stress')]: '#ef4444',
  [zhCategory('common.category.memory')]: '#ec4899',
  [zhCategory('common.category.daily')]: '#facc15',
  [zhCategory('common.category.peripheral')]: '#14b8a6',
  [zhCategory('common.category.board')]: '#3b82f6',
  [zhCategory('common.category.game')]: '#22c55e',
};
export const CATEGORY_FALLBACK = '#64748b';
export function categoryColor(name: string): string {
  return CATEGORY_COLORS[name] ?? CATEGORY_FALLBACK;
}

// 首页磁盘分类占用卡片（AssetOverview）的柱状色板
export const CAT_COLORS = ['#8a93a6', '#5b8def', '#3fbf7f', '#9b6de0', '#f2a33c', '#e05252', '#c9d0da', '#f0a0c0'];

// 文件类型 → 色相（FileView 文件行 / TreeView 文件字形）
export const EXT_PALETTE = [
  '#6db5ff', '#7ee2a8', '#f2a33c', '#c9a0ff', '#ff6d8f',
  '#ff9d5c', '#ffd166', '#8fd3ff', '#a8998c', '#c9c5c0',
];
export function extColor(ext: string): string {
  let h = 0;
  for (let i = 0; i < ext.length; i++) h = (h * 31 + ext.charCodeAt(i)) | 0;
  return EXT_PALETTE[Math.abs(h) % EXT_PALETTE.length];
}

// Treemap 文件族色 / 目录暖色板 / 工程垃圾灰
export const FAMILY_COLORS: Array<[RegExp, string]> = [
  [/^(mp4|mov|mkv|avi|webm|flv|wmv|ts|m4v|rmvb)$/i, '#ff6d8f'],
  [/^(png|jpg|jpeg|gif|bmp|webp|svg|ico|heic|raw|cr2|nef|tif|tiff)$/i, '#ff9d5c'],
  [/^(mp3|wav|flac|m4a|ogg|aac|wma|ape|opus)$/i, '#7ee2a8'],
  [/^(zip|rar|7z|tar|gz|xz|bz2|iso|dmg|cab|pkg)$/i, '#c9a0ff'],
  [/^(doc|docx|pdf|ppt|pptx|xls|xlsx|txt|md|odt|rtf)$/i, '#6db5ff'],
  [/^(exe|msi|dll|sys|bat|cmd|com|apk|deb|rpm)$/i, '#8fd3ff'],
  [/^(json|toml|yaml|yml|xml|ini|conf|log|sql|db|sqlite|py|js|ts|rs|go|java|c|cpp|h|hpp)$/i, '#ffd166'],
  [/^(node_modules|__pycache__|\.git|\.cache|\.tmp|\.temp)$/i, '#a8998c'],
];
export const FILE_FALLBACK = '#c9c5c0';
export const JUNK_DIR_RE = /^(node_modules|__pycache__|\.git|\.cache|\.tmp|\.temp)$/i;
export const JUNK_COLOR = '#a8998c';
export const DIR_COLORS = ['#ffb3a7', '#ffd1a1', '#ffe8a3', '#b3e5c8', '#a8d8ff', '#d3bfff', '#f8c8e0', '#d9d4c8'];