// AI 可操作工具集（agent-server MCP）—— 前端接入层。
// agent-server 是独立进程（stdio MCP），后端 agent.rs 负责 spawn + rmcp client，
// 这里只暴露 2 个调用面：列工具清单 / 调工具。

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
  /** 掐断当前所有在飞工具调用（用户点「停止」）。后端 agent_call_tool 的
   *  select 分支收到取消信号立即返回错误，AI 回路随之收尾——此前停止按钮
   *  对已在飞的工具调用无效（挂死 12h 的根因之一）。 */
  cancelToolCalls: () =>
    isTauri
      ? invoke<boolean>('agent_tool_cancel')
      : Promise.resolve(false),
};
