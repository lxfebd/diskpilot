// AI agent 的多轮循环（自 advisorClient.ts 拆出）：按 provider 协议
// （OpenAI-like / Ollama / Anthropic / Gemini）反复「模型调用 → 执行工具 →
// 回填结果」，最多 MAX_ROUNDS 轮；结束时把 pending 的清理清单 / CLI 确认卡 /
// 压测确认交给 UI 回调。

import {
  loadSettings,
  isConfigured,
  fetchJson,
  extractAnthropicText,
  dataUrlBase64,
  CHAT_SYSTEM,
  isToolsUnsupportedError,
} from './provider';
import type { AdvisorSettings } from './provider';
import {
  execTool,
  buildToolDefs,
  getCliIndexContext,
  parseProposalFromText,
  resetAgentPending,
  takeProposal,
  peekStress,
  peekSessionConfirm,
  refreshMcpTools,
} from './tools';
import type {
  AgentOpts,
  TraceEvent,
} from './tools';
import { prefs } from '../theme';

const TOOL_LABEL: Record<string, string> = {
  web_search: '联网搜索',
  path_size: '路径大小实测',
  inspect_dir: '列目录',
  propose_cleanup: '生成清理清单',
  propose_cleanup_plan: '生成清理建议清单',
  get_disk_health: '全盘健康读取',
  get_steam_library_summary: 'Steam 库存盘点',
  get_cleanup_suggestions: '清理建议读取',
  get_system_info: '系统信息读取',
  run_system_probe: '系统状态实时采集',
  get_cli_tool_usage: '工具箱工具用法',
  get_tool_manifest: '读取工具能力说明书',
  run_cli_tool: '运行工具箱 CLI 工具',
  get_hardware_info: '读取硬件信息',
  analyze_disk_health: '分析硬盘健康',
  run_hardware_test: '运行硬件压测',
  generate_hw_report: '生成硬件报告',
  adjust_power_plan: '调整电源计划',
  list_plugins: '列出已安装插件',
  market_plugins: '查看插件市场',
  install_plugin: '安装插件',
  uninstall_plugin: '卸载插件',
  activate_plugin: '插件化工具',
  export_plugin: '导出插件',
  // ── agent-server MCP 工具集（39 个，全只读 L0）──
  disk_health: '磁盘空间概览',
  disk_partition_usage: '卷用量查询',
  disk_volume_meta: '分区类型元数据',
  file_type_stats: '扩展名占用统计',
  file_tree: '目录树',
  list_dir: '列目录（MCP）',
  read_file: '读文件内容',
  find_files: '按名查找文件',
  disk_find_biggest_files: '最大文件查询',
  disk_find_duplicate_files: '重复文件查询',
  disk_io_usage: '磁盘 IO 实时率',
  system_info: '系统信息（MCP）',
  list_processes: '进程列表',
  process_info: '进程详情',
  app_list: '已安装程序清单',
  sys_services: '服务列表',
  sys_drivers: '驱动列表',
  sys_boot_items: '开机启动项',
  hw_cpu: 'CPU 信息',
  hw_gpu: '显卡信息',
  hw_memory: '内存信息',
  hw_motherboard: '主板信息',
  hw_temperature: '温度读取',
  hw_sensors: '硬件传感器快照',
  hw_battery: '电池信息',
  hw_disk_smart: '磁盘 SMART 健康',
  net_status: '网络状态',
  net_connections: '网络连接',
  net_speed: '网络速率实测',
  net_share: '共享文件夹',
  net_wifi: 'WiFi 信息',
  security_event_logs: '系统事件日志',
  security_firewall_rules: '防火墙规则',
  security_login_events: '登录事件',
};

function safeParseArgs(raw: unknown): Record<string, unknown> {
  if (typeof raw === 'object' && raw !== null) return raw as Record<string, unknown>;
  if (typeof raw === 'string') {
    try { return JSON.parse(raw) as Record<string, unknown>; } catch { return {}; }
  }
  return {};
}

function summarizeArgs(args: Record<string, unknown>): string {
  return Object.values(args).map((v) => String(v)).join(' · ').slice(0, 200);
}

const MAX_ROUNDS = 6;

export async function agentChat(o: AgentOpts): Promise<string> {
  const settings = loadSettings();
  if (!isConfigured(settings)) throw new Error('AI 未配置 — 在右上角的设置里填一个 API key');
  const showThink = prefs.showThinking;
  // 每轮对话开始刷新用户 MCP 服务器工具（设置页增删后下一次对话即生效）。
  try { await refreshMcpTools(); } catch { /* 拉取失败不阻塞对话 */ }
  const emit = (e: TraceEvent) => { try { o.onEvent?.(e); } catch { /* UI 错误不打断链路 */ } };
  const tools = buildToolDefs();
  resetAgentPending();
  // 有可选 system 则用调用方的；否则用 CHAT_SYSTEM 并在尾部补一段工具箱工具索引，
  // 让模型知道本机有哪些 CLI 工具可用（索引只含分类/名字/简介，用法按需先查后跑）。
  const baseSystem = o.system ?? CHAT_SYSTEM;
  const cliIndex = await getCliIndexContext();
  // 性能上下文注入：让 AI 知道自己跑在多强的机器上、工具执行该多节制。
  // 弱机上明确抑制烤机/压测/深度扫描这类长时高负载操作。
  let perfLine = '';
  try {
    const { perfContextLine } = await import('../hwProfile');
    perfLine = await perfContextLine();
  } catch { /* 非关键路径，拿不到就略过 */ }
  const hwSection = `## 硬件检测能力
你可以读取本机硬件信息与健康状态（只读，零风险，随时可调）：
- get_hardware_info：读 CPU / GPU / 内存 / 主板 / BIOS / 磁盘 / 温度传感器（WMI CIM，无需管理员）
- analyze_disk_health：读硬盘 SMART（通电时间 / 温度 / 磨损 / 读写错误），非管理员权限下部分字段为 null
- generate_hw_report：生成硬件检测报告（markdown / json / html），可选保存到用户指定路径
- run_hardware_test：运行硬件压力/基准测试（🔴 高负载，必须用户在确认面板里点「确认」后才执行）
- adjust_power_plan：调整电源计划（需用户在「AI 权限中心」开启后，每次仍需确认）

### 硬件检测行为准则
1. 读取类操作（get_hardware_info / analyze_disk_health / generate_hw_report）可随时执行，无需询问用户。
2. run_hardware_test 必须先告知用户风险与注意事项，等用户在确认面板中确认后才可执行；用户取消就别再试。
3. 发现硬件异常时主动给出建议（硬盘寿命低 → 建议备份；温度过高 → 建议清灰/检查散热）。
4. 不要编造硬件数据，所有数值必须来自工具返回值。
5. 弱机（hwProfile=weak）上，不要主动建议烤机/压测/深度扫描这类长时高负载操作。`;
  const system = cliIndex
    ? `$${baseSystem}\n\n$${perfLine}\n\n$${hwSection}\n\n$${cliIndex}\n\n工具使用规则：要用 run_cli_tool 执行某个 CLI 工具前，必须先调用 get_cli_tool_usage 查看该工具的绝对路径、参数表和示例，确认参数无误后才能调用 run_cli_tool；中/高风险工具会自动弹确认框，确认后才真正执行。`
    : `$${baseSystem}\n\n$${perfLine}\n\n$${hwSection}`;

  let answer: string;
  if (settings.provider === 'anthropic') answer = await agentAnthropic(settings, system, o, tools.anthropic, emit, showThink);
  else if (settings.provider === 'gemini') answer = await agentGemini(settings, system, o, tools.gemini, emit, showThink);
  else if (settings.provider === 'ollama') answer = await agentOpenaiLike(settings, system, o, tools.openai, emit, showThink, true);
  else answer = await agentOpenaiLike(settings, system, o, tools.openai, emit, showThink, false);

  // 优先用工具调用产生的清单；没有则尝试从正文兜底解析（模型没走 tools 协议时）。
  const proposal = takeProposal() ?? parseProposalFromText(answer);
  if (proposal) { try { o.onProposal?.(proposal); } catch { /* ignore */ } }
  // CLI 工具确认已改为挂起模式（与其他待确认项一致）：run_cli_tool 命中确认门
  // 时在 execCliTool 内部弹卡并等待 resolveCliTool()，这里不再取走。
  // 待确认的硬件压测：交给调用方（UI）弹确认面板，确认后由 UI 回调 resolveStress()
  // 让 agentChat 的工具调用链继续往下走。
  const st = peekStress();
  if (st) { try { o.onStressConfirm?.(st); } catch { /* ignore */ } }
  // 待确认的 L2 会话操作（插件管理 / 电源计划等）：交给调用方（UI）弹确认卡，
  // 确认后由 UI 回调 resolveSessionConfirm() 让工具调用链继续。
  const pc = peekSessionConfirm();
  if (pc) { try { o.onSessionConfirm?.(pc); } catch { /* ignore */ } }
  return answer;
}

async function agentOpenaiLike(
  s: AdvisorSettings,
  system: string,
  o: AgentOpts,
  toolDefs: unknown[],
  emit: (e: TraceEvent) => void,
  showThink: boolean,
  ollama: boolean,
): Promise<string> {
  const base = (s.baseUrl || (ollama ? 'http://localhost:11434' : 'https://api.openai.com/v1')).replace(/\/$/, '');
  const url = ollama ? `${base}/api/chat` : `${base}/chat/completions`;
  const headers: Record<string, string> = ollama
    ? { 'Content-Type': 'application/json' }
    : { 'Content-Type': 'application/json', Authorization: `Bearer ${s.apiKey}` };

  const userContent: unknown = (o.images?.length ?? 0) > 0 && !ollama
    ? [{ type: 'text', text: o.user }, ...o.images!.map((img) => ({ type: 'image_url', image_url: { url: img.dataUrl } }))]
    : o.user;
  const msgs: any[] = [
    { role: 'system', content: system },
    ...(o.history ?? []).map((h) => ({ role: h.role, content: h.content })),
    ollama && (o.images?.length ?? 0) > 0
      ? { role: 'user', content: o.user, images: o.images!.map((i) => dataUrlBase64(i.dataUrl)) }
      : { role: 'user', content: userContent },
  ];

  let usedTools = toolDefs.length > 0;
  for (let round = 0; round < MAX_ROUNDS; round++) {
    const body: any = { model: s.model, messages: msgs };
    if (ollama) body.stream = false;
    if (usedTools) body.tools = toolDefs;
    let data: any;
    try {
      data = await fetchJson(url, { method: 'POST', headers, body: JSON.stringify(body), signal: o.signal });
    } catch (e) {
      if (usedTools && isToolsUnsupportedError(e)) {
        emit({ kind: 'note', text: '该模型不支持工具调用，已退回直接回答' });
        usedTools = false;
        continue;
      }
      throw e;
    }
    const msg = data?.message ?? data?.choices?.[0]?.message ?? {};
    const reasoning = typeof msg.reasoning_content === 'string' ? msg.reasoning_content : typeof msg.reasoning === 'string' ? msg.reasoning : '';
    if (reasoning && showThink) emit({ kind: 'thinking', text: reasoning.slice(0, 2000) });
    const calls: any[] = msg.tool_calls ?? [];
    if (calls.length === 0) return String(msg.content ?? '').trim();
    msgs.push(ollama ? { role: 'assistant', content: msg.content ?? '', tool_calls: calls } : msg);
    for (const c of calls) {
      const name = c?.function?.name ?? '';
      const args = safeParseArgs(c?.function?.arguments);
      const label = TOOL_LABEL[name] ?? name;
      emit({ kind: 'tool_call', text: `调用 ${label}`, detail: summarizeArgs(args) });
      const out = await execTool(name, args, o);
      emit({ kind: 'tool_result', text: `${label} 返回`, detail: out.slice(0, 1500) });
      msgs.push({ role: 'tool', tool_call_id: c.id, content: out });
    }
  }
  emit({ kind: 'note', text: '工具轮数用满，基于已有结果直接总结' });
  const finalBody: any = { model: s.model, messages: [...msgs, { role: 'user', content: '请基于以上工具结果直接给出最终回答，不要再调用工具。' }] };
  if (ollama) finalBody.stream = false;
  const data = await fetchJson(url, { method: 'POST', headers, body: JSON.stringify(finalBody), signal: o.signal });
  const msg = data?.message ?? data?.choices?.[0]?.message ?? {};
  return String(msg.content ?? '').trim();
}

async function agentAnthropic(
  s: AdvisorSettings,
  system: string,
  o: AgentOpts,
  toolDefs: unknown[],
  emit: (e: TraceEvent) => void,
  showThink: boolean,
): Promise<string> {
  const base = (s.baseUrl || 'https://api.anthropic.com').replace(/\/$/, '');
  const url = `${base}/v1/messages`;
  const headers = {
    'Content-Type': 'application/json',
    'x-api-key': s.apiKey,
    'anthropic-version': '2023-06-01',
    'anthropic-dangerous-direct-browser-access': 'true',
  };
  const firstContent: unknown = (o.images?.length ?? 0) > 0
    ? [...o.images!.map((img) => ({ type: 'image', source: { type: 'base64', media_type: img.mimeType, data: dataUrlBase64(img.dataUrl) } })), { type: 'text', text: o.user }]
    : o.user;
  const msgs: any[] = [
    ...(o.history ?? []).map((h) => ({ role: h.role, content: h.content })),
    { role: 'user', content: firstContent },
  ];

  let usedTools = toolDefs.length > 0;
  for (let round = 0; round < MAX_ROUNDS; round++) {
    const body: any = { model: s.model, max_tokens: 4096, system, messages: msgs };
    if (usedTools) body.tools = toolDefs;
    let data: any;
    try {
      data = await fetchJson(url, { method: 'POST', headers, body: JSON.stringify(body), signal: o.signal });
    } catch (e) {
      if (usedTools && isToolsUnsupportedError(e)) {
        emit({ kind: 'note', text: '该模型不支持工具调用，已退回直接回答' });
        usedTools = false;
        continue;
      }
      throw e;
    }
    const blocks: any[] = data?.content ?? [];
    let textAcc = '';
    for (const b of blocks) {
      if (b?.type === 'thinking' && showThink && b.thinking) emit({ kind: 'thinking', text: String(b.thinking).slice(0, 2000) });
      else if (b?.type === 'text') textAcc += b.text ?? '';
    }
    const uses = blocks.filter((b) => b?.type === 'tool_use');
    if (uses.length === 0) {
      if (!textAcc.trim()) throw new Error(`Anthropic: 没拿到正文（stop_reason=${data?.stop_reason ?? 'unknown'}）`);
      return textAcc.trim();
    }
    msgs.push({ role: 'assistant', content: blocks });
    const results: any[] = [];
    for (const u of uses) {
      const label = TOOL_LABEL[u?.name ?? ''] ?? u?.name ?? '';
      emit({ kind: 'tool_call', text: `调用 ${label}`, detail: summarizeArgs(u?.input ?? {}) });
      const out = await execTool(String(u?.name ?? ''), (u?.input ?? {}) as Record<string, unknown>, o);
      emit({ kind: 'tool_result', text: `${label} 返回`, detail: out.slice(0, 1500) });
      results.push({ type: 'tool_result', tool_use_id: u.id, content: [{ type: 'text', text: out }] });
    }
    msgs.push({ role: 'user', content: results });
  }
  emit({ kind: 'note', text: '工具轮数用满，基于已有结果直接总结' });
  const body = { model: s.model, max_tokens: 4096, system, messages: [...msgs, { role: 'user', content: '请基于以上工具结果直接给出最终回答。' }] };
  const data = await fetchJson(url, { method: 'POST', headers, body: JSON.stringify(body), signal: o.signal });
  return extractAnthropicText(data);
}

async function agentGemini(
  s: AdvisorSettings,
  system: string,
  o: AgentOpts,
  fnDecls: unknown[],
  emit: (e: TraceEvent) => void,
  showThink: boolean,
): Promise<string> {
  const base = (s.baseUrl || 'https://generativelanguage.googleapis.com').replace(/\/$/, '');
  const url = `${base}/v1beta/models/${encodeURIComponent(s.model)}:generateContent?key=${encodeURIComponent(s.apiKey)}`;
  const headers = { 'Content-Type': 'application/json' };
  const parts: unknown[] = [{ text: o.user }];
  for (const img of o.images ?? []) parts.push({ inline_data: { mime_type: img.mimeType, data: dataUrlBase64(img.dataUrl) } });
  const contents: any[] = [
    ...(o.history ?? []).map((h) => ({ role: h.role, parts: [{ text: h.content }] })),
    { role: 'user', parts },
  ];

  let usedTools = fnDecls.length > 0;
  for (let round = 0; round < MAX_ROUNDS; round++) {
    const body: any = {
      systemInstruction: { parts: [{ text: system }] },
      contents,
      generationConfig: { temperature: 0.4 },
    };
    if (usedTools) body.tools = [{ function_declarations: fnDecls }];
    let data: any;
    try {
      data = await fetchJson(url, { method: 'POST', headers, body: JSON.stringify(body), signal: o.signal });
    } catch (e) {
      if (usedTools && isToolsUnsupportedError(e)) {
        emit({ kind: 'note', text: '该模型不支持工具调用，已退回直接回答' });
        usedTools = false;
        continue;
      }
      throw e;
    }
    const respParts: any[] = data?.candidates?.[0]?.content?.parts ?? [];
    let textAcc = '';
    const calls: any[] = [];
    for (const p of respParts) {
      if (p?.thought === true && showThink && p.text) emit({ kind: 'thinking', text: String(p.text).slice(0, 2000) });
      else if (p?.text) textAcc += p.text;
      if (p?.function_call) calls.push(p.function_call);
    }
    if (calls.length === 0) return textAcc.trim();
    contents.push({ role: 'model', parts: respParts });
    for (const fc of calls) {
      const name = String(fc?.name ?? '');
      const label = TOOL_LABEL[name] ?? name;
      emit({ kind: 'tool_call', text: `调用 ${label}`, detail: summarizeArgs(fc?.args ?? {}) });
      const out = await execTool(name, (fc?.args ?? {}) as Record<string, unknown>, o);
      emit({ kind: 'tool_result', text: `${label} 返回`, detail: out.slice(0, 1500) });
      contents.push({ role: 'user', parts: [{ function_response: { name, response: { result: out } } }] });
    }
  }
  emit({ kind: 'note', text: '工具轮数用满，基于已有结果直接总结' });
  const body = {
    systemInstruction: { parts: [{ text: system }] },
    contents: [...contents, { role: 'user', parts: [{ text: '请基于以上工具结果直接给出最终回答。' }] }],
  };
  const data = await fetchJson(url, { method: 'POST', headers, body: JSON.stringify(body), signal: o.signal });
  return String(data?.candidates?.[0]?.content?.parts?.[0]?.text ?? '').trim();
}
