import { invoke } from '@tauri-apps/api/core';
import type { Scaffold, UndoEntry, CondaEnv, PreflightReport } from '../types';
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
    isTauri ? invoke<string>('install_scaffold', { toml }) : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'install_scaffold' }))),

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
      : mocks.scaffoldRegistryList(),

  /** 从社区仓库一键安装（L2 scaffold.manage + confirmed 双校验，Ed25519 签名强制）。 */
  scaffoldRegistryInstall: (id: string, confirmed: boolean): Promise<string> =>
    isTauri
      ? invoke<string>('scaffold_registry_install', { id, confirmed })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'scaffold_registry_install' }))),

  /**
   * Winapp2.ini 社区规则导入预览（借 BleachBit 生态取长补短）：
   * 把社区 winapp2.ini 文本解析成可清理 scaffold 列表，**只预览不写盘**。
   * 危险条目已由后端红线闸标注 `red_line` 拒绝理由，UI 据此红字提示；
   * 用户确认后再逐条走 installScaffold 入库。纯解析，无任何系统副作用。
   */
  parseWinapp2: (content: string): Promise<Winapp2PreviewItem[]> =>
    isTauri
      ? invoke<Winapp2PreviewItem[]>('parse_winapp2', { content })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'parse_winapp2' }))),

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
      : mocks.scopeSizesBatch(scaffoldId, rootPaths, scopeDays),

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
      : mocks.cleanupSuggestions(rootPath, scopeDays, cachedOnly),

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
      : mocks.executeScope(scaffoldId, scopeId, rootPath, dryRun, opts);
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
      : mocks.executeAiPlan(selectedPaths, userConfirmed, dryRun);
  },

  /** R289：AI 清理预演（纯只读，不要求用户确认）——AI 自己核实清单里每条
   *   路径是否真实存在、能清多少字节；预演不回收、不写台账。真执行仍只走
   *   executeAiPlan（userConfirmed，确认窗是唯一放行条件）。 */
  executeAiPlanDryRun: (paths: string[]) => {
    if (paths.length === 0) return Promise.resolve([] as UndoEntry[]);
    return isTauri
      ? invoke<UndoEntry[]>('execute_ai_plan', { paths, userConfirmed: false, dryRun: true })
      : mocks.executeAiPlan(paths, false, true);
  },

  listUndo: (limit?: number) =>
    isTauri ? invoke<UndoEntry[]>('list_undo', { limit: limit ?? null }) : mocks.listUndo(limit),

  // P2b 清理预检（只读）：建议退出的进程里还在跑的 + 推荐预选 scope id。
  // 绝不在这里杀进程——杀进程是独立写路径，走 toolbelt 的确认闸。
  cleanupPreflight: (scaffoldId: string) =>
    isTauri
      ? invoke<PreflightReport>('cleanup_preflight', { scaffoldId })
      : Promise.resolve({ scaffold_id: scaffoldId, running_processes: [], recommended_scope_ids: [] }),

  // index 是展示列表位置；source 是与后端对齐的条目路径（防列表在两次
  // 刷新之间变化导致 index 漂移、恢复错条目——后端会双重校验）。
  undo: (index: number, source: string) =>
    isTauri ? invoke<UndoEntry | null>('undo', { index, source }) : mocks.undo(index, source),

  /** 打开系统回收站（只开窗口，不代还原）——「回收」类条目的还原出口。 */
  openRecycleBin: () =>
    isTauri
      ? invoke<void>('open_recycle_bin')
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'open_recycle_bin' }))),

  listCondaEnvs: (condaRoot: string) =>
    isTauri
      ? invoke<CondaEnv[]>('list_conda_envs', { condaRoot })
      : mocks.listCondaEnvs(),

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
      : mocks.scanDuplicateFiles(rootPath, minSize),

  // ── 清理提醒（R3）──
  /** R293 主动查询某盘「多久没扫」：只读 scan-stamps.jsonl，零磁盘扫描。
   *  root 传任意路径都行，后端按盘根归一化匹配（C:\\ / C:/ / c: 同一条记录）。
   *  三态必须可区分：never=还没扫过、clock_skew=时间戳异常、days=距上次天数。 */
  scanStaleness: (root: string): Promise<ScanStaleness | null> =>
    isTauri ? invoke<ScanStaleness>('scan_staleness_cmd', { root }) : Promise.resolve(null),

  /** R480 扫描摘要历史：scan-summary.jsonl 全量（按追加序，每 root 最近 30 条）。
   *  缺文件返回空数组；读失败由后端 Err 直接抛给调用侧。 */
  scanSummaryHistory: (): Promise<ScanSummaryView[] | null> =>
    isTauri ? invoke<ScanSummaryView[]>('scan_summary_history') : Promise.resolve(null),

  getReminderConfig: () =>
    isTauri
      ? invoke<ReminderConfig>('get_reminder_config')
      : mocks.getReminderConfig(),

  setReminderConfig: (patch: {
    enabled?: boolean;
    interval_hours?: number;
    min_bytes?: number;
    weekly_report_enabled?: boolean;
    weekly_interval_days?: number;
    full_warn_days?: number;
    /** R297 「这块盘多久没扫」阈值（天）；0 = 关闭该提醒。后端钳 ≤3650。 */
    scan_stale_days?: number;
    /** R291 温度持续采样开关。关掉只是不采，不影响已有历史样本。 */
    thermal_sample_enabled?: boolean;
    /** 持续偏高阈值 ℃（后端钳 50..=110）。 */
    thermal_sustained_c?: number;
    /** 连续采样点数（后端钳 1..=60；1 = 瞬时判定）。 */
    thermal_sustained_samples?: number;
    /** R292 启动项变化监听开关。 */
    startup_watch_enabled?: boolean;
    /** 启动项快照间隔分钟（后端钳 1..=1440）。 */
    startup_watch_minutes?: number;
  }) =>
    isTauri
      ? invoke<ReminderConfig>('set_reminder_config', {
          enabled: patch.enabled ?? null,
          intervalHours: patch.interval_hours ?? null,
          minBytes: patch.min_bytes ?? null,
          weeklyReportEnabled: patch.weekly_report_enabled ?? null,
          weeklyIntervalDays: patch.weekly_interval_days ?? null,
          fullWarnDays: patch.full_warn_days ?? null,
          scanStaleDays: patch.scan_stale_days ?? null,
          // R298：热管理与启动项监听的 5 个旋钮。后端早已接收（reminder.rs:160-165），
          // 此前前端零入口，用户只能手改 reminder.json。
          thermalSampleEnabled: patch.thermal_sample_enabled ?? null,
          thermalSustainedC: patch.thermal_sustained_c ?? null,
          thermalSustainedSamples: patch.thermal_sustained_samples ?? null,
          startupWatchEnabled: patch.startup_watch_enabled ?? null,
          startupWatchMinutes: patch.startup_watch_minutes ?? null,
        })
      : mocks.setReminderConfig(patch),

  /** 手动触发一次提醒检查（只读，只出清单 + 通知，绝不执行）。 */
  runReminderCheck: () =>
    isTauri
      ? invoke<ReminderPayload | null>('run_reminder_check_cmd')
      : mocks.runReminderCheck(),

  /** R299：温度采样历史（只读）。此前这套点只有 reminder 后台线程自己读——
   *  用户在设置里打开采样开关，却看不到采到没有、采了多少、最高多少。
   *  三态与后端同口径：缺文件 = 从未采样 → 空数组；读失败 → 抛错（不伪装成
   *  「没有样本」）。浏览器模式无真实传感器历史 → 空数组（诚实：没读=没有）。 */
  getThermalHistory: (): Promise<ThermalPoint[]> =>
    isTauri ? invoke<ThermalPoint[]>('get_thermal_history') : mocks.getThermalHistory(),

  /** R300 立即对比启动项（只读）：读 startup-history.jsonl 的最后两份快照做集合 diff，
   *  区分「没历史」与「读失败」——真空态后端返回 Err("no_history")，UI 说「首次快照」，
   *  真实读失败说「未读到」。绝不用「无变化」糊住任一种。零系统扫描、零写入。 */
  checkStartupDelta: (): Promise<StartupChanged> =>
    isTauri ? invoke<StartupChanged>('check_startup_delta_cmd') : mocks.checkStartupDelta(),

  /** R100 立即检查各盘空间趋势（只读）：逐盘返回预计满盘天数/无法预测的原因。
    *  浏览器模式无真实历史 → 空列表（诚实：没读=没有）。 */
  checkSpaceForecast: () =>
    isTauri
      ? invoke<SpaceForecast[]>('check_space_forecast_cmd')
      : mocks.checkSpaceForecast(),

  // ── 磁盘空间历史（R6）──
  /** 读取全部磁盘空间趋势（只读）：`{root -> [快照]}`，按时间顺序。
   *  根目录是扫描根（C:\ 等）；缺历史返回空对象。 */
  getSpaceHistory: () =>
    isTauri
      ? invoke<Record<string, SpacePoint[]>>('get_space_history')
      : mocks.getSpaceHistory(),

  // ── 周巡检简报（health-butler P1）──
  /** 手动生成一份本周健康简报（只读 L0：扫建议 + 温度 + 硬件快照对比，绝不清理）。 */
  generateWeeklyReport: () =>
    isTauri
      ? invoke<WeeklyReport>('generate_weekly_report_cmd')
      : mocks.generateWeeklyReport(),
  /** 读最近 N 份周报（只读，历史页回显）。 */
  getWeeklyReports: (limit?: number) =>
    isTauri
      ? invoke<WeeklyReport[]>('get_weekly_reports', { limit: limit ?? 5 })
      : mocks.getWeeklyReports(limit),

  // ── 建议清理跨次对比（health-butler P0）──
  /** 某盘建议清理总量与上次的环比（只读 L0）。缺历史 → null；读失败 → reject。 */
  suggestionDiff: (root: string) =>
    isTauri
      ? invoke<SuggestionDiff | null>('suggestion_diff', { rootPath: root })
      : mocks.suggestionDiff(root),

  // ── 按项授权自动进清单（health-butler P2-1，后端 auto_list.rs）──
  /** 读授权规则集，并让后端按本机真实 scaffolds 标注每条规则的生效状态
   *  （ok / unknown_scaffold）。`knownScaffolds` 由调用方从 list_scaffolds 传入，
   *  后端不反向依赖 scaffold 加载路径。 */
  getAutoListConfigWithStatus: (knownScaffolds: string[]) =>
    isTauri
      ? invoke<AutoListConfigView>('get_auto_list_config_with_status', { knownScaffolds })
      : mocks.getAutoListConfigWithStatus(knownScaffolds),

  /** 覆写整套授权规则集（设置页持有的就是完整规则集）。
   *  **只影响「该不该主动告知」，不产生任何执行计划**——真执行仍只有
   *  `executeAiPlan(userConfirmed)` 一条路。不需要 confirm 参数：它写的是
   *  规则配置，不是文件删除。 */
  setAutoListConfig: (config: AutoListConfig): Promise<AutoListConfig> =>
    isTauri
      ? invoke<AutoListConfig>('set_auto_list_config', { config })
      : mocks.setAutoListConfig(config),
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
  /** 周巡检简报开关（health-butler P1，默认关） */
  weekly_report_enabled: boolean;
  /** 周报间隔天（1..=90，默认 7） */
  weekly_interval_days: number;
  /** 空间耗尽预警阈值天数（R99，默认 0=关）：按趋势外推 ≤N 天满的盘会提醒 */
  full_warn_days: number;
  /** 「多久没扫」提醒阈值天（R293，默认 21；0 = 关闭该提醒）。
   *  今日体检「扫描时效」格与它同口径：设置里改，体检跟着改。 */
  scan_stale_days: number;
  /** R291 温度持续采样开关（默认关）。独立于 enabled——采集本身零成本
   *  （顺手读一次传感器），开着它才能让「温度持续偏高」变得可判断。 */
  thermal_sample_enabled: boolean;
  /** 持续偏高判定阈值 ℃（默认 85，后端钳 50..=110）。低于睿频上限、高于
   *  日常轻负载，减少误报。 */
  thermal_sustained_c: number;
  /** 连续多少个采样点全超温才算「持续」（默认 5，后端钳 1..=60）。
   *  采样周期 5 分钟 → 默认 5 点 ≈ 连续 25 分钟；1 会退化成瞬时判定。 */
  thermal_sustained_samples: number;
  /** R292 启动项变化监听开关（默认关）。独立于 enabled：只想知道「启动项
   *  有没有被动过」的用户可以只开这个。 */
  startup_watch_enabled: boolean;
  /** 启动项快照间隔分钟（默认 60，后端钳 1..=1440）。 */
  startup_watch_minutes: number;
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
  /** health-butler P2-1 按项授权命中项：(scaffold_id, 可清字节)。
   *  只用于「免去想起来要看」的主动告知，不含任何执行计划。
   *  旧后端可能不带此字段 → 缺省空数组（向前兼容，不当作「没命中」以外的含义）。 */
  auto_list_hits: [string, number][];
  ts: number;
}

/** 一条按项授权规则：某 scaffold 可清量达到 `min_bytes` 时自动进清单。
 *  **只影响清单**，不触碰执行——真执行仍只有用户逐项确认后的
 *  `execute_ai_plan(user_confirmed)` 一条路。 */
export interface AutoListRule {
  /** scaffold id（如 `wechat`）。必须能在本机 scaffolds 里找到，否则闲置。 */
  scaffold_id: string;
  /** 触发阈值（字节）。达到即自动进清单，0 = 一有可清量就进。 */
  min_bytes: number;
}

/** 规则 + 其在本机 scaffolds 下的生效状态（UI 用它明示闲置原因）。 */
export interface AutoListRuleView extends AutoListRule {
  /** ok / unknown_scaffold：id 不在本机 scaffolds 中 → 规则闲置，不猜近似 id。 */
  status: 'ok' | 'unknown_scaffold';
}

/** 规则集（set_auto_list_config 的入参/返回值形状；不带生效状态）。 */
export interface AutoListConfig {
  /** 总开关。false = 任何规则都不生效（与 reminder.enabled 同语义）。 */
  enabled: boolean;
  rules: AutoListRule[];
}

/** 规则集 + 每条规则的状态（get_auto_list_config_with_status 返回形状）。 */
export interface AutoListConfigView {
  enabled: boolean;
  rules: AutoListRuleView[];
}

/** R99 空间耗尽预警（space-full-reminder 负载，逐盘）：days=0 表示已满。 */
export interface SpaceFullWarn {
  path: string;
  days: number;
}

/** scan_staleness_cmd 的返回形状（R293 主动查询）。三个态必须可区分——
 *  never = 从未扫过（说「还没扫过」，不是说「扫了很久」）；
 *  clock_skew = 时间戳晚于当前（时钟被改/回拨，此刻任何天数都不可信）；
 *  days = 距上次扫描天数（null = never 或 clock_skew；0 = 一天内刚扫过）。
 *  与后端 ScanStalenessOut 一一对应；调用侧不必再猜。 */
export interface ScanStaleness {
  /** 该盘从未有过扫描记录（真空态）。 */
  never: boolean;
  /** 记录时间晚于当前时刻（时钟回拨 / 时间戳异常）。 */
  clock_skew: boolean;
  /** 距上次扫描的天数；null = never 或 clock_skew。 */
  days: number | null;
  /** 上次扫描的 unix 秒（展示「上次扫于 X」用）；null = 无有效记录。 */
  last_at: number | null;
}

/** scan_summary_history 的返回条目（R480），与后端 ScanSummary 一一对应：
 *  一次真实扫描的聚合统计（不含路径明细）。 */
export interface ScanSummaryView {
  v: number;
  /** 归一化扫描根（c:/）。 */
  root: string;
  /** 扫描完成时刻（unix 秒）。 */
  at: number;
  duration_ms: number;
  /** mft | walkdir。 */
  mode: string;
  file_count: number;
  dir_count: number;
  total_bytes: number;
  /** 一级子目录体量 Top20（按 bytes 降序）。 */
  top_dirs: { path: string; bytes: number; files: number }[];
  /** 大文件 Top50（按 bytes 降序）。 */
  top_files: { path: string; bytes: number }[];
  /** 体量分档直方图。 */
  buckets: { label: string; count: number; bytes: number }[];
}

/** R293 扫描久未提醒（scan-stale 负载，逐盘）。只通知「多久没扫」，
 *  不推测用户为什么没扫。 */
export interface ScanStaleWarn {
  /** 归一化盘根（c:/）。 */
  path: string;
  /** 距上次扫描天数；从未扫过时为 null。 */
  days: number | null;
  /** true = 该盘从未有过扫描记录。 */
  never: boolean;
}

/** R291 温度持续偏高（thermal-sustained 负载）。只通知，不含任何写路径。 */
export interface ThermalSustained {
  /** 判定阈值（℃）。 */
  threshold_c: number;
  /** 参与判定的采样点数。 */
  samples: number;
  /** 窗口内最高温；null = 读不到（不拿 0℃ 冒充）。 */
  temp_max_c: number | null;
  /** 生成时间（unix 秒）。 */
  ts: number;
}

/** R292 启动项变化（startup-changed / check_startup_delta_cmd 返回形状）。
 *  added/gone 是身份键（location + 单元分隔符 + name）——只说「自上次快照后
 *  新出现/消失」，不说是谁做的。 */
export interface StartupChanged {
  /** 新出现的身份键。 */
  added: string[];
  /** 消失的身份键。 */
  gone: string[];
  /** 距上一份快照的天数（0 = 一天内）。 */
  gap_days: number;
  /** 上一份快照时间（unix 秒）。 */
  prev_at: number;
}

/** R100 逐盘趋势检查结果（check_space_forecast_cmd 返回）。
 *  days = null 时 reason 说明为何预测不了（从实况诚实上报，缺失≠满盘）。 */
export interface SpaceForecast {
  path: string;
  days: number | null;
  /** ok / no_history / not_enough / releasing */
  reason: string;
  /** days ≤ 预警阈值（未设阈值恒 false） */
  warn: boolean;
}

// ── 周巡检简报（health-butler P1，后端 weekly_report.rs 返回形状）──

/** 单份周报：只读健康快照，绝不带任何清理动作。 */
export interface WeeklyReport {
  /** 生成时间（unix 秒） */
  ts: number;
  /** 各盘建议可清合计（只读统计） */
  drives: DriveCleanup[];
  /** 全部盘可清合计 */
  total_bytes: number;
  /** 本轮最高温（无则 null） */
  temp_max_c: number | null;
  /** 本周有归档的硬件快照数 */
  hw_snapshots: number;
  /** 周内硬件变化简要（温度 / 通电差），无则空 */
  hw_delta: string[];
  /** 每块物理盘的健康摘要（带盘符）。旧归档没有这个字段 → 按空处理。 */
  disks?: WeeklyDiskHealth[];
  /** 盘健康读取失败的原因（R86）：非空 = 读失败（不是「没读到」）。 */
  disks_note?: string | null;
}

/** 周报里的单块盘健康：health 是后端原值，grade 是统一分级（good/caution/bad/unknown）。 */
export interface WeeklyDiskHealth {
  friendly: string;
  /** 该盘覆盖的盘符（`C` / `F,G`）；映射不到时为空串。 */
  letters: string;
  health: string;
  grade: string;
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

// ── 温度采样历史（R291 后端 / R299 前端接线，thermal_history.rs 返回形状）──

/** 单条温度快照：`temp_max_c` = 该帧归一化最高温（CPU/GPU/主板取最大，
 *  与 fan_curve_advice().temp_max_c 同源口径）；`at` 为 Unix 秒。
 *  采样周期 5 分钟、最多留 3000 条（约 3 周半）。 */
export interface ThermalPoint {
  temp_max_c: number;
  at: number;
}

// ── 建议清理跨次对比（health-butler P0，后端 suggestion_history.rs 返回形状）──

/** 单个 scaffold 的环比明细（只在两个快照都出现过的 scaffold 才有）。 */
export interface SuggestionScopeDiff {
  scaffold_id: string;
  now_bytes: number;
  prev_bytes: number;
  /** 差值（字节），正=比上次多 */
  delta: number;
}

/** 某盘建议总量的跨次对比。缺字段一律 null / 空数组，不编造差值。 */
export interface SuggestionDiff {
  /** 本次快照时间（unix 秒） */
  at: number;
  /** 上次快照时间；没有则为 0 */
  prev_at: number;
  /** 总量差值：>0 多出，<0 更少，=0 持平。**没有上次 → null**（不假装持平）。 */
  total_delta: number | null;
  /** 与上次间隔天数（环比跨度参照）；无上次或时间异常 → null */
  gap_days: number | null;
  /** 共同出现的 scaffold 环比明细 */
  scaffolds: SuggestionScopeDiff[];
  /** 本次才出现的 scaffold（上次没有）：净新增，不谈涨跌 */
  new_scaffolds: string[];
  /** 上次有、本次没有的 scaffold：已清完或不再命中 */
  gone_scaffolds: string[];
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

// ── Winapp2.ini 社区规则导入（后端 parse_winapp2 返回形状）──

/** 单条可导入的 Winapp2 清理规则（只预览，未落盘） */
export interface Winapp2PreviewItem {
  /** 生成的安全 id（= 后续 installScaffold 的脚本 id） */
  id: string;
  /** 社区规则名（如「Google Chrome Autofill Data *」） */
  name: string;
  /** 风险等级（'low' | 'medium' | 'high'，转换产物恒为 low） */
  risk: string;
  /** 文件清理 scope 数（注册表项不执行，已剥离） */
  scope_count: number;
  /** 可读 scope 说明（lable → glob） */
  scopes: string[];
  /** 转换说明（回收站模式 / 含注册表项提示 / 社区警告） */
  disclaimer: string;
  /** 序列化好的 toml（勾选后直接 installScaffold 入库）；红线被拒项为 null */
  toml: string | null;
  /** 命中红线时的拒绝理由（中文，直接进 UI）；null = 可安全安装 */
  red_line: string | null;
}
