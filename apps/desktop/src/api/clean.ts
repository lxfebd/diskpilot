import { invoke } from '@tauri-apps/api/core';
import type { Scaffold, UndoEntry, CondaEnv } from '../types';
import { isTauri } from '../env';
import * as mocks from '../mocks';
import { t } from '../i18n';

/**
 * 清理域：scaffold 管理 / 清理建议 / 执行（scope 与 AI 计划）/ 撤销。
 * 所有删除路径都遵守「先清单 + 用户确认 + 默认回收站」的铁律，
 * 前端既有确认闸在此保持原样（executeAiPlan 的 userConfirmed 硬校验不变）。
 * 由 api.ts 聚合进 `api` 对象，保持 `api.executeScope()` 等调用面不变。
 */
export const cleanApi = {
  listScaffolds: () =>
    isTauri ? invoke<Scaffold[]>('list_scaffolds') : Promise.resolve(mocks.SCAFFOLDS),

  /** 一键安装社区 scaffold：后端校验 toml → 写入用户目录 → 热重载。返回脚本 id。 */
  installScaffold: (toml: string): Promise<string> =>
    isTauri ? invoke<string>('install_scaffold', { toml }) : Promise.resolve(''),

  /** 把 AI 权限快照同步到后端（权限中心改动/应用启动时调用）。后端写命令
   * （execute_ai_plan / toolbelt_run / plugin_*）据此做纵深校验。 */
  syncPerms: (enabled: string[]): Promise<void> =>
    isTauri ? invoke<void>('sync_perms', { enabled }) : Promise.resolve(),

  /** 卸载用户安装的 scaffold（内嵌脚本不可卸载，后端拒绝）。 */
  uninstallScaffold: (id: string): Promise<void> =>
    isTauri ? invoke<void>('uninstall_scaffold', { id }) : Promise.resolve(),

  /** 查询脚本来源：'embedded' | 'user'。 */
  scaffoldSource: (id: string): Promise<string> =>
    isTauri ? invoke<string>('scaffold_source', { id }) : Promise.resolve('embedded'),

  /** 社区脚本仓库列表（只读 L0）：条目 + 告警。 */
  scaffoldRegistryList: (): Promise<ScaffoldRegistryOut> =>
    isTauri
      ? invoke<ScaffoldRegistryOut>('scaffold_registry_list')
      : Promise.resolve({ items: [], warning: null, updated_at: 0 }),

  /** 从社区仓库一键安装（L2 scaffold.manage + confirmed 双校验，Ed25519 签名强制）。 */
  scaffoldRegistryInstall: (id: string, confirmed: boolean): Promise<string> =>
    isTauri
      ? invoke<string>('scaffold_registry_install', { id, confirmed })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'scaffold_registry_install' }))),

  scopeSizesBatch: (
    scaffoldId: string,
    rootPaths: string[],
    scopeDays?: Record<string, number>,
    wxidFilter?: string[],
    envFilter?: string[],
  ) =>
    isTauri
      ? invoke<{ root_path: string; sizes: { scope_id: string; bytes: number; file_count: number; total_bytes: number; total_files: number }[] }[]>('scope_sizes_batch', {
          scaffoldId,
          rootPaths,
          scopeDays: scopeDays ?? null,
          wxidFilter: wxidFilter ?? null,
          envFilter: envFilter ?? null,
        })
      : mocks.scopeSizesBatch(scaffoldId, rootPaths),

  cleanupSuggestions: (
    rootPath: string,
    scopeDays?: Record<string, number>,
    cachedOnly?: boolean,
  ) =>
    isTauri
      ? invoke<{ scaffold_id: string; scope_id: string; label: string; desc: string; path: string; bytes: number; files: number; total_bytes: number }[]>('cleanup_suggestions', {
          rootPath,
          scopeDays: scopeDays ?? null,
          cachedOnly: cachedOnly ?? false,
        })
      : Promise.resolve([] as { scaffold_id: string; scope_id: string; label: string; desc: string; path: string; bytes: number; files: number; total_bytes: number }[]),

  /**
   * 执行单个 scaffold scope。`dryRun=true` 只预览不落盘，可随意调；
   * `dryRun=false` 是真实清理，必须显式带 `confirmed: true`（调用方只在
   * 用户看完清单并点「确认执行」之后才置位）——后端 `execute_scope` 同口径
   * 硬校验，前端这一层先拦住误用。
   */
  executeScope: (
    scaffoldId: string,
    scopeId: string,
    rootPath: string,
    dryRun: boolean,
    opts: ExecuteScopeOpts = {},
  ) => {
    if (!dryRun && opts.confirmed !== true) {
      return Promise.reject(new Error(t('errors.confirmExecuteScope')));
    }
    return isTauri
      ? invoke<UndoEntry[]>('execute_scope', {
          scaffoldId,
          scopeId,
          rootPath,
          dryRun,
          olderThanDays: opts.olderThanDays ?? null,
          wxidFilter: opts.wxidFilter ?? null,
          envFilter: opts.envFilter ?? null,
          confirmed: opts.confirmed ?? null,
        })
      : Promise.resolve([] as UndoEntry[]);
  },

  // AI 清理执行入口（仅由确认窗在用户显式确认后调用）。前端第一道闸：
  // userConfirmed 不为 true 直接拒绝；只传用户勾选的 selectedPaths，
  // 未勾选项根本不发给后端。后端 execute_ai_plan 会二次校验同样条件。
  executeAiPlan: (selectedPaths: string[], userConfirmed: boolean, dryRun: boolean) => {
    if (!userConfirmed) {
      return Promise.reject(new Error(t('errors.confirmAiPlan')));
    }
    if (selectedPaths.length === 0) {
      return Promise.resolve([] as UndoEntry[]);
    }
    return isTauri
      ? invoke<UndoEntry[]>('execute_ai_plan', { paths: selectedPaths, userConfirmed, dryRun })
      : Promise.resolve([] as UndoEntry[]);
  },

  listUndo: (limit?: number) =>
    isTauri ? invoke<UndoEntry[]>('list_undo', { limit: limit ?? null }) : Promise.resolve([]),

  // index 是展示列表位置；source 是与后端对齐的条目路径（防列表在两次
  // 刷新之间变化导致 index 漂移、恢复错条目——后端会双重校验）。
  undo: (index: number, source: string) =>
    isTauri ? invoke<UndoEntry | null>('undo', { index, source }) : Promise.resolve(null),

  listCondaEnvs: (condaRoot: string) =>
    isTauri
      ? invoke<CondaEnv[]>('list_conda_envs', { condaRoot })
      : Promise.resolve([] as CondaEnv[]),

  inspect: (path: string, sampleCount: number) =>
    isTauri ? invoke<string[]>('inspect_path', { path, sampleCount }) : mocks.inspect(path, sampleCount),

  revealInExplorer: (path: string) =>
    isTauri ? invoke<void>('reveal_in_explorer', { path }) : Promise.resolve(),

  /**
   * 把用户点选的显式路径移入回收站（文件树右键 / 聊天建议卡）。
   * 动作由后端 `recycle_paths` 写死成 Recycle，前端提交不了「永久删除」；
   * `confirmed` 必须为 true —— 调用方在两步确认弹窗通过后才置位。
   */
  recyclePaths: (paths: string[], reason: string, confirmed: boolean): Promise<UndoEntry[]> => {
    if (!confirmed) {
      return Promise.reject(new Error(t('errors.confirmRecyclePaths')));
    }
    if (paths.length === 0) {
      return Promise.resolve([] as UndoEntry[]);
    }
    return isTauri
      ? invoke<UndoEntry[]>('recycle_paths', { paths, reason, confirmed })
      : mocks.recyclePaths(paths, reason);
  },

  // 重复文件结构化扫描（R2）：只读建议，不删任何东西。返回分组，每组含
  // keep 建议（保留哪份）与 recycle_candidates（可回收副本）。清理动作
  // 由前端确认窗经 executeAiPlan（user_confirmed）执行，本命令无写权限。
  scanDuplicateFiles: (rootPath: string, minSize?: number) =>
    isTauri
      ? invoke<DupGroup[]>('scan_duplicate_files_cmd', {
          rootPath,
          minSize: minSize ?? null,
        })
      : Promise.resolve([] as DupGroup[]),

  // ── 清理提醒（R3）──
  getReminderConfig: () =>
    isTauri
      ? invoke<ReminderConfig>('get_reminder_config')
      : Promise.resolve({ enabled: false, interval_hours: 24, min_bytes: 200 * 1024 * 1024 }),

  setReminderConfig: (patch: {
    enabled?: boolean;
    interval_hours?: number;
    min_bytes?: number;
  }) =>
    isTauri
      ? invoke<ReminderConfig>('set_reminder_config', {
          enabled: patch.enabled ?? null,
          intervalHours: patch.interval_hours ?? null,
          minBytes: patch.min_bytes ?? null,
        })
      : Promise.resolve({ enabled: false, interval_hours: 24, min_bytes: 200 * 1024 * 1024 }),

  /** 手动触发一次提醒检查（只读，只出清单 + 通知，绝不执行）。 */
  runReminderCheck: () =>
    isTauri
      ? invoke<ReminderPayload | null>('run_reminder_check_cmd')
      : Promise.resolve(null),

  // ── 磁盘空间历史（R6）──
  /** 读取全部磁盘空间趋势（只读）：`{root -> [快照]}`，按时间顺序。
   *  根目录是扫描根（C:\ 等）；缺历史返回空对象。 */
  getSpaceHistory: () =>
    isTauri
      ? invoke<Record<string, SpacePoint[]>>('get_space_history')
      : Promise.resolve({}),
};

// 清理域内部类型（节点正由 types.ts 提供；此处为本域独立形状，语义同 rust 端
// 返回值，前端组件 import type 时从 '../api' 统一取，勿在此重复导出同形类型）

/** executeScope 的可选旋钮（避免七个位置参数互相穿插）。 */
export interface ExecuteScopeOpts {
  /** 只清 mtime 早于 N 天的文件（scope.prompt.kind === 'days' 时用）。 */
  olderThanDays?: number;
  /** 微信多账号：只清这些 wxid 目录。 */
  wxidFilter?: string[];
  /** conda / 环境类 scope 的白名单过滤。 */
  envFilter?: string[];
  /** 真实执行（dryRun=false）必须显式 true；后端 execute_scope 同口径硬校验。 */
  confirmed?: boolean;
}

// ── 重复文件建议（R2，后端 dups.rs 返回形状）──

/** 单个重复候选文件 */
export interface DupFile {
  path: string;
  /** 文件 mtime（UNIX 秒）。读不到为 null。 */
  mtime: number | null;
  /** 独占打开失败（被进程占用）→ 判定为运行中，不进可回收候选 */
  is_running: boolean;
}

/** 一组重复文件（size 分组 + 头部哈希聚类后 ≥2 个） */
export interface DupGroup {
  /** 组内文件大小（字节，>0） */
  size: number;
  /** 组内全部文件（含运行中文件，展示用） */
  files: DupFile[];
  /** 建议保留的文件 path（启发式：path 短 + mtime 旧） */
  keep: string;
  /** 建议回收的文件 path（= files 中非 keep 且非运行中） */
  recycle_candidates: string[];
  /** 组内运行中文件 path（展示「跳过原因」） */
  running: string[];
}

// ── 清理提醒（R3，后端 reminder.rs 返回形状）──

/** 提醒配置（持久化 app_data_dir/reminder.json） */
export interface ReminderConfig {
  /** 总开关。false = 后台线程不干活（默认） */
  enabled: boolean;
  /** 间隔小时（1..=168） */
  interval_hours: number;
  /** 至少多少字节才值得提醒（默认 200MB） */
  min_bytes: number;
}

/** 单盘可清字节（只读统计） */
export interface DriveCleanup {
  path: string;
  bytes: number;
}

/** 提醒负载（emit 给前端） */
export interface ReminderPayload {
  drives: DriveCleanup[];
  total_bytes: number;
  ts: number;
}

// ── 磁盘空间历史（R6，后端 space_history.rs 返回形状）──

/** 单条空间快照（扫描完成后追加一行 space-history.jsonl） */
export interface SpacePoint {
  /** 扫描根路径（如 C:\ 或 D:\dir） */
  root: string;
  /** 卷总容量（字节） */
  total_bytes: number;
  /** 已用容量（字节，= total - free） */
  used_bytes: number;
  /** Unix 秒 */
  at: number;
}
// ── 社区脚本仓库（R8，后端 scaffold_registry.rs 返回形状）──

/** 索引条目：一份可安装的社区清理脚本 */
export interface RegistryScaffold {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  risk: string;
  tags: string[];
  url: string;
  sha256: string;
  signature: string;
  signer: string;
  downloads: number;
}

/** scaffold_registry_list 返回：条目 + 告警 + 更新时间 */
export interface ScaffoldRegistryOut {
  items: RegistryScaffold[];
  warning: string | null;
  updated_at: number;
}
