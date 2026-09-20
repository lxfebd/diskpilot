// Browser-side AI advisor client（W5 拆分后的 re-export barrel）。
//
// 原 1360 行单文件拆成 advisor/ 下三个模块，依赖方向 provider ← tools ← agent：
//   provider.ts — 设置持久化 + 各 provider HTTP 协议 + 单次对话（freeChat/overviewChat）
//   tools.ts    — agent 工具层：23 工具 execTool 分发、tool definitions、待确认中间态
//   agent.ts    — 多轮 agentChat 循环（openai/anthropic/gemini 三种协议）
//
// 现有 import 全部继续从本文件取，零改动。

export * from './advisor/provider';
export * from './advisor/tools';
export * from './advisor/agent';
