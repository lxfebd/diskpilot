import { invoke } from '@tauri-apps/api/core';
import type { SteamInventory, WorkshopItem, SystemProbe, NetworkProbe } from '../types';
import { isTauri } from '../env';
import * as mocks from '../mocks';
import { t } from '../i18n';
import { common } from '../i18n/namespaces/common';
import { isPermEnabled } from '../permissions';

// 插件写操作的前置权限闸：plugin.manage 未开启时直接拒绝，与后端
// `plugin:denied` 同口径——用户先在权限中心开权限，而不是点按钮后收到
// 后端 400。只读查询（list/search/verify/market）不经过这道闸。
function requirePluginManage<T>(run: () => Promise<T>): Promise<T> {
  if (!isPermEnabled('plugin.manage')) {
    return Promise.reject(new Error(t('errors.pluginManageMissing')));
  }
  return run();
}

// 插件写操作的第二道闸：`confirmed` 必须显式为 true。后端每个 plugin_* 命令
// 都以 `plugin:confirm:` 硬校验，这里在前端同口径先拦——确认状态只能来自
// 调用方的两步确认弹窗 / 权限确认门（AI 通道的 promptSessionConfirm），
// 任何 API 函数都不许把 true 烤死在实现里，否则确认门等于不存在。
function requireConfirmed(confirmed: boolean): Promise<void> {
  return confirmed
    ? Promise.resolve()
    : Promise.reject(new Error(t('errors.confirmPluginWrite')));
}

/**
 * 扩展域：Steam / 硬件压测 / 工具墙（图吧 + 插件）/ 更新 / 系统工具。
 * 这一域承载「生态」部分：市场 / 启停 / 导出 / 白名单 CLI 执行。
 * 由 api.ts 聚合进 `api` 对象，保持 `api.toolbeltRun()` 等调用面不变。
 */
export const systemExtApi = {
  // ── Steam ──
  listSteamGames: () =>
    isTauri
      ? invoke<SteamInventory>('list_steam_games')
      : Promise.resolve(mocks.STEAM_INVENTORY),

  listSteamWorkshopItems: (libraryRoot: string, appid: number) =>
    isTauri
      ? invoke<WorkshopItem[]>('list_steam_workshop_items', { libraryRoot, appid })
      : Promise.resolve(mocks.steamWorkshopItems(appid)),

  fetchWorkshopTitles: (ids: number[]) =>
    isTauri
      ? invoke<Record<number, string>>('fetch_workshop_titles', { ids })
      : Promise.resolve(mocks.workshopTitles(ids)),

  /**
   * 读取本地翻译 cache 中已缓存的中文名（幂等，纯读）。未命中的 appid
   * 被排除，由组件再调 translateSteamNamesFetch 进入网络 fetch 路径。
   */
  translateSteamNames: (appids: number[]) =>
    isTauri
      ? invoke<Record<number, string>>('translate_steam_names', { appids })
      : Promise.resolve({}),

  /**
   * 对未命中 cache 的 appid 逐个查 Steam Storefront API（简体中文），
   * 成功写回本地永久 cache。网络不可达时静默返回 {}（设计稿 §7.3）。
   */
  translateSteamNamesFetch: (appids: number[]) =>
    isTauri
      ? invoke<Record<number, string>>('translate_steam_names_fetch', { appids })
      : Promise.resolve({}),

  openSteamUrl: (
    action: 'uninstall' | 'rungameid' | 'validate' | 'nav' | 'workshop_page',
    appid: number,
  ) =>
    isTauri ? invoke<void>('open_steam_url', { action, appid }) : Promise.resolve(),

  // ── 系统探测 ──
  runSystemProbe: (sample = false) =>
    isTauri
      ? invoke<SystemProbe>('run_system_probe', { sample })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'run_system_probe' }))),

  // 网络检测：走后端 Rust 真实 TCP 连接，不受 WebView CSP 限制
  networkProbe: (targets: string[]) =>
    isTauri
      ? invoke<NetworkProbe[]>('network_probe', { targets })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'network_probe' }))),

  // ── 图吧工具箱 CLI 接入 ──
  toolbeltStatus: () =>
    isTauri
      ? invoke<ToolbeltStatus>('toolbelt_status')
      : Promise.resolve<ToolbeltStatus>({ tools_root: null, tools: [] }),

  // 缝口 A：全目录扫描（主面板工具墙，含全部图形工具）
  toolbeltCatalog: () =>
    isTauri
      ? invoke<ToolbeltCatalog>('toolbelt_catalog')
      : Promise.resolve<ToolbeltCatalog>(mocks.TOOLBELT_CATALOG as ToolbeltCatalog),

  toolbeltUsage: (tool: string) =>
    isTauri
      ? invoke<ToolbeltUsage>('toolbelt_usage', { tool })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'toolbelt_usage' }))),

  toolbeltRun: (tool: string, args: string[], timeoutSecs?: number, confirmed?: boolean) =>
    isTauri
      ? invoke<ToolbeltRunReport>('toolbelt_run', {
          tool,
          args,
          timeoutSecs: timeoutSecs ?? null,
          confirmed: confirmed ?? null,
        })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'toolbelt_run' }))),

  /** 后端 toolbelt_run 的中/高风险确认门错误判定：以稳定前缀 `toolbelt:confirm:`
   * 区分「需要用户确认」与其他失败，不再依赖错误文案关键词匹配。 */
  isToolbeltConfirmError: (e: unknown): boolean =>
    typeof e === 'string' && e.startsWith('toolbelt:confirm:'),

  /** 后端 scan_path 取消错误判定：以稳定前缀 `scan:cancelled:` 区分「用户取消
   * 扫描」与真实失败，不再依赖错误文案关键词匹配。 */
  isScanCancelledError: (e: unknown): boolean =>
    typeof e === 'string' && e.startsWith('scan:cancelled:'),

  // 全部 Tool Manifest（能力说明书列表，前端详情卡 + AI 路由共用）
  toolbeltManifests: () =>
    isTauri
      ? invoke<ToolManifest[]>('toolbelt_manifests')
      : Promise.resolve<ToolManifest[]>([]),

  // 双击工具墙 / 详情卡「启动」：GUI 工具 shell 直接拉起，不等退出
  toolbeltLaunch: (tool: string) =>
    isTauri
      ? invoke<void>('toolbelt_launch', { tool })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'toolbelt_launch' }))),

  // 工具墙「移除」：把工具目录移入系统回收站（可恢复，可在「最近清理/撤销」恢复）。
  // confirmed 由调用方在两步确认弹窗（recycleAsk）通过后显式传 true——后端
  // toolbelt_recycle 同口径硬校验，false 直接 400。
  toolbeltRecycle: (tool: string, confirmed: boolean) => {
    if (!confirmed) {
      return Promise.reject(new Error(t('errors.confirmToolbeltRecycle')));
    }
    return isTauri
      ? invoke<{ name: string; dir_rel: string; recycled: string[] }>('toolbelt_recycle', { tool, confirmed })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'toolbelt_recycle' })));
  },

  // 检查更新：查 GitHub Releases 最新版，返回本地/远端版本 + 是否有新版
  checkUpdate: () =>
    isTauri
      ? invoke<UpdateInfo>('check_update')
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'check_update' }))),

  // 插件管理：卸载（回收站可恢复）/ 安装（本地 .zip 插件包）
  // 写操作需 confirmed=true（前端插件确认卡通过后才置位）。权限前置：
  // plugin.manage 未开启时直接拒绝（与后端 plugin:denied 同口径），
  // 不把「点按钮 → 后端 400」留给用户。
  pluginUninstall: (id: string, confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<{ id: string; name: string; dir_rel: string; recycled: string[] }>('plugin_uninstall', { id, confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_uninstall' }))),
      ),
    ),

  pluginInstall: (zipPath: string, confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<string>('plugin_install', { zipPath, confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_install' }))),
      ),
    ),

  // 远程安装：从 https URL 下载插件 zip（B2，2026-09-09）。
  // 与本地安装同为 plugin.manage 权限；后端强制 https + Ed25519 签名，
  // 下载进度走 plugin-download-progress 事件（Toolbelt.tsx 监听）。
  pluginInstallUrl: (url: string, confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<string>('plugin_install_url', { url, confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_install_url' }))),
      ),
    ),

  // 社区注册表：列表 / 搜索 / 一键安装（B3，2026-09-09）。
  // 列表与搜索只读 L0；安装复用 plugin.manage（后端会再校验 confirmed + 权限）。
  pluginRegistryList: () =>
    isTauri
      ? invoke<RegistryListOut>('plugin_registry_list')
      : Promise.resolve(EMPTY_REGISTRY),
  pluginRegistrySearch: (q: string) =>
    isTauri
      ? invoke<RegistryListOut>('plugin_registry_search', { q })
      : Promise.resolve({ ...EMPTY_REGISTRY, query: q }),
  pluginRegistryInstall: (id: string, confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<string>('plugin_registry_install', { id, confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_registry_install' }))),
      ),
    ),

  // 插件市场远端分发完整版（2026-09-11）：索引 URL 配置 + 刷新 + 校验 + 台账。
  // 写操作（set url / refresh / install / update / rollback）都需 confirmed=true，
  // 后端再做 plugin.manage 权限校验。索引 URL 默认留空 =「未配置索引」。
  pluginRegistryConfig: () =>
    isTauri
      ? invoke<{ registry_url: string }>('plugin_registry_config')
      : Promise.resolve({ registry_url: '' }),
  pluginSetRegistryUrl: (url: string, confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<{ registry_url: string }>('plugin_set_registry_url', { url, confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_set_registry_url' }))),
      ),
    ),
  pluginRegistryRefresh: (confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<RegistryRefreshOut>('plugin_registry_refresh', { confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_registry_refresh' }))),
      ),
    ),
  pluginRegistryVerify: () =>
    isTauri
      ? invoke<RegistryVerifyOut>('plugin_registry_verify')
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_registry_verify' }))),
  pluginListInstalled: () =>
    isTauri
      ? invoke<{ installed: InstalledPlugin[]; total_events: number }>('plugin_list_installed')
      : Promise.resolve({ installed: [], total_events: 0 }),
  pluginRegistryInstallVersion: (id: string, version: string | undefined, confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<RegistryInstallOut>('plugin_registry_install', { id, version: version ?? null, confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_registry_install' }))),
      ),
    ),
  pluginRegistryUpdate: (id: string, version: string | undefined, confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<RegistryInstallOut>('plugin_registry_update', { id, version: version ?? null, confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_registry_update' }))),
      ),
    ),
  pluginRegistryRollback: (id: string, version: string | undefined, confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<RegistryInstallOut>('plugin_registry_rollback', { id, version: version ?? null, confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_registry_rollback' }))),
      ),
    ),

  // 插件市场：内置 CLI 工具索引（哪些已插件化）+ 激活/摘除（补写/移除 tool.plugin.json）
  pluginMarket: () =>
    isTauri
      ? invoke<MarketPlugin[]>('plugin_market')
      : Promise.resolve<MarketPlugin[]>([]),
  pluginActivate: (tool: string, confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<string>('plugin_activate', { tool, confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_activate' }))),
      ),
    ),
  pluginDeactivate: (tool: string, confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<string>('plugin_deactivate', { tool, confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_deactivate' }))),
      ),
    ),

  // 插件分发：把已安装插件（目录含 tool.plugin.json）打包成可分发 zip。
  // outZip 为保存目标路径（前端 save 对话框选定）；已存在时覆盖。
  // 写操作需 confirmed=true。
  pluginExport: (id: string, outZip: string, confirmed: boolean) =>
    requirePluginManage(() =>
      requireConfirmed(confirmed).then(() =>
        isTauri
          ? invoke<{ id: string; name: string; dir_rel: string; zip: string }>('plugin_export', { id, outZip, confirmed })
          : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'plugin_export' }))),
      ),
    ),

  // ── 硬件检测与受控压测（L0 只读 / L1 受控 / L2 授权）──
  // refresh=true 时后端无视 5 分钟 TTL 强制重跑 PowerShell（刷新按钮）。
  hwInfo: (refresh = false) =>
    isTauri
      ? invoke<HwInfo>('hw_info', { refresh: refresh || null })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'hw_info' }))),
  hwDiskHealth: (refresh = false) =>
    isTauri
      ? invoke<HwDiskHealth>('hw_disk_health', { refresh: refresh || null })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'hw_disk_health' }))),
  hwRunTest: (
    testType: string,
    durationSeconds?: number,
    tempLimitCelsius?: number,
    confirmed?: boolean,
  ) =>
    isTauri
      ? invoke<HwTestReport>('hw_run_test', {
          testType,
          durationSeconds: durationSeconds ?? null,
          tempLimitCelsius: tempLimitCelsius ?? null,
          confirmed: confirmed ?? null,
        })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'hw_run_test' }))),
  hwStopTest: () =>
    isTauri
      ? invoke<void>('hw_stop_test')
      : Promise.resolve(),
  hwReport: (format?: string, savePath?: string) =>
    isTauri
      ? invoke<HwReportOut>('hw_report', { format: format ?? null, savePath: savePath ?? null })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'hw_report' }))),
  hwHistory: () =>
    isTauri
      ? invoke<HwSnapshotMeta[]>('get_hw_history')
      : Promise.resolve([]),
  compareHwSnapshots: (archiveA?: string, archiveB?: string) =>
    isTauri
      ? invoke<HwCompareOut>('compare_hw_snapshots', {
          archiveA: archiveA ?? null,
          archiveB: archiveB ?? null,
        })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'compare_hw_snapshots' }))),
  powerPlan: (setScheme?: string, confirmed?: boolean) =>
    isTauri
      ? invoke<PowerPlanOut>('power_plan', { setScheme: setScheme ?? null, confirmed: confirmed ?? null })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'power_plan' }))),

  // ── 系统工具（L2：启动项 / 风扇建议 / 驱动检查）──
  // 启动项：枚举（只读）/ 启停（禁用=改名 .disabled 备份，可恢复）/ 删除（进回收站）
  listStartupItems: () =>
    isTauri
      ? invoke<StartupItem[]>('list_startup_items')
      : Promise.resolve([]),
  setStartupItem: (id: string, enable: boolean, confirmed: boolean) => {
    if (!confirmed) {
      return Promise.reject(new Error(t('errors.confirmSystemWrite')));
    }
    return isTauri
      ? invoke<void>('set_startup_item', { id, enable, confirmed })
      : Promise.resolve();
  },
  removeStartupItem: (id: string, confirmed: boolean) => {
    if (!confirmed) {
      return Promise.reject(new Error(t('errors.confirmSystemWrite')));
    }
    return isTauri
      ? invoke<void>('remove_startup_item', { id, confirmed })
      : Promise.resolve();
  },

  // 风扇曲线建议：读温度/负载传感器 + 风扇信息，生成建议档位与曲线锚点（只建议，不写 EC）
  fanCurveAdvice: (refresh = false) =>
    isTauri
      ? invoke<FanCurveAdvice>('fan_curve_advice', { refresh: refresh || null })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'fan_curve_advice' }))),

  // 风扇控制（L2，fan.control）：档位/自定义曲线落地调速 + 只读状态 + 恢复默认
  fanControl: (level: string, curve: { load_pct: number; fan_pct: number }[] | undefined, tempBreakerC: number | undefined, confirmed: boolean) => {
    if (!confirmed) {
      return Promise.reject(new Error(t('errors.confirmSystemWrite')));
    }
    return isTauri
      ? invoke<FanControlResult>('fan_control', {
          level: level || null,
          curve: curve && curve.length > 0 ? curve : null,
          tempBreakerC: tempBreakerC ?? null,
          confirmed,
        })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'fan_control' })));
  },
  fanStatus: () =>
    isTauri
      ? invoke<FanStatus>('fan_status')
      : Promise.resolve({
          probe: {
            channel: 'wmi',
            writable: false,
            mode: 'readonly',
            // mock 冒充后端返回的探测说明：与真后端同口径，恒取中文（不译清单）
            reason: common.zh['common.mock.fanReadonlyChannel'],
            cli: undefined,
            fans: [],
          },
          temp_c: null,
          writable: false,
          has_snapshot: false,
          guard_running: false,
          note: '',
        }),
  fanCurveApply: (restoreDefault: boolean, confirmed: boolean) => {
    if (!confirmed) {
      return Promise.reject(new Error(t('errors.confirmSystemWrite')));
    }
    return isTauri
      ? invoke<FanControlResult>('fan_curve_apply', { restoreDefault: restoreDefault || null, confirmed })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'fan_curve_apply' })));
  },

  // 驱动检查：已装驱动（CIM，秒回）/ Windows Update 可选驱动更新（联网，需 L2 授权）
  listInstalledDrivers: () =>
    isTauri
      ? invoke<InstalledDriver[]>('list_installed_drivers')
      : Promise.resolve([]),
  checkDriverUpdates: () =>
    isTauri
      ? invoke<DriverUpdate[]>('check_driver_updates')
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'check_driver_updates' }))),

  setToolsRoot: (path: string) =>
    isTauri
      ? invoke<void>('set_tools_root', { path })
      : Promise.resolve(),

  // ── 通用配置 ──
  generalConfig: () =>
    isTauri
      ? invoke<{ hardware_accel: boolean }>('general_config')
      : Promise.resolve({ hardware_accel: true }),

  setGeneral: (hardwareAccel: boolean) =>
    isTauri
      ? invoke<void>('set_general', { hardwareAccel })
      : Promise.resolve(),
};

// ── 工具墙类型 ──
export type ToolbeltRisk = 'low' | 'medium' | 'high';

export interface ToolbeltToolSpec {
  name: string;
  category: string;
  description: string;
  exe_rel: string | null;
  risk: ToolbeltRisk;
  timeout_secs: number;
  installed: boolean;
}

export interface ToolbeltStatus {
  tools_root: string | null;
  tools: ToolbeltToolSpec[];
}

// ── 缝口 A：全目录扫描（主面板工具墙）──
export interface ToolbeltArchVariant {
  name: string;
  file_rel: string;
  arch: string;
}

export interface ToolbeltCatalogItem {
  name: string;
  category: string;
  dir_rel: string;
  exe_rel: string | null;
  extension: string;
  /** 真实 exe 图标（data:image/png;base64,...），无则为 null → 回退 Lucide。 */
  icon: string | null;
  description: string;
  tags: string[];
  is_linked: boolean;
  is_builtin_link: boolean;
  arch: string;
  arch_variants: ToolbeltArchVariant[];
  linked_from: string[];
  risk: string;
  launch_target: string | null;
  publisher: string | null;
  version: string | null;
  tutorial_url: string | null;
  download_url: string | null;
  /** 纯推广跳转项（流量卡/加速器返利链 .bat）：前端灰化 + 「推广」徽标 + 一键隐藏。 */
  is_promotion: boolean;
  /** 目录内 tool.plugin.json 的插件元数据；无则 null/缺省（传统工具）。 */
  plugin?: ToolPluginMeta | null;
}

export interface MarketPlugin {
  id: string;
  name: string;
  category: string;
  purpose: string;
  risk: string;
  permission_level: string;
  publisher: string;
  exe_rel: string;
  installed: boolean;
}

/** 社区注册表条目（B3 + 远端分发完整版）：id 是合法 kebab-case，url 为 https 下载地址 */
export interface RegistryPlugin {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  category: string;
  risk: string;
  tags: string[];
  url: string;
  sha256: string;
  signer: string;
  downloads: number;
  /** 远端分发完整版（2026-09-11）字段，serde default 向后兼容： */
  versions?: RegistryPluginVersion[];
  license?: string;
  depends_on?: string[];
  verified?: boolean;
  homepage?: string;
  /** 后端附加标注（annotate_plugins）：与内置工具重复 → true。 */
  builtin?: boolean;
  /** 后端附加标注：已在本地安装（台账命中）→ true。 */
  installed?: boolean;
}

/** 单个版本的下载元信息（resolve 后取最新）。 */
export interface RegistryPluginVersion {
  version: string;
  url: string;
  sha256: string;
  signer: string;
  min_app?: string;
  size_hint?: number;
  notes?: string;
}

/** plugin_registry_list 返回（信封 + 条目 + 索引配置状态）。 */
export interface RegistryListOut {
  items: RegistryPlugin[];
  /** 与内置工具重复而被剔除的条目数（后端 dedup，前端展示提示）。 */
  skipped_builtin?: number;
  warning: string | null;
  updated_at: number;
  schema: number;
  source_url: string;
  fetched_at: number;
  signed: boolean;
  verified: boolean;
  signer: string;
  registry_url: string;
  indexed: boolean;
}

/** plugin_registry_refresh 返回（刷新结果 + 信封状态）。 */
export interface RegistryRefreshOut {
  ok: boolean;
  error: string | null;
  registry_url: string;
  fetched_at: number;
  plugins: number;
  signed: boolean;
  verified: boolean;
  warning: string | null;
}

/** plugin_registry_verify 返回（签名/字段完备性逐条检查）。 */
export interface RegistryVerifyOut {
  signed: boolean;
  envelope_ok: boolean;
  envelope_error: string | null;
  schema: number;
  fetched_at: number;
  items: {
    id: string;
    name: string;
    version: string;
    has_url: boolean;
    https_url: boolean;
    has_sha256: boolean;
    has_signer: boolean;
    verified: boolean;
    issue: string | null;
  }[];
  warning: string | null;
}

/** plugin_registry_install / update / rollback 返回。 */
export interface RegistryInstallOut {
  id: string;
  name: string;
  version: string;
  dir_rel: string;
  cached: boolean;
  blob_hash: string;
}

/** 安装台账条目（plugin_list_installed）。 */
export interface InstalledPlugin {
  id: string;
  name: string;
  version: string;
  source: string;
  dir_rel: string;
  installed_at: number;
  signer: string;
  blob: string;
  blob_hash: string;
  history: LedgerEntry[];
}

/** 台账事件（历史/回滚目标）。 */
export interface LedgerEntry {
  ts: number;
  kind: string;
  id: string;
  name: string;
  version: string;
  source: string;
  dir_rel: string;
  blob: string;
  blob_hash: string;
  signer: string;
  from_version: string;
}

const EMPTY_REGISTRY: RegistryListOut = {
  items: [],
  warning: null,
  updated_at: 0,
  schema: 0,
  source_url: '',
  fetched_at: 0,
  signed: false,
  verified: false,
  signer: '',
  registry_url: '',
  indexed: false,
};

export interface ToolPluginMeta {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  entry: string;
  category: string;
  risk: string;
  tags: string[];
  permissions: string[];
  web: { homepage: string | null; download: string | null } | null;
  schema: number;
}

export interface ToolbeltCatalogCategory {
  name: string;
  tools: ToolbeltCatalogItem[];
}

export interface ToolbeltCatalog {
  tools_root: string | null;
  categories: ToolbeltCatalogCategory[];
  total: number;
  category_order: string[];
}

// ── Tool Manifest（能力说明书）── 与 crates/toolbelt/src/manifest.rs 序列化对齐
export interface ToolManifestParam {
  name: string;
  flag: string;
  type: string;
  required: boolean;
  default: string | null;
  desc: string;
}
export interface ToolManifestInvocation {
  mode: 'cli' | 'gui';
  launchable: boolean;
  args_template: string;
  params: ToolManifestParam[];
  timeout_secs: number;
}
export interface ToolManifestOutput {
  format: string;
  parser: string;
  schema: string;
}
export interface ToolManifestExample {
  args: string;
  desc: string;
  expect: string;
}
export interface ToolManifest {
  name: string;
  category: string;
  exe_rel: string;
  purpose: string;
  when_to_use: string;
  when_not_to_use: string;
  invocation: ToolManifestInvocation;
  output: ToolManifestOutput;
  risk: string;
  permission_level: string;
  side_effects: string;
  examples: ToolManifestExample[];
  publisher: string;
  tags: string[];
  tutorial_url: string;
  download_hint: string;
}

export interface ToolbeltUsage {
  tool: string;
  category: string;
  exe: string;
  risk: ToolbeltRisk;
  usage: string;
}

export interface ToolbeltRunReport {
  command_line: string;
  risk: ToolbeltRisk;
  outcome: {
    exit_code: number | null;
    timed_out: boolean;
    stdout: string;
    stderr: string;
  };
}

// ── 硬件检测类型 ──
export interface HwInfo {
  cpu?: Array<Record<string, unknown>>;
  cpu_usage?: Array<Record<string, unknown>>;
  gpu?: Array<Record<string, unknown>>;
  gpu_live?: {
    name?: string;
    utilization_pct?: number;
    temperature_c?: number;
    memory_used_bytes?: number;
    memory_total_bytes?: number;
  };
  memory?: Array<Record<string, unknown>>;
  system?: Array<Record<string, unknown>>;
  bios?: Array<Record<string, unknown>>;
  board?: Array<Record<string, unknown>>;
  os?: Array<Record<string, unknown>>;
  disk?: Array<Record<string, unknown>>;
  thermal?: Array<Record<string, unknown>>;
}

export interface HwDiskHealth {
  disks?: Array<Record<string, unknown>>;
  reliability?: boolean;
  cdi_report?: Array<Record<string, unknown>>;
  note?: string;
}

export interface HwTestReport {
  test_type: string;
  tool: string;
  command_line: string;
  duration_run_secs: number;
  stop_reason: string;
  exit_code: number | null;
  temp_peak_c: number | null;
  temp_monitor_active: boolean;
  bench_excerpt: string | null;
  note: string;
}

export interface HwReportOut {
  format: string;
  markdown: string;
  saved_path: string | null;
  note: string;
}

/** Rust 端 get_hw_history 返回：单份硬件快照归档的元信息（不含正文） */
export interface HwSnapshotMeta {
  file: string;
  at: number;
  machine: string;
  disk_count: number;
}

/** Rust 端 compare_hw_snapshots 返回：两次归档的逐磁盘字段对比 */
export interface HwCompareOut {
  ok: boolean;
  a: string;
  b: string;
  disks: HwDiskCompare[];
  reason: string;
}

export interface HwDiskCompare {
  device: string;
  fields: HwFieldDelta[];
}

export interface HwFieldDelta {
  name: string;
  old: unknown;
  new: unknown;
  delta: string;
}

export interface PowerPlanOut {
  active: string | null;
  schemes: string[];
}

/** Rust 端 list_startup_items 返回：单个启动项 */
export interface StartupItem {
  id: string;
  name: string;
  command: string;
  location: string;
  enabled: boolean;
  target: string;
}

/** Rust 端 fan_curve_advice 返回：建议档位 + 曲线锚点 + 传感器读数 */
export interface FanCurveAdvice {
  level: string;
  level_label: string;
  reason: string;
  temp_max_c: number | null;
  cpu_load_pct: number | null;
  curve: { load_pct: number; fan_pct: number }[];
  fans: Array<Record<string, unknown>>;
  note: string;
}

/** 风扇控制通道探测（Rust fan_probe_all 返回结构子集） */
export interface FanProbe {
  channel: string;
  writable: boolean;
  mode: string;
  reason: string;
  cli?: { name: string; exe: string; exe_path: string; subcmd_tpl: string };
  fans?: Array<Record<string, unknown>>;
}

/** Rust 端 fan_status 返回：探测 + 温度 + 守护状态（只读） */
export interface FanStatus {
  probe: FanProbe;
  temp_c: number | null;
  writable: boolean;
  has_snapshot: boolean;
  guard_running: boolean;
  note: string;
}

/** Rust 端 fan_control / fan_curve_apply 返回：写操作结果 */
export interface FanControlResult {
  ok: boolean;
  applied: boolean;
  channel?: string;
  speed_pct?: number;
  original_speed_pct?: number;
  temp_breaker_c?: number;
  guard_running?: boolean;
  probe?: FanProbe;
  note: string;
}

/** Rust 端 list_installed_drivers 返回：已装驱动条目 */
export interface InstalledDriver {
  name: string;
  provider: string;
  version: string;
  date: string;
  class: string;
}

/** Rust 端 check_driver_updates 返回：Windows Update 可选更新条目 */
export interface DriverUpdate {
  title: string;
  kb: string;
  driver_provider: string;
  driver_version: string;
  category: string;
  is_driver: boolean;
}

/** Rust 端 check_update 返回：本地/远端版本 + 是否有新版 + 下载页 */
export interface UpdateInfo {
  current: string;
  latest: string;
  available: boolean;
  url: string;
  notes: string;
}