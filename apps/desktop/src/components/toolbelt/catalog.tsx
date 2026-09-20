// ── 工具墙共享展示助手：分类/风险/推广文案 + 图标映射 ──
// 供 toolwall / 详情卡 / 服务面板共用。
import { useState } from 'react';
import {
  Activity, CircuitBoard, Cpu, ExternalLink, Flame, Gamepad2, Gauge,
  HardDrive, Keyboard, Megaphone, MemoryStick, Monitor, Package, ShieldAlert,
  ShieldCheck, ShieldX, Star,
  type LucideIcon,
} from 'lucide-react';
import { t } from '../../i18n';
import { CATEGORY_COLORS as categoryColors, CATEGORY_FALLBACK } from '../../colors';
import type { ToolbeltCatalogItem, ToolbeltToolSpec } from '../../api';

// 图吧工具箱 Tools 下的中文分类目录名（后端返回，用于查表匹配，**不是文案**，故不译）。
// 标 @i18n-keep：它们是匹配用的字符串常量，按约定保持原生书写、不计入欠账。
const CAT_DIR = {
  hardware: '硬件信息', // @i18n-keep
  cpuTool: '处理器工具', // @i18n-keep
  cpu: 'CPU工具', // @i18n-keep
  board: '主板工具', // @i18n-keep
  mem: '内存工具', // @i18n-keep
  gpu: '显卡工具', // @i18n-keep
  diskTool: '硬盘工具', // @i18n-keep
  disk: '磁盘工具', // @i18n-keep
  screenTool: '显示器工具', // @i18n-keep
  screen: '屏幕工具', // @i18n-keep
  detect: '综合检测', // @i18n-keep
  general: '综合工具', // @i18n-keep
  common: '常用工具', // @i18n-keep
  periph: '外设工具', // @i18n-keep
  burn: '烤鸡工具', // @i18n-keep
  game: '游戏工具', // @i18n-keep
  other: '其他工具', // @i18n-keep
};

// 工具 → 图标映射。按工具名优先匹配，命中不了回退到分类。
// 学图吧工具箱/开源工具箱 UI：每张卡片放一个一眼可辨的彩色软件图标。
export const CATEGORY_ICON: Record<string, LucideIcon> = {
  [CAT_DIR.cpuTool]: Cpu,
  [CAT_DIR.gpu]: Monitor,
  [CAT_DIR.diskTool]: HardDrive,
  [CAT_DIR.detect]: Gauge,
  [CAT_DIR.other]: Package,
  [CAT_DIR.burn]: Flame,
  [CAT_DIR.mem]: MemoryStick,
  [CAT_DIR.common]: Star,
  [CAT_DIR.periph]: Keyboard,
  [CAT_DIR.screenTool]: Monitor,
  [CAT_DIR.board]: CircuitBoard,
  [CAT_DIR.game]: Gamepad2,
};

export function toolIconFor(tool: ToolbeltToolSpec): LucideIcon {
  // 常见工具名直接命中具体图标，比纯分类更"一眼可辨"
  if (tool.name === 'WizTree' || tool.name === 'DiskGenius' || tool.name === 'Defraggler' || tool.name === 'CrystalDiskInfo') return HardDrive;
  if (tool.name === 'AIDA64' || tool.name === 'HWiNFO') return Activity;
  if (tool.name === 'Everything') return Activity;
  if (tool.name === 'Ventoy') return Package;
  if (tool.name === 'UltraISO' || tool.name === 'WinDbg' || tool.name === 'Autoruns') return Package;
  return CATEGORY_ICON[tool.category] ?? Package;
}

export const CATEGORY_COLOR = categoryColors;
export function toolColorFor(tool: ToolbeltToolSpec): string {
  return CATEGORY_COLOR[tool.category] ?? CATEGORY_FALLBACK;
}

// ── 小白人话：把卡片上的术语简介翻译成一句话人话 ──
// 图吧工具箱卡片 description 来自后端 cli_tools_doc.md 的「名称 —— 简介」，
// 是技术向一句话（如「SMART 健康信息」「蓝屏 dump 分析」），不认识术语的新手
// 看了不知道「这东西对我有什么用」。这里按工具名（大小写不敏感）给常见工具
// 补一条大白话描述，命中则优先显示；未命中（用户自己放的工具）回落后端原文。
// 键值均为 i18n 键：value 是 toolbelt.plain.* 文案键。
const PLAIN_TOOLS: Record<string, string> = {
  'crystaldiskinfo': 'toolbelt.plain.crystaldiskinfo',
  'crystaldiskmark': 'toolbelt.plain.crystaldiskmark',
  'wiztree': 'toolbelt.plain.wiztree',
  'spacesniffer': 'toolbelt.plain.spacesniffer',
  'everything': 'toolbelt.plain.everything',
  'defraggler': 'toolbelt.plain.defraggler',
  'diskgenius': 'toolbelt.plain.diskgenius',
  'aida64': 'toolbelt.plain.aida64',
  'hwinfo': 'toolbelt.plain.hwinfo',
  'hwmonitor': 'toolbelt.plain.hwmonitor',
  'cpuz': 'toolbelt.plain.cpuz',
  'gpuz': 'toolbelt.plain.gpuz',
  'coretemp': 'toolbelt.plain.coretemp',
  'throttlestop': 'toolbelt.plain.throttlestop',
  'prime95': 'toolbelt.plain.prime95',
  'furmark': 'toolbelt.plain.furmark',
  'urwtest': 'toolbelt.plain.urwtest',
  'ventoy': 'toolbelt.plain.ventoy',
  'rufus': 'toolbelt.plain.rufus',
  'ultraiso': 'toolbelt.plain.ultraiso',
  'ddu': 'toolbelt.plain.ddu',
  'bluescreenview': 'toolbelt.plain.bluescreenview',
  'autoruns': 'toolbelt.plain.autoruns',
  'batteryinfoview': 'toolbelt.plain.batteryinfoview',
  'usbdeview': 'toolbelt.plain.usbdeview',
  'windbg': 'toolbelt.plain.windbg',
  'hdtune': 'toolbelt.plain.hdtune',
  'dism++': 'toolbelt.plain.dismpp',
};
// 大小写不敏感命中（含双名工具「Autoruns / autorunsc」的前缀段）。
export function plainDescriptionFor(name: string): string | null {
  const n = name.trim().toLowerCase();
  for (const [k, v] of Object.entries(PLAIN_TOOLS)) {
    if (n === k || n.startsWith(k + ' ') || n.startsWith(k + '/')) return t(v);
  }
  return null;
}

// 磁盘目录名 → 分类 slug（目录名不统一：处理器工具/CPU工具、硬盘工具/磁盘工具…）。
// 键是后端返回的中文目录名（匹配用，见 CAT_DIR 说明），值是稳定英文 slug，真正的显示文案在 toolbelt.cat.* 表里。
const CATEGORY_SLUG: Record<string, string> = {
  [CAT_DIR.hardware]: 'hardware',
  [CAT_DIR.cpuTool]: 'cpu',
  [CAT_DIR.cpu]: 'cpu',
  [CAT_DIR.board]: 'board',
  [CAT_DIR.mem]: 'mem',
  [CAT_DIR.gpu]: 'gpu',
  [CAT_DIR.diskTool]: 'disk',
  [CAT_DIR.disk]: 'disk',
  [CAT_DIR.screenTool]: 'screen',
  [CAT_DIR.screen]: 'screen',
  [CAT_DIR.detect]: 'general',
  [CAT_DIR.general]: 'general',
  [CAT_DIR.common]: 'general',
  [CAT_DIR.periph]: 'periph',
  [CAT_DIR.burn]: 'burn',
  [CAT_DIR.game]: 'game',
  [CAT_DIR.other]: 'other',
};

export function catDisplayName(dirName: string): string {
  switch (CATEGORY_SLUG[dirName] ?? dirName) {
    case 'hardware': return t('toolbelt.cat.hardware');
    case 'cpu': return t('toolbelt.cat.cpu');
    case 'board': return t('toolbelt.cat.board');
    case 'mem': return t('toolbelt.cat.mem');
    case 'gpu': return t('toolbelt.cat.gpu');
    case 'disk': return t('toolbelt.cat.disk');
    case 'screen': return t('toolbelt.cat.screen');
    case 'general': return t('toolbelt.cat.general');
    case 'periph': return t('toolbelt.cat.periph');
    case 'burn': return t('toolbelt.cat.burn');
    case 'game': return t('toolbelt.cat.game');
    case 'other': return t('toolbelt.cat.other');
    // 未知分类（用户自己建的目录名）原样显示，与旧行为一致。
    default: return dirName;
  }
}

export const RISK_ICON: Record<string, LucideIcon> = {
  low: ShieldCheck,
  medium: ShieldAlert,
  high: ShieldX,
};
// 风险等级 → 显示标签：函数式求值（切语言即时生效）；未知等级返回 undefined。
export function riskLabelDisplay(risk: string): string | undefined {
  if (risk === 'low') return t('toolbelt.risk.low');
  if (risk === 'medium') return t('toolbelt.risk.medium');
  if (risk === 'high') return t('toolbelt.risk.high');
  return undefined;
}

// ── 迁移内容物管控：已知「免费版内含广告/赞助/商业推广」的第三方工具 ──
// 这些工具（图吧工具箱原版目录里）自身带广告弹窗 / 赞助按钮 / 付费推广，
// 启动前明确提示用户，避免毫无防备地看到广告。名单按工具名（大小写不敏感）
// 匹配；命中后在卡片打「含推广」徽标、启动确认文案里注明。
// 键是工具名（匹配用，不译），值是 toolbelt.ad.* 文案键。
const AD_TOOLS: Record<string, string> = {
  'ddu': 'toolbelt.ad.ddu',
  'display driver uninstaller': 'toolbelt.ad.ddu',
  'coretemp': 'toolbelt.ad.coretemp',
  'core temp': 'toolbelt.ad.coretemp',
  'wiztree': 'toolbelt.ad.wiztree',
  'aida64': 'toolbelt.ad.aida64',
  'aida64 稳定性测试': 'toolbelt.ad.aida64', // @i18n-keep 目录名尾巴，匹配用
  'ventoy': 'toolbelt.ad.ventoy',
  'msi afterburner': 'toolbelt.ad.msiAfterburner',
  'geek uninstaller': 'toolbelt.ad.geekUninstaller',
  'ultraiso': 'toolbelt.ad.ultraiso',
};
export function adNoticeFor(name: string): string | null {
  const n = name.trim().toLowerCase();
  if (AD_TOOLS[n]) return t(AD_TOOLS[n]);
  // 尾部模糊匹配（如 "AIDA64 稳定性测试" 命中 "aida64"）
  for (const [k, v] of Object.entries(AD_TOOLS)) {
    if (n.endsWith(k) || k.endsWith(n)) return t(v);
  }
  return null;
}

function catalogIconFor(name: string, category: string): LucideIcon {
  const n = name.toLowerCase();
  if (n.includes('wiztree') || n.includes('diskgenius') || n.includes('defraggler') || n.includes('crystaldiskinfo')) return HardDrive;
  if (n.includes('aida') || n.includes('hwinfo')) return Activity;
  if (n.includes('cpuz')) return Cpu;
  if (n.includes('prime') || n.includes('furmark') || n.includes('linx')) return Flame;
  return CATEGORY_ICON[category] ?? Package;
}

// 工具墙/详情卡共用的「带兜底的工具图标」：优先显示 exe 抽取的真实图标，
// 加载失败（损坏/超限/空 data URI）时自动降级到分类 Lucide 图标，避免空白格。
// .bat/.cmd/.lnk 跳转项（官网下载/在线工具/推广）没有本地 exe 图标，
// 统一用 ExternalLink 表达「点开是网页」；推广项用推广色进一步区分。
export function SafeToolIcon({ tool, size, color }: { tool: ToolbeltCatalogItem; size: number; color: string }) {
  const [broken, setBroken] = useState(false);
  const isJump = (tool.extension === 'bat' || tool.extension === 'cmd' || tool.extension === 'lnk') && !tool.icon;
  const Fallback = isJump
    ? (tool.is_promotion ? Megaphone : ExternalLink)
    : catalogIconFor(tool.name, tool.category);
  if (!tool.icon || broken) {
    return <Fallback size={size} color={color} strokeWidth={1.8} />;
  }
  return (
    <img
      className={size > 18 ? 'toolwall-icon-img' : 'tdm-icon-img'}
      src={tool.icon}
      alt={tool.name}
      draggable={false}
      onError={() => setBroken(true)}
    />
  );
}