// AI 可操作工具集（agent-server MCP）—— 前端接入层。
// agent-server 是独立进程（stdio MCP），后端 agent.rs 负责 spawn + rmcp client，
// 这里只暴露 3 个调用面：列工具清单 / 调工具 / 探活。

import { invoke } from '@tauri-apps/api/core';
import { isTauri } from '../env';
import { t } from '../i18n';

export interface AgentToolMeta {
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
  /** 是否写操作（需 confirmed=true；后端 WRITE_TOOLS 单源真值）。 */
  writable: boolean;
}

export interface AgentToolCall {
  is_error: boolean;
  text: string;
}

export const agentApi = {
  /** 拉取 agent-server 的全部 MCP 工具清单（writable 标注只读/写，注册进工具墙/AI 工具表）。 */
  listTools: () =>
    isTauri ? invoke<AgentToolMeta[]>('agent_list_tools') : Promise.resolve([]),
  /** 调用单个 MCP 工具，返回文本结果（isError 时文本含拒绝/错误原因）。
   *  confirmed=true 用于写工具（process_kill / service_control）——后端会硬校验。 */
  callTool: (name: string, args: Record<string, unknown>, confirmed?: boolean) =>
    isTauri
      ? invoke<AgentToolCall>('agent_call_tool', { name, arguments: args, confirmed })
      : Promise.reject(new Error(t('errors.agentDesktopOnly'))),
  /** 探活：agent-server.exe 是否已构建/可定位（前端据此显示「工具集未就绪」提示）。 */
  ready: () => (isTauri ? invoke<boolean>('agent_server_ready') : Promise.resolve(false)),
};
