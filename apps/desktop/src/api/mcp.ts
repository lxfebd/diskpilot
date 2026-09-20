// 通用 MCP 服务器（可插拔 MCP 插件）— 前端接入层。
// 后端 mcp.rs 负责：持久化服务器配置 + stdio/http 双桥接 + 7 个命令。
// 前端暴露：服务器 CRUD / 测试连接 / 拉取合并工具清单 / 调用工具。

import { invoke } from '@tauri-apps/api/core';
import { isTauri } from '../env';
import { t } from '../i18n';

export type McpTransportType = 'stdio' | 'http';

export interface McpTransportPayload {
  type: McpTransportType;
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string | null;
  url?: string;
  headers?: Record<string, string>;
  timeout_secs?: number | null;
}

export interface McpServerPayload {
  name: string;
  enabled?: boolean;
  writable?: boolean;
  tool_prefix?: string | null;
  /** 工具级权限映射（质量安全，2026-09-11）：工具名 → L0/L1/L2/L3 或具体 perm id。 */
  permission_map?: Record<string, string> | null;
  transport: McpTransportPayload;
}

export interface McpServerConfig {
  id: string;
  name: string;
  enabled: boolean;
  writable: boolean;
  tool_prefix?: string | null;
  permission_map?: Record<string, string> | null;
  transport: McpTransportPayload;
}

export interface McpServerStatus {
  id: string;
  name: string;
  enabled: boolean;
  writable: boolean;
  tool_prefix?: string | null;
  transport_type: McpTransportType;
  /** 完整传输配置（复用编辑/开关，后端把 config.transport 原样回传）。 */
  transport: McpTransportPayload;
  tool_count: number | null;
  error: string | null;
  /** 权限映射摘要（形如 `{n 个 L2 / m 个 L0}`）。 */
  perm_scope: string;
  /** 完整权限映射（编辑表单预填用）。 */
  permission_map?: Record<string, string> | null;
}

export interface McpToolMeta {
  server_id: string;
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
  writable: boolean;
  /** 工具最终权限级（L0/L1/L2/L3 或具体 perm id）。 */
  perm: string;
}

export interface McpToolCall {
  is_error: boolean;
  text: string;
}

/** 审计日志条目（mcp_audit_tail）。 */
export interface McpAuditEntry {
  ts: number;
  op: string;
  server_id: string;
  tool?: string;
  perm?: string;
  granted?: boolean;
  args?: unknown;
  ok?: boolean;
  error?: string | null;
  server_name?: string;
  writable?: boolean;
}

export const mcpApi = {
  /** 服务器列表 + 每个 enabled 服务器的健康探测。 */
  mcpListServers: () =>
    isTauri ? invoke<McpServerStatus[]>('mcp_list_servers') : Promise.resolve([]),
  /** 测试单个服务器：连接 + tools/list，返回工具清单。 */
  mcpTestServer: (id: string) =>
    isTauri ? invoke<McpToolMeta[]>('mcp_test_server', { id }) : Promise.reject(new Error(t('mcp.desktopOnly'))),
  /** 添加 MCP 服务器（L2 写）。confirmed 必须由调用方显式传 true——后端 mcp_add_server 硬校验。 */
  mcpAddServer: (payload: McpServerPayload, confirmed: boolean) =>
    isTauri
      ? invoke<McpServerConfig>('mcp_add_server', { payload, confirmed: confirmed || null })
      : Promise.reject(new Error(t('mcp.desktopOnly'))),
  /** 编辑 MCP 服务器（L2 写）。 */
  mcpUpdateServer: (id: string, payload: McpServerPayload, confirmed: boolean) =>
    isTauri
      ? invoke<McpServerConfig>('mcp_update_server', { id, payload, confirmed: confirmed || null })
      : Promise.reject(new Error(t('mcp.desktopOnly'))),
  /** 删除 MCP 服务器（L2 写，只删配置）。 */
  mcpRemoveServer: (id: string, confirmed: boolean) =>
    isTauri
      ? invoke<void>('mcp_remove_server', { id, confirmed: confirmed || null })
      : Promise.reject(new Error(t('mcp.desktopOnly'))),
  /** 所有 enabled 服务器工具合并清单（供 refreshMcpTools 拉取）。 */
  mcpListTools: () =>
    isTauri ? invoke<McpToolMeta[]>('mcp_list_tools') : Promise.resolve([]),
  /** 调用某个 MCP 服务器上的工具（权限映射判定：L0 放行 / L1 确认 / L2 确认+权限 / L3 永禁）。
   *  confirmed 只能来自调用方的确认门（advisor 的 promptSessionConfirm），不许默认 true。 */
  mcpCallTool: (serverId: string, name: string, args: Record<string, unknown>, confirmed: boolean) =>
    isTauri
      ? invoke<McpToolCall>('mcp_call_tool', {
          serverId,
          name,
          arguments: args,
          confirmed: confirmed || null,
        })
      : Promise.reject(new Error(t('mcp.desktopOnlyCall'))),
  /** 审计日志尾部（只读 L0）：返回最近 N 条 call_tool / 服务器增删改记录。 */
  mcpAuditTail: (limit = 50) =>
    isTauri
      ? invoke<McpAuditEntry[]>('mcp_audit_tail', { limit })
      : Promise.resolve([]),
};