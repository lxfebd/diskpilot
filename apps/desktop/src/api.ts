import { invoke } from '@tauri-apps/api/core';
import { isTauri } from './env';
import { scanApi } from './api/scan';
import { cleanApi } from './api/clean';
import { aiApi } from './api/ai';
import { systemExtApi } from './api/system-ext';
import { agentApi } from './api/agent';
import { mcpApi } from './api/mcp';

/**
 * api.ts —— 前端唯一后端入口（聚合层）。
 *
 * 68 个 Tauri 命令按「树干」域拆到 api/ 子模块，本文件只做聚合与再导出，
 * 保证调用面 `api.scan()` / `import type { HwInfo } from './api'` 完全不变。
 * 域文件：api/scan.ts（扫描） · api/clean.ts（清理/撤销/scaffold）
 *          api/ai.ts（AI 搜索/代理/上下文） · api/system-ext.ts（Steam/硬件/工具墙/插件/系统工具）
 *          api/agent.ts（agent-server MCP 工具集）
 */
export const api = {
  // ── 扫描域 ──
  webviewHeartbeat: () =>
    isTauri ? invoke<void>('webview_heartbeat') : Promise.resolve(),

  ...scanApi,
  ...cleanApi,
  ...aiApi,
  ...systemExtApi,
  ...agentApi,
  ...mcpApi,
};

// ── 类型再导出（保持 `import type { X } from './api'` 不变）──
export type {
  ToolbeltRisk,
  ToolbeltToolSpec,
  ToolbeltStatus,
  ToolbeltArchVariant,
  ToolbeltCatalogItem,
  MarketPlugin,
  ToolPluginMeta,
  ToolbeltCatalogCategory,
  ToolbeltCatalog,
  ToolManifestParam,
  ToolManifestInvocation,
  ToolManifestOutput,
  ToolManifestExample,
  ToolManifest,
  ToolbeltUsage,
  ToolbeltRunReport,
  HwInfo,
  HwDiskHealth,
  HwTestReport,
  HwReportOut,
  HwSnapshotMeta,
  HwCompareOut,
  HwDiskCompare,
  HwFieldDelta,
  PowerPlanOut,
  StartupItem,
  FanCurveAdvice,
  FanProbe,
  FanStatus,
  FanControlResult,
  InstalledDriver,
  DriverUpdate,
  UpdateInfo,
  RegistryPlugin,
  RegistryPluginVersion,
  RegistryListOut,
  RegistryRefreshOut,
  RegistryVerifyOut,
  RegistryInstallOut,
  InstalledPlugin,
  LedgerEntry,
} from './api/system-ext';
export type { ChatScanContext, WebSearchHit } from './api/ai';
export type { DupFile, DupGroup, ExecuteScopeOpts, ReminderConfig, DriveCleanup, ReminderPayload, SpacePoint, RegistryScaffold, ScaffoldRegistryOut } from './api/clean';
export type { AgentToolMeta, AgentToolCall } from './api/agent';
export type {
  McpTransportPayload,
  McpServerPayload,
  McpServerConfig,
  McpServerStatus,
  McpToolMeta,
  McpToolCall,
  McpAuditEntry,
} from './api/mcp';

// ── 四级权限模型 ──
/** L0=只读(始终开) L1=受控(每次确认) L2=高危(需授权+每次确认) L3=永久禁止 */
export type PermLevel = 'L0' | 'L1' | 'L2' | 'L3';
/** 本次对话期间免重复确认 */
export type AuthMode = 'single' | 'session';
