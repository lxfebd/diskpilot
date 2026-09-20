// 四级权限模型（对齐「AI × 图吧工具箱集成设计」）：
//   L0 只读 —— 始终开启，不可关闭：读硬件信息 / 硬盘健康 / 生成报告
//   L1 受控 —— 默认开启，每次执行需用户确认（可设「本次会话免确认」）
//   L2 高危 —— 默认关闭，用户需在「AI 权限中心」手动开启，每次仍需确认
//   L3 永久禁止 —— 不可解锁：格式化 / 改 BIOS / 超频 / 删系统文件
//
// 用户拥有「给钥匙」的权利，但有些门没有锁孔——这不是不信任用户，是保护用户。
//
// 文案：这里是**数据表**（模块顶层求值），所以 label/desc 存的是 i18n 键名
// （`perm.<id>.label` / `perm.<id>.desc`），由消费组件在渲染处 t() 求值。
// 直接在顶层写中文会把文案烤死在首次 import，切语言不生效。

import type { PermLevel, AuthMode } from './api';

export type PermId =
  | 'hw.read'
  | 'hw.disk_health'
  | 'hw.report'
  | 'hw.stress'
  | 'hw.export'
  | 'cli.run'
  | 'power.plan'
  | 'cleanup.execute'
  | 'plugin.manage'
  | 'scaffold.manage'
  | 'startup.manage'
  | 'fan.adjust'
  | 'fan.control'
  | 'driver.check'
  | 'mcp.manage'
  | 'sys.control'
  | 'file.recycle'
  | 'app.uninstall'
  | 'disk.format'
  | 'bios.flash'
  | 'hw.overclock'
  | 'system.files';

export interface PermDef {
  id: PermId;
  level: PermLevel;
  /** 名称文案键（渲染处 t(permLabelKey(id))） */
  labelKey: string;
  /** 说明文案键 */
  descKey: string;
  authMode: AuthMode;
}

const key = (id: PermId, what: 'label' | 'desc') => `perm.${id}.${what}`;

/** 权限表种子：文案以键名形式在渲染处求值。 */
type PermSeed = Pick<PermDef, 'id' | 'level' | 'authMode'>;

export const PERMS: PermDef[] = ([
  { id: 'hw.read', level: 'L0', authMode: 'single' },
  { id: 'hw.disk_health', level: 'L0', authMode: 'single' },
  { id: 'hw.report', level: 'L0', authMode: 'single' },
  { id: 'hw.stress', level: 'L1', authMode: 'session' },
  { id: 'hw.export', level: 'L1', authMode: 'single' },
  { id: 'cli.run', level: 'L1', authMode: 'session' },
  { id: 'power.plan', level: 'L2', authMode: 'session' },
  { id: 'cleanup.execute', level: 'L2', authMode: 'single' },
  { id: 'plugin.manage', level: 'L2', authMode: 'session' },
  { id: 'scaffold.manage', level: 'L2', authMode: 'single' },
  { id: 'startup.manage', level: 'L2', authMode: 'session' },
  { id: 'fan.adjust', level: 'L2', authMode: 'session' },
  { id: 'fan.control', level: 'L2', authMode: 'single' },
  { id: 'driver.check', level: 'L2', authMode: 'session' },
  { id: 'mcp.manage', level: 'L2', authMode: 'single' },
  { id: 'sys.control', level: 'L2', authMode: 'single' },
  { id: 'file.recycle', level: 'L2', authMode: 'single' },
  { id: 'app.uninstall', level: 'L2', authMode: 'single' },
  // L3：门没有锁孔。以下操作一律拒绝（isPermEnabled 对 L3 恒 false），
  // 工具墙启动也被 manifest 的 permission_level==='L3' 拦截（如 FPT64 刷 BIOS）。
  { id: 'disk.format', level: 'L3', authMode: 'single' },
  { id: 'bios.flash', level: 'L3', authMode: 'single' },
  { id: 'hw.overclock', level: 'L3', authMode: 'single' },
  { id: 'system.files', level: 'L3', authMode: 'single' },
] as PermSeed[]).map((p) => ({ ...p, labelKey: key(p.id, 'label'), descKey: key(p.id, 'desc') }));

const STORAGE_KEY = 'diskpilot.perms.v1';

export interface PermConfig {
  enabled: Record<PermId, boolean>;
}

const DEFAULT_ENABLED: Record<PermId, boolean> = {
  'hw.read': true,
  'hw.disk_health': true,
  'hw.report': true,
  'hw.stress': true,
  'hw.export': true,
  'cli.run': true,
  'power.plan': false,
  'cleanup.execute': false,
  'plugin.manage': false,
  'scaffold.manage': false,
  'startup.manage': false,
  'fan.adjust': false,
  'fan.control': false,
  'driver.check': false,
  'mcp.manage': false,
  'sys.control': false,
  'file.recycle': false,
  'app.uninstall': false,
  // L3 永远 false——不过 isPermEnabled 会直接短路，这里只是把键补全
  // 免得 loadPermConfig 逐键合并时把存量配置里的 L3 误读为可开关。
  'disk.format': false,
  'bios.flash': false,
  'hw.overclock': false,
  'system.files': false,
};

export function defaultPermConfig(): PermConfig {
  return { enabled: { ...DEFAULT_ENABLED } };
}

export function loadPermConfig(): PermConfig {
  if (typeof window === 'undefined') return defaultPermConfig();
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaultPermConfig();
    const parsed = JSON.parse(raw) as Partial<PermConfig>;
    const enabled: Record<PermId, boolean> = { ...DEFAULT_ENABLED };
    if (parsed.enabled && typeof parsed.enabled === 'object') {
      for (const k of Object.keys(parsed.enabled)) {
        if (k in DEFAULT_ENABLED) {
          enabled[k as PermId] = Boolean((parsed.enabled as Record<string, boolean>)[k]);
        }
      }
    }
    return { enabled };
  } catch {
    return defaultPermConfig();
  }
}

export function savePermConfig(cfg: PermConfig): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(cfg));
  } catch { /* 忽略配额/隐私模式异常 */ }
}

export function resetPermConfig(): PermConfig {
  const cfg = defaultPermConfig();
  savePermConfig(cfg);
  return cfg;
}

/** L0 始终为 true，不可关闭；L3 永远 false（不可解锁）；L1/L2 按配置。 */
export function isPermEnabled(id: PermId, cfg?: PermConfig): boolean {
  const def = PERMS.find((p) => p.id === id);
  if (def?.level === 'L0') return true;
  if (def?.level === 'L3') return false;
  const c = cfg ?? loadPermConfig();
  return c.enabled[id] ?? DEFAULT_ENABLED[id];
}

/** 会话级授权缓存：本次对话期间免重复确认。agentChat 开头清空。 */
const sessionAuth = new Set<PermId>();

export function grantSessionAuth(id: PermId): void {
  sessionAuth.add(id);
}

export function hasSessionAuth(id: PermId): boolean {
  return sessionAuth.has(id);
}

export function clearSessionAuths(): void {
  sessionAuth.clear();
}

export function permDef(id: PermId): PermDef | undefined {
  return PERMS.find((p) => p.id === id);
}
