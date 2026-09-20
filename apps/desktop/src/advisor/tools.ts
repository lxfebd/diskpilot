// AI agent 的工具层（自 advisorClient.ts 拆出）：工具调用协议类型、23 个
// 后端工具的 execTool 分发、各 provider 的 tool definitions、以及"待用户确认"
// 的中间态（清理清单 / CLI 确认卡 / 硬件压测）。
//
// 分层：provider（HTTP+协议）← tools（本文件）← agent（多轮循环）。
// pending* 状态由 agentChat 开始时 resetAgentPending()、结束时 take/peek
// 取走交给 UI 回调——工具执行与 UI 确认弹窗之间靠这组访问器桥接。

import { api } from '../api';
import type { HwInfo, HwDiskHealth, ToolManifest, ToolManifestInvocation } from '../api';
import { ensureHwLoaded, ensureSysProbe } from '../hwCache';
import { isTauri } from '../env';
import { formatBytes } from '../format';
import { prefs } from '../theme';
import { isPermEnabled, hasSessionAuth, grantSessionAuth, type PermId } from '../permissions';
import { splitArgs } from '../components/toolbelt/services';
import { parseFirstJson } from './provider';
import type { ChatImage } from './provider';

/**
 * 按 manifest 的 args_template 把结构化参数拼成命令行参数字符串（同后端语义）。
 * 模板占位符 `{name}` 的前缀通常已含参数开关字面量（如 `--demo {demo}`、
 * `/export="{outfile}"`），所以非布尔参数直接用值替换占位符即可。
 * - 布尔参数 `{name}`（值为 true 时展开为 `flag`，false/省略整段剔除）——
 *   模板里可能是独立占位（`{silent}`）或带值形态（`/admin={admin}`）：
 *   独立占位 → 替换为 flag；带值形态 → 替换为 flag + true/false。
 * - 无值的可选参数整段剔除（连前缀开关一起）。
 * 返回渲染后的完整命令串；无占位符/无法渲染时返回 null。
 */
export function renderArgsTemplate(
  inv: ToolManifestInvocation,
  values: Record<string, string | number | boolean>,
): string | null {
  const tmpl = inv.args_template;
  if (!tmpl || !tmpl.includes('{')) return null;
  const re = /\{([a-zA-Z0-9_]+)\}/g;
  const pieces: string[] = [];
  let last = 0;
  let m: RegExpExecArray | null;
  let any = false;
  while ((m = re.exec(tmpl)) !== null) {
    const name = m[1];
    const prefix = tmpl.slice(last, m.index);
    const param = inv.params.find((p) => p.name === name);
    const raw = values[name];
    if (raw === undefined || raw === null || raw === '') {
      // 无值：这段占位（含前缀开关）整段剔除。
      last = re.lastIndex;
      continue;
    }
    const flag = param?.flag ?? '';
    if (param?.type === 'boolean') {
      // 布尔：独立占位 → 展开 flag；带值形态（前缀含 `=` 如 `/admin={admin}`）
      // → 追加 true（前缀已含开关和 `=`）。false 显式传入也剔除。
      if (raw === false) {
        last = re.lastIndex;
        continue;
      }
      const eqForm = prefix.trimEnd().endsWith('=');
      pieces.push(eqForm ? `${prefix}true` : `${prefix}${flag}`);
    } else {
      pieces.push(`${prefix}${String(raw)}`);
    }
    any = true;
    last = re.lastIndex;
  }
  const tail = tmpl.slice(last);
  if (tail.trim()) pieces.push(tail);
  return any ? pieces.join('') : null;
}

/**
 * 校验结构化参数是否符合 manifest 参数表：
 * 返回错误消息；全部通过返回 null。必填缺失、未知参数名都会被拦。
 */
export function validateManifestArgs(
  inv: ToolManifestInvocation,
  values: Record<string, unknown>,
): string | null {
  const known = new Set(inv.params.map((p) => p.name));
  for (const k of Object.keys(values)) {
    if (!known.has(k)) {
      return `参数表里没有「${k}」。请先调用 get_cli_tool_usage 查看该工具的参数表，只传参数表内声明的参数。`;
    }
  }
  for (const p of inv.params) {
    if (p.required && (values[p.name] === undefined || values[p.name] === null || values[p.name] === '')) {
      return `缺少必填参数「${p.name}」${p.flag ? `（${p.flag}）` : ''}：${p.desc}。请先调用 get_cli_tool_usage 查看参数表。`;
    }
  }
  return null;
}

export type TraceKind = 'thinking' | 'tool_call' | 'tool_result' | 'note';

export interface TraceEvent {
  kind: TraceKind;
  text: string;
  detail?: string;
}

// 一次待确认的 CLI 工具调用（图吧工具箱）。AI 通过 run_cli_tool 提交后，
// 若工具为中/高风险，后端要求 confirmed=true；这里把参数包成确认卡交 UI。
export interface PendingCliTool {
  tool: string;
  args: string[];
  timeoutSecs?: number;
  risk: string;
  /** 后端 toolbelt_run 返回的原始报错（含原因），给确认卡展示用。 */
  reason: string;
}

export interface AgentOpts {
  system?: string;
  user: string;
  /** 多轮上下文：之前的 user/assistant 回合，回放进 messages 使 AI 记住前文。
   * 缺省时每轮都是全新对话（AI 不记得上一轮说了什么）。 */
  history?: { role: 'user' | 'assistant'; content: string }[];
  images?: ChatImage[];
  /** 用户停止信号：前端「停止」按钮 abort 后，AI 请求（fetchJson→aiRequest→
   * aiProxy 或 fetch）立即中断并抛「已取消」，agent 循环不再继续。 */
  signal?: AbortSignal;
  onEvent?: (e: TraceEvent) => void;
  onProposal?: (p: AgentProposal) => void;
  /** 需要用户确认才能执行的 CLI 工具调用（中/高风险 run_cli_tool）。 */
  onToolConfirm?: (t: PendingCliTool) => void;
  /** AI 调用 get_hardware_info / analyze_disk_health 时，把结构化数据交给 UI
   * 渲染为硬件报告卡片（不塞进正文）。 */
  onHwCard?: (card: { info: HwInfo | null; health: HwDiskHealth | null }) => void;
  /** AI 调用 run_hardware_test 时，把待确认的压测参数交给 UI 弹确认面板。
   * 确认后由 UI 通过 resolveStress() 回调，agentChat 的工具调用链才能继续。 */
  onStressConfirm?: (t: PendingStress) => void;
  /** AI 调用插件管理工具（install/uninstall/activate/export）或调整电源计划等
   * 需要会话确认的操作时，把待确认的操作交给 UI 弹确认卡。确认后由 UI 通过
   * resolveSessionConfirm() 回调，agent 的工具调用链继续并把结果回填给模型。 */
  onSessionConfirm?: (t: PendingSessionConfirm) => void;
}

export interface PendingStress {
  testType: string;
  durationSeconds: number;
  tempLimitCelsius: number;
  label: string;
  tool: string;
}

/** 一次需要会话确认的 L2 操作（插件管理 / 电源计划等）。权限中心开启后
 * 仍需每次确认（除非勾选「本次会话免确认」）；UI 弹确认卡让用户拍板。 */
export interface PendingSessionConfirm {
  /** 权限 id（power.plan / plugin.manage），UI 用于展示权限名与风险级。 */
  permId: string;
  /** 操作标题，如「调整电源计划」「安装插件」。 */
  title: string;
  /** 操作目标，如电源方案名 / 插件 id / zip 路径，展示给用户看。 */
  target: string;
  /** 具体动作描述（含写目录/回收站/改系统设置等副作用）。 */
  detail: string;
}

export interface AgentProposal {
  title: string;
  items: {
    path: string;
    reason: string;
    risk: 'safe' | 'caution' | 'danger';
    what: string;
    purpose: string;
    impact: string;
  }[];
}

// ── 工具注册表（dsh 万物皆插件思路的轻量落地） ──
// 每个工具一个 ToolDef：schema（供各 provider 生成 tool defs）+ execute 执行体。
// 新增工具只需 push 一项，execTool 查表分发、buildToolDefs 遍历注册，零散改点。
export interface ToolDef {
  name: string;
  description: string;
  /** JSON-schema properties（OpenAI/Anthropic/Gemini 共用）。 */
  properties: Record<string, unknown>;
  required: string[];
  /** 只在 Tauri 环境注册（依赖后端命令）。 */
  tauriOnly?: boolean;
  /** 受 webEnabled 开关控制。 */
  webGated?: boolean;
  execute: (args: Record<string, unknown>, o: AgentOpts) => Promise<string>;
}

export const toolRegistry: ToolDef[] = [
  {
    name: 'web_search',
    description: '联网搜索（Bing/duckduckgo）。回答涉及不确定的客观事实、软件行为、最新信息时使用。结果含标题/链接/摘要，引用时给出链接。',
    properties: { query: { type: 'string', description: '搜索关键词' } },
    required: ['query'],
    webGated: true,
    execute: async (args) => {
      const q = String(args.query ?? '').trim();
      if (!q) return '搜索词为空';
      const hits = await api.webSearch(q);
      if (hits.length === 0) return '搜索没有返回结果';
      return hits.slice(0, 6).map((h, i) => `[${i + 1}] ${h.title}\nURL: ${h.url}\n摘要: ${h.snippet || '（无摘要）'}`).join('\n\n');
    },
  },
  {
    name: 'path_size',
    description: '实时测量本机某路径真实占用字节数（用户提供路径后用此核实，不要凭扫描缓存猜）。',
    properties: { path: { type: 'string', description: '绝对路径，如 C:\\Users\\x\\Downloads' } },
    required: ['path'],
    tauriOnly: true,
    execute: async (args) => {
      const p = String(args.path ?? '').trim();
      if (!p) return '路径为空';
      const bytes = await api.estimateSize(p);
      return `${p} 实测占用 ≈ ${formatBytes(bytes)}（实时遍历，非扫描缓存）`;
    },
  },
  {
    // 前端轻量版列目录（无 PathGuard）。MCP 版 `list_dir`（agent-server，带
    // 盘根/系统目录/主目录根守卫）才是 AI 的首选，本工具仅作 Web 预览降级。
    name: 'inspect_dir',
    description: '列出本机某目录下的直接子项（抽样路径），用于核实目录里到底有什么。',
    properties: { path: { type: 'string', description: '目录绝对路径' } },
    required: ['path'],
    tauriOnly: true,
    execute: async (args) => {
      const p = String(args.path ?? '').trim();
      if (!p) return '路径为空';
      const items = await api.inspect(p, 40);
      return items.length > 0 ? items.join('\n') : `${p} 是空目录、不存在或不可读`;
    },
  },
  {
    name: 'propose_cleanup_plan',
    description: '生成一份【待用户确认】的清理建议清单。你只能出清单，不能执行任何删除/回收/移动操作——清单会展示给用户，由用户逐项勾选并确认后才移入回收站。调用前先用 list_dir / path_size / get_cleanup_suggestions 核实每个路径真实存在。宁可少列不可错列：系统目录、软件安装目录、用户文档/照片/下载一律不要列入；拿不准的单项直接不列并在回答里说明。每项必须写清三件事：what（这是什么）、purpose（干什么用的）、impact（删了会怎样），并给出 risk 风险等级：safe=可放心清的缓存/临时文件，caution=谨慎（如旧安装包、构建产物），danger=高危（可能含用户数据、需重新生成、耗时恢复）。',
    properties: {
      title: { type: 'string', description: '一句话概括，如「C 盘缓存与临时文件」' },
      items: {
        type: 'array',
        description: '建议清理的路径清单，最多 10 项，按把握从大到小排序',
        items: {
          type: 'object',
          properties: {
            path: { type: 'string', description: '绝对路径（Windows 反斜杠），必须真实存在' },
            what: { type: 'string', description: '这是什么（一句话）' },
            purpose: { type: 'string', description: '它是干什么用的' },
            impact: { type: 'string', description: '删了会怎样（有无影响 / 如何恢复）' },
            risk: { type: 'string', enum: ['safe', 'caution', 'danger'], description: '风险等级：safe 可放心清 / caution 谨慎 / danger 高危' },
            reason: { type: 'string', description: '（可选）一句话补充为什么可删' },
          },
          required: ['path', 'what', 'purpose', 'impact', 'risk'],
        },
      },
    },
    required: ['title', 'items'],
    tauriOnly: true,
    execute: async (args) => {
      // 纯出计划：AI 只生成待确认清单，绝不执行任何文件操作。清单交给 UI
      // 确认窗，由用户逐项勾选 + 确认后才真正移入回收站。兼容旧 wrapped 形态。
      const src = (args.propose_cleanup ?? args) as Record<string, unknown>;
      const norm = normalizeProposal(src);
      if (!norm) return '清理清单为空：items 里每项必须有 path。请核对路径后重新生成。';
      pendingProposal = norm;
      return `清理建议清单已生成（${norm.items.length} 项）。你绝不能自行执行任何删除/回收。清单已交给用户确认窗，由用户逐项勾选并确认后才执行。请简短总结你建议清理了什么、预期释放多少，并提醒用户逐项核对路径后再勾选。不要再调用工具。`;
    },
  },
  {
    name: 'get_disk_health',
    description: '读取本机所有磁盘的实时容量与使用率（总/已用/可用）。回答磁盘空间、哪块盘快满了、该清哪里之前先调用本工具获取真实数据，不要凭记忆或猜测。',
    properties: {},
    required: [],
    tauriOnly: true,
    execute: async () => {
      // 走 agent-server MCP disk_health（AI 工具统一单源），避免与 list_drives 双实现
      const r = await api.callTool('disk_health', {});
      if (r.is_error) return `（读取失败）${r.text}`;
      return r.text;
    },
  },
  {
    name: 'get_steam_library_summary',
    description: '只读汇总本机 Steam 游戏库：游戏总数、总占用、沉睡（大体积且久未启动）游戏 Top 榜、鬼魂安装数、创意工坊订阅数。用户问「Steam 占了多少空间/有哪些游戏可以卸载腾空间/Steam 库情况」时调用本工具拿真实数据。注意：本工具只读元数据（ACF），不读取游戏文件内容；卸载建议只提示用户去 Steam Inspector 或 Steam 客户端操作，你不能执行任何卸载。',
    properties: {},
    required: [],
    tauriOnly: true,
    execute: async () => {
      const inv = await api.listSteamGames();
      if (!inv.steam_root) {
        return `未检测到 Steam（已检查：${inv.candidates_checked.join('、') || '无'}）。用户可能没装 Steam 或装在其他位置。`;
      }
      const games = inv.libraries.flatMap((l) => l.games);
      if (games.length === 0) return `Steam 在 ${inv.steam_root}，但库里没有游戏。`;
      const totalBytes = games.reduce((s, g) => s + g.size_bytes, 0);
      const ghosts = games.filter((g) => g.is_ghost);
      const sleepers = games
        .filter((g) => g.default_recommended)
        .sort((a, b) => b.size_bytes - a.size_bytes)
        .slice(0, 8);
      const workshop = games.reduce((s, g) => s + g.workshop_item_count, 0);
      const lines: string[] = [
        `Steam 库：${games.length} 款游戏，共 ${formatBytes(totalBytes)}（${inv.libraries.length} 个库根）。`,
        `鬼魂安装（ACF 残留、目录缺失）：${ghosts.length} 个。`,
        workshop > 0 ? `创意工坊订阅合计：${workshop} 项。` : '',
      ].filter(Boolean);
      if (sleepers.length > 0) {
        const recBytes = games.filter((g) => g.default_recommended).reduce((s, g) => s + g.size_bytes, 0);
        lines.push('', `沉睡游戏（体积大且久未启动，共可释放 ≈ ${formatBytes(recBytes)}）：`);
        for (const g of sleepers) {
          lines.push(`- ${g.name_en}：${formatBytes(g.size_bytes)} · ${g.recommendation_reason ?? ''}`);
        }
        lines.push('', '如需处理，请引导用户打开「工作台 → Steam 盘点」查看完整清单，卸载由 Steam 客户端完成（本应用不代删）。');
      } else {
        lines.push('', '没有达到推荐卸载标准（≥30GB 且 ≥6 月未启动，或 ≥50GB 且 ≥3 月未启动）的游戏。');
      }
      return lines.join('\n');
    },
  },
  {
    name: 'get_cleanup_suggestions',
    description: '获取后端已算好的某路径下的清理建议清单（label/说明/可释放字节数/文件数）。用户想知道"某个目录能清理出多少空间"时调用；返回的是 DiskPilot 扫描脚本匹配的清理项，按可释放大小从大到小排列。',
    properties: {
      path: { type: 'string', description: '要分析的目录绝对路径，如 C:\\Users\\x\\AppData\\Local' },
      scope_days: { type: 'object', description: '可选：{ scope_id: 保留天数 }，只对指定清理范围限制天数；省略则用默认策略', additionalProperties: { type: 'number' } },
    },
    required: ['path'],
    tauriOnly: true,
    execute: async (args) => {
      // 清理建议：直接调后端已算好的 cleanup_suggestions，不重复计算
      const rootPath = String(args.path ?? '').trim();
      const scopeDays = args.scope_days && typeof args.scope_days === 'object'
        ? (args.scope_days as Record<string, unknown>)
        : undefined;
      const daysMap: Record<string, number> = {};
      if (scopeDays) {
        for (const [k, v] of Object.entries(scopeDays)) {
          if (typeof v === 'number' && v > 0) daysMap[k] = v;
        }
      }
      const suggestions = await api.cleanupSuggestions(rootPath, Object.keys(daysMap).length > 0 ? daysMap : undefined);
      if (suggestions.length === 0) return `${rootPath} 没有可清理的建议（可能尚未扫描或没有匹配项）。`;
      return [
        ...suggestions.map((s) => `- ${s.label}（${s.path}）：${s.desc}（约 ${formatBytes(s.bytes)}，${s.files} 个文件）`),
        '',
        '这些 path 是磁盘扫描脚本算出的真实目录，出清理清单时只能引用上面出现的 path，逐一对应；禁止凭印象编造或替换路径（如 C:\\Users\\Public\\…）。',
      ].join('\n');
    },
  },
  {
    name: 'get_fan_status',
    description: '读取当前风扇控制状态（只读）：通道探测结果（WMI 只读 / CLI 外挂可写）、当前温度、是否已应用自定义转速、后台温度守护是否运行。用户问"风扇现在什么状态""能不能调速"时调用。',
    properties: {},
    required: [],
    tauriOnly: true,
    execute: async () => {
      const st = await api.fanStatus();
      const probe = st.probe ?? {};
      const cliName = probe.cli?.name ?? '无';
      const lines = [
        `可调速通道：${st.writable ? '有（CLI：' + cliName + '）' : '无（WMI 只读，未检测到 nbfc）'}`,
        `当前温度：${st.temp_c != null ? Math.round(st.temp_c) + '℃' : '读不到'}`,
        `已应用自定义转速：${st.has_snapshot ? '是（有原转速快照）' : '否'}`,
        `后台温度守护：${st.guard_running ? '运行中' : '未运行'}`,
        st.note ? `说明：${st.note}` : '',
      ];
      return lines.filter(Boolean).join('\n');
    },
  },
  {
    name: 'adjust_fan_curve',
    description: '调整风扇转速（L2 写操作）：按档位（quiet/balanced/balanced_plus/high/urgent）落地调速。需要用户开启「风扇控制」权限并逐次确认；应用后后台有温度熔断守护（默认 90℃，超限自动回退原转速）。用户说"把风扇调安静点""风扇拉满""调风扇"时调用；只读场景请用 get_fan_status。',
    properties: {
      level: { type: 'string', enum: ['quiet', 'balanced', 'balanced_plus', 'high', 'urgent'], description: '目标档位：quiet≈35% / balanced≈55% / balanced_plus≈70% / high≈85% / urgent=100%' },
      temp_breaker_c: { type: 'number', description: '可选：熔断温度阈值（默认 90℃，范围 70-105）。超过该温度自动回退原转速。' },
    },
    required: ['level'],
    tauriOnly: true,
    execute: async (args) => {
      if (!isPermEnabled('fan.control')) {
        return '风扇控制未授权：请前往「设置 → AI 权限中心」开启「风扇控制」后重试。';
      }
      const level = String(args.level ?? '').trim();
      if (!['quiet', 'balanced', 'balanced_plus', 'high', 'urgent'].includes(level)) {
        return '请提供有效档位 level（quiet / balanced / balanced_plus / high / urgent）。';
      }
      const breaker = typeof args.temp_breaker_c === 'number' ? args.temp_breaker_c : 90;
      const g = await promptSessionConfirm(
        'fan.control',
        '调整风扇转速',
        level,
        `通过调速通道把风扇调到「${level}」档（后台温度熔断 ${Math.round(breaker)}℃ 自动回退）`,
      );
      if (!g.granted) {
        return '调整风扇转速需要用户确认后执行，已取消，未做任何修改。';
      }
      const res = await api.fanControl(level, undefined, breaker, true);
      return JSON.stringify(res, null, 2);
    },
  },
  {
    name: 'get_system_info',
    description: '读取本机系统信息 + 实时状态（Windows 版本 / CPU 核心与当前占用 / 内存总量与使用率 / 各磁盘容量 / 开机时长）。用户问"我这电脑什么配置""电脑现在卡不卡""帮我看看电脑状态"时调用。要评价电脑当前运行情况（CPU/内存是否吃紧），用本工具或 run_system_probe 拿真实数据。',
    properties: {},
    required: [],
    tauriOnly: true,
    execute: async () => {
      const ua = typeof navigator !== 'undefined' ? navigator.userAgent : '';
      const plat = ua.match(/Windows NT (\d+\.\d+)/)?.[1] ?? '';
      const winVer =
        plat === '10.0' ? 'Windows 10 / 11' :
        plat === '6.3' ? 'Windows 8.1' :
        plat === '6.2' ? 'Windows 8' :
        plat === '6.1' ? 'Windows 7' :
        plat ? `Windows (NT ${plat})` : '未知系统';
      const lines = [`- 系统：${winVer}`];
      try {
        // 真实探针：CPU 占用/内存总量与使用/磁盘/开机时长（Win32，非浏览器近似值）
        // 走 5 秒共享缓存：切页/多轮查询不重复采样。
        const p = await ensureSysProbe(false);
        const upDays = Math.floor(p.uptime_secs / 86400);
        const upHrs = Math.floor((p.uptime_secs % 86400) / 3600);
        lines.push(
          `- CPU：${p.cpu_cores} 逻辑核心，当前占用约 ${p.cpu_percent.toFixed(1)}%`,
          `- 内存：${formatBytes(p.mem_total_bytes)}，已用 ${formatBytes(p.mem_used_bytes)}（${p.mem_percent.toFixed(1)}%）`,
          `- 开机时长：${upDays} 天 ${upHrs} 小时`,
        );
        if (p.drives.length > 0) {
          lines.push('- 磁盘：');
          for (const d of p.drives) {
            lines.push(`  - ${d.path}：总 ${formatBytes(d.total_bytes)} / 已用 ${formatBytes(d.used_bytes)}（${d.percent.toFixed(1)}%）`);
          }
        }
      } catch {
        lines.push(
          '- 实时数据不可用（非 Tauri 环境），CPU 核心 / 内存为浏览器近似值：',
          `- CPU 逻辑核心：${(navigator as { hardwareConcurrency?: number }).hardwareConcurrency ?? '未知'}`,
          `- 内存：${(navigator as { deviceMemory?: number }).deviceMemory ?? '未知'} GB`,
        );
      }
      return lines.join('\n');
    },
  },
  {
    name: 'run_system_probe',
    description: '实时采集电脑运行状态：CPU 总占用百分比、内存总量/已用/使用率、各磁盘容量使用率、开机时长、CPU 逻辑核心数。用户让你"评价电脑运行情况""看看现在卡不卡""跑测试时帮我盯一下负载"时调用本工具拿真实数据；sample=true 时做 600ms 双采样测 CPU 实时占用更准（默认单次更快）。烤机/压测期间可反复调用观察负载变化。',
    properties: { sample: { type: 'boolean', description: 'true = 双采样测实时 CPU 占用（多花 ~0.6 秒）；默认 false' } },
    required: [],
    tauriOnly: true,
    execute: async (args) => {
      const sample = args.sample === true;
      const p = await ensureSysProbe(sample);
      const upDays = Math.floor(p.uptime_secs / 86400);
      const upHrs = Math.floor((p.uptime_secs % 86400) / 3600);
      const lines = [
        `CPU 占用：${p.cpu_percent.toFixed(1)}%（${p.cpu_cores} 逻辑核心）`,
        `内存：${formatBytes(p.mem_total_bytes)}，已用 ${formatBytes(p.mem_used_bytes)}（${p.mem_percent.toFixed(1)}%）`,
        `开机时长：${upDays} 天 ${upHrs} 小时`,
      ];
      if (p.drives.length > 0) {
        lines.push('磁盘：');
        for (const d of p.drives) {
          lines.push(`- ${d.path}：总 ${formatBytes(d.total_bytes)} / 已用 ${formatBytes(d.used_bytes)}（${d.percent.toFixed(1)}%）`);
        }
      }
      return lines.join('\n');
    },
  },
  {
    name: 'get_tool_manifest',
    description: '获取图吧工具箱中某个工具的结构化能力说明书（Tool Manifest JSON）：用途、适用场景、调用参数、风险等级、副作用、示例。比 get_cli_tool_usage 更详细（含完整参数表/副作用/官方链接）。执行 run_cli_tool 前若需要确认参数细节可用本工具。',
    properties: { toolName: { type: 'string', description: '工具名，如 WizTree、CrystalDiskInfo、Prime95' } },
    required: ['toolName'],
    tauriOnly: true,
    execute: async (args) => {
      // 按需查单个工具的五段式能力说明书（结构化 JSON）：AI 在执行前可只
      // 取自己关心的字段（invocation.params / risk / side_effects），比注入
      // 全量索引省 token。非 Tauri / 未收录返回 null。
      const tool = String(args.toolName ?? '').trim();
      if (!tool) return '工具名为空：请提供 toolName。';
      const m = await getManifestFor(tool);
      if (!m) return `工具箱中没有「${tool}」的能力说明书（未收录或非 Tauri 环境）。`;
      return JSON.stringify(m, null, 2).slice(0, 4000);
    },
  },
  {
    name: 'get_cli_tool_usage',
    description: '获取图吧工具箱中某个命令行工具的完整使用文档（绝对路径、参数表、示例、注意事项）。执行 run_cli_tool 之前必须先调用本工具确认参数，再执行。',
    properties: { toolName: { type: 'string', description: '工具名，如 WizTree、CrystalDiskInfo、Prime95、FurMark、autorunsc' } },
    required: ['toolName'],
    tauriOnly: true,
    execute: async (args) => {
      const tool = String(args.toolName ?? '').trim();
      if (!tool) return '工具名为空：请提供 toolName。';
      const u = await api.toolbeltUsage(tool);
      const m = await getManifestFor(tool);
      if (m) {
        // Manifest 优先：完整五段式能力说明书比 usage 文档更适合 AI 决策
        const perm = m.permission_level ? ` · 权限 ${m.permission_level}` : '';
        const params = m.invocation.params.map((p) => {
          const def = p.default != null ? `（默认 ${p.default}）` : '';
          return `  - ${p.flag || p.name} ${p.required ? '[必填]' : '[可选]'}: ${p.desc}${def}`;
        }).join('\n');
        const examples = m.examples.map((ex) => `  - \`${ex.args}\` —— ${ex.desc}${ex.expect ? `（期望：${ex.expect}）` : ''}`).join('\n');
        const pub = m.publisher ? ` | 出品：${m.publisher}` : '';
        const tags = m.tags.length > 0 ? ` | 标签：${m.tags.join(' / ')}` : '';
        const tut = m.tutorial_url ? `\n官方文档/教程：${m.tutorial_url}` : '';
        const dl = m.download_hint ? `\n获取（官方源）：${m.download_hint}` : '';
        return [
          `# ${m.name}（${m.category}，风险 ${m.risk}${perm}${pub}${tags}）`,
          `用途：${m.purpose}`,
          `适合用：${m.when_to_use}`,
          `别用它：${m.when_not_to_use}`,
          `副作用：${m.side_effects}`,
          `调用方式：${m.invocation.mode === 'cli' ? `命令行 ${m.invocation.args_template || '（无固定模板）'}` : 'GUI（不可命令行调用）'}`,
          m.invocation.params.length > 0 ? `参数：\n${params}` : '',
          m.examples.length > 0 ? `示例：\n${examples}` : '',
          `绝对路径：\`${u.exe}\``,
          tut,
          dl,
        ].filter(Boolean).join('\n');
      }
      return [`# ${u.tool}（${u.category}，风险 ${u.risk}）`, `绝对路径：\`${u.exe}\``, '', u.usage].join('\n');
    },
  },
  {
    name: 'run_cli_tool',
    description: '运行图吧工具箱中的命令行工具（如 WizTree 分析磁盘占用、CrystalDiskInfo 查硬盘 SMART、Prime95/FurMark 烤机）。中/高风险工具会弹出确认框，由用户确认后才真正执行；低风险工具直接运行。执行前务必先调 get_cli_tool_usage 查看该工具的参数表和示例，确认参数无误再调用。对长时间运行的工具（烤机、基准测试）请设置较大的 timeout_secs。',
    properties: {
      toolName: { type: 'string', description: '工具名，与 get_cli_tool_usage 一致' },
      args: { type: 'string', description: '命令行参数（空格分隔），无参数则省略或传空字符串' },
      structured_args: {
        type: 'object',
        description: '可选：按 get_cli_tool_usage 参数表传的结构化参数（参数名→值）。提供时后端按 args_template 拼参并校验必填；与 args 二选一，同时提供时以 structured_args 为准。',
        additionalProperties: true,
      },
      timeout_secs: { type: 'number', description: '可选：超时秒数，默认 60，范围 5~3600；烤机/压测请设大' },
    },
    required: ['toolName'],
    tauriOnly: true,
    execute: async (args, o) => {
      const tool = String(args.toolName ?? '').trim();
      if (!tool) return '工具名为空：请提供 toolName。';
      const m = await getManifestFor(tool);
      if (m?.invocation.mode === 'gui') {
        return `「${tool}」是 GUI 工具，无法命令行调用，请让用户双击启动。`;
      }
      const timeoutSecs = typeof args.timeout_secs === 'number' && args.timeout_secs > 0
        ? Math.round(args.timeout_secs) : undefined;
      // 结构化参数优先：按 manifest 参数表校验 + args_template 拼参。
      // AI 用 get_cli_tool_usage 拿到参数表后传结构化对象，比自由文本更不易出错。
      const structArgs = args.structured_args;
      if (structArgs && m) {
        const err = validateManifestArgs(m.invocation, structArgs as Record<string, unknown>);
        if (err) return err;
        const rendered = renderArgsTemplate(m.invocation, structArgs as Record<string, string | number | boolean>);
        if (rendered === null) {
          return `无法按模板拼参数：${m.invocation.args_template}。请改用 args 传原始参数，或先调用 get_cli_tool_usage 确认参数名。`;
        }
        const rawArgs = rendered;
        return execCliTool(tool, rawArgs, timeoutSecs, m, o);
      }
      const rawArgs = String(args.args ?? '').trim();
      return execCliTool(tool, rawArgs, timeoutSecs, m, o);
    },
  },
  {
    name: 'get_hardware_info',
    description: '读取本机硬件信息：CPU / GPU / 内存 / 主板 / BIOS / 磁盘 / 温度传感器。只读操作，零风险，随时可调用。返回结构化 JSON。用户问"我这电脑什么配置""配置怎么样"时调用。',
    properties: {
      categories: {
        type: 'array',
        description: '可选：只返回这些类别（cpu/gpu/memory/disk/board/bios/os/system/thermal），省略返回全部',
        items: { type: 'string' },
      },
    },
    required: [],
    tauriOnly: true,
    execute: async (args, o) => {
      const cache = await ensureHwLoaded();
      const info = cache.info ?? {};
      const categories = Array.isArray(args.categories) ? (args.categories as string[]) : [];
      const filtered = categories.length > 0
        ? Object.fromEntries(Object.entries(info).filter(([k]) => categories.includes(k)))
        : info;
      const json = JSON.stringify(filtered, null, 2);
      // UI 渲染硬件报告卡片：不把原始 JSON 塞进正文，让模型自己总结。
      o.onHwCard?.({ info: filtered as HwInfo, health: cache.health });
      return json;
    },
  },
  {
    name: 'analyze_disk_health',
    description: '分析硬盘健康状态：读取 SMART 通电时间 / 温度 / 磨损度 / 读写错误。只读操作，零风险。用户问"我的硬盘还好吗""硬盘寿命多少"时调用。非管理员权限下部分字段可能为 null。',
    properties: {},
    required: [],
    tauriOnly: true,
    execute: async (_args, o) => {
      const cache = await ensureHwLoaded();
      o.onHwCard?.({ info: cache.info, health: cache.health });
      return JSON.stringify(cache.health, null, 2);
    },
  },
  {
    name: 'generate_hw_report',
    description: '生成完整的硬件检测报告（markdown / json / html）。只读操作，零风险。用户说"给我一份硬件报告""导出配置清单"时调用。savePath 可选：指定保存路径（必须是绝对路径，扩展名 .md/.html/.json/.txt，不会覆盖已有文件）。',
    properties: {
      format: { type: 'string', enum: ['markdown', 'html', 'json'], description: '报告格式，默认 markdown' },
      savePath: { type: 'string', description: '可选：保存到该绝对路径' },
    },
    required: [],
    tauriOnly: true,
    execute: async (args) => {
      const fmt = String(args.format ?? 'markdown').trim() || 'markdown';
      const savePath = typeof args.savePath === 'string' && args.savePath.trim()
        ? args.savePath.trim()
        : undefined;
      // 带 savePath 写文件属于 L1「导出报告到文件」：权限中心未开启时拒绝，
      // 不静默退回只读预览（后端也没有独立的保存确认门，全靠这层权限）。
      if (savePath && !isPermEnabled('hw.export')) {
        return '导出报告到文件未授权：请前往「设置 → AI 权限中心」开启「导出报告到文件」后重试（不带 savePath 的预览不受影响）。';
      }
      const out = await api.hwReport(fmt, savePath);
      return [
        `报告格式：${out.format}`,
        out.saved_path ? `已保存到：${out.saved_path}` : '',
        out.note,
        '—— 正文预览（前 3000 字符） ——',
        out.markdown.slice(0, 3000),
      ].filter(Boolean).join('\n');
    },
  },
  {
    name: 'run_hardware_test',
    description: '运行硬件压力/基准测试。🔴 这是高负载操作，会导致 CPU/GPU 满载、温度升高。必须在用户明确确认后才会执行，AI 不得自行执行。测试期间持续监控温度，超过上限自动停止；用户也可随时在监控条上点击停止。支持：cpu_stress（Prime95 CPU 烤机）、gpu_stress（FurMark GPU 烤机）、gpu_benchmark（FurMark 基准）、cpu_benchmark（AIDA64 基准）。内存专项与磁盘专项暂未开放。',
    properties: {
      testType: {
        type: 'string',
        enum: ['cpu_stress', 'gpu_stress', 'gpu_benchmark', 'cpu_benchmark'],
        description: '测试类型',
      },
      durationSeconds: { type: 'number', description: '测试持续时间（秒），默认 300，范围 30~1800' },
      tempLimitCelsius: { type: 'number', description: '温度上限（℃），超过即自动停止，默认 90' },
    },
    required: ['testType'],
    tauriOnly: true,
    execute: async (args) => runHardwareTest(args),
  },
  {
    name: 'adjust_power_plan',
    description: '调整 Windows 电源计划（平衡 / 高性能 / 节能）。需用户在「AI 权限中心」开启此权限，且每次执行仍需用户确认。只支持 balanced / high_performance / power_saver 三个已知方案。',
    properties: {
      scheme: { type: 'string', enum: ['balanced', 'high_performance', 'power_saver'], description: '目标电源计划' },
    },
    required: ['scheme'],
    tauriOnly: true,
    execute: async (args) => {
      if (!isPermEnabled('power.plan')) {
        return '电源计划调整未授权：请前往「设置 → AI 权限中心」开启「调整电源计划」后重试。';
      }
      const scheme = String(args.scheme ?? '').trim();
      if (!scheme) return '请提供 scheme（balanced / high_performance / power_saver）';
      const g = await promptSessionConfirm(
        'power.plan',
        '调整电源计划',
        scheme,
        `执行 powercfg /setactive ${scheme} 切换 Windows 电源计划（影响性能与续航）`,
      );
      if (!g.granted) {
        return '调整电源计划需要用户确认后执行，已取消，未做任何修改。';
      }
      const report = await api.powerPlan(scheme, true);
      return JSON.stringify(report, null, 2);
    },
  },
  {
    name: 'list_plugins',
    description: '列出工具箱里所有已安装的插件（目录内含 tool.plugin.json 的工具）。返回每个插件的 id/名称/版本/作者/分类/入口/权限声明。用户问"有哪些插件/装了哪些扩展"时调用。',
    properties: {},
    required: [],
    tauriOnly: true,
    execute: async () => {
      const cat = await api.toolbeltCatalog();
      const plugs = cat.categories
        .flatMap((c) => c.tools)
        .filter((t): t is typeof t & { plugin: NonNullable<typeof t.plugin> } => !!t.plugin)
        .map((t) => ({
          id: t.plugin.id,
          name: t.plugin.name || t.name,
          version: t.plugin.version,
          author: t.plugin.author || '',
          category: t.category,
          entry: t.plugin.entry,
          risk: t.plugin.risk || t.risk,
          permissions: t.plugin.permissions,
          dir_rel: t.dir_rel,
        }));
      if (plugs.length === 0) return '工具箱里没有插件（目录内都没有 tool.plugin.json）。';
      const lines = [`已安装插件（${plugs.length}）：`];
      for (const p of plugs) {
        lines.push(
          `- ${p.name}（v${p.version}）`,
          `  id: ${p.id} · 分类: ${p.category} · 作者: ${p.author || '未知'} · 风险: ${p.risk}`,
          `  入口: ${p.entry} · 权限: ${p.permissions.join('/') || '无'}`,
        );
      }
      return lines.join('\n');
    },
  },
  {
    name: 'install_plugin',
    description: '从本地 .zip 插件包安装一个工具插件：包内必须有 tool.plugin.json（含 id/category/entry），解压到 Tools 根对应分类。已存在同名插件会报错（需先卸载）。用户提供插件 zip 路径时调用。',
    properties: { zipPath: { type: 'string', description: '插件包 .zip 的绝对路径' } },
    required: ['zipPath'],
    tauriOnly: true,
    execute: async (args) => {
      const zipPath = String(args.zipPath ?? '').trim();
      if (!zipPath) return '请提供插件包 .zip 路径（zipPath）。';
      const g = await promptSessionConfirm(
        'plugin.manage',
        '安装插件',
        zipPath,
        `从 ${zipPath} 解压安装插件到工具箱 Tools 目录（zip 包内内容将写入磁盘）`,
      );
      if (!g.granted) {
        return '插件管理未授权：请前往「设置 → AI 权限中心」开启「管理插件」并确认后执行。';
      }
      const rel = await api.pluginInstall(zipPath, true);
      return `插件安装成功：${rel}`;
    },
  },
  {
    name: 'uninstall_plugin',
    description: '卸载一个已安装插件：把它的工具目录移入系统回收站（可恢复，可在「最近清理/撤销」里恢复）。只接受插件 id（list_plugins 返回的 id）。卸载会连带移除该工具目录。',
    properties: { id: { type: 'string', description: '插件 id（list_plugins 返回）' } },
    required: ['id'],
    tauriOnly: true,
    execute: async (args) => {
      const id = String(args.id ?? '').trim();
      if (!id) return '请提供插件 id（list_plugins 返回）。';
      const g = await promptSessionConfirm(
        'plugin.manage',
        '卸载插件',
        id,
        `把插件「${id}」的整个工具目录移入系统回收站（可从「最近清理/撤销」恢复）`,
      );
      if (!g.granted) {
        return '插件管理未授权：请前往「设置 → AI 权限中心」开启「管理插件」并确认后执行。';
      }
      const r = await api.pluginUninstall(id, true);
      return `插件「${r.name}」已卸载（移入回收站可恢复）：${r.dir_rel}`;
    },
  },
  {
    name: 'market_plugins',
    description: '列出内置插件市场：toolbelt 收录的 CLI 工具哪些可插件化、哪些已插件化。用户问"有哪些工具可以变成插件/市场里有什么"时调用。',
    properties: {},
    required: [],
    tauriOnly: true,
    execute: async () => {
      const list = await api.pluginMarket();
      if (list.length === 0) return '市场为空（toolbelt 内置清单未收录任何工具）。';
      const installed = list.filter((p) => p.installed);
      const avail = list.filter((p) => !p.installed);
      const fmt = (p: { name: string; category: string; risk: string; permission_level: string; purpose: string }) =>
        `- ${p.name}（${p.category}，风险 ${p.risk}${p.permission_level ? ` / ${p.permission_level}` : ''}）：${p.purpose}`;
      return [
        `内置插件市场（共 ${list.length}）：`,
        `已插件化（${installed.length}）：`,
        ...installed.map(fmt),
        `可插件化（${avail.length}）：`,
        ...avail.map(fmt),
        '「插件化」只补写 tool.plugin.json（工具本体已在工具箱），用 activate_plugin 执行。',
      ].join('\n');
    },
  },
  {
    name: 'activate_plugin',
    description: '把市场里的某个工具插件化：给它的工具目录补写 tool.plugin.json。只补清单、不动工具本体；已插件化会报错。用户说"把 X 变成插件/安装插件"时配合 market_plugins 使用。',
    properties: { tool: { type: 'string', description: '市场里的工具名（market_plugins 返回），如 Prime95' } },
    required: ['tool'],
    tauriOnly: true,
    execute: async (args) => {
      const tool = String(args.tool ?? '').trim();
      if (!tool) return '请提供工具名（tool）。';
      const g = await promptSessionConfirm(
        'plugin.manage',
        '插件化',
        tool,
        `给工具「${tool}」的目录补写 tool.plugin.json，把它标记为插件（不修改工具本体）`,
      );
      if (!g.granted) {
        return '插件管理未授权：请前往「设置 → AI 权限中心」开启「管理插件」并确认后执行。';
      }
      const path = await api.pluginActivate(tool, true);
      return `「${tool}」已插件化：${path}`;
    },
  },
  {
    name: 'export_plugin',
    description: '把已安装插件（目录含 tool.plugin.json）打包成可分发 zip，用于分享/迁移。只接受插件 id（list_plugins 返回）；用户给出保存路径时调用，未给则用默认文件名。',
    properties: {
      id: { type: 'string', description: '插件 id（list_plugins 返回）' },
      outZip: { type: 'string', description: '保存为的 .zip 绝对路径；省略时用默认文件名（插件目录同级的 <id>-v<version>.zip）' },
    },
    required: ['id'],
    tauriOnly: true,
    execute: async (args) => {
      const id = String(args.id ?? '').trim();
      if (!id) return '请提供插件 id（list_plugins 返回）。';
      const outZip = String(args.outZip ?? '').trim() || undefined;
      const g = await promptSessionConfirm(
        'plugin.manage',
        '导出插件',
        id,
        `把插件「${id}」打包成 zip（${outZip ?? '默认路径'}）写入磁盘`,
      );
      if (!g.granted) {
        return '插件管理未授权：请前往「设置 → AI 权限中心」开启「管理插件」并确认后执行。';
      }
      // 未给路径时让前端用默认名（挂在 Tools 根下，方便找）
      const r = await api.pluginExport(id, outZip ?? '', true);
      return `插件「${r.name}」已导出：${r.zip}`;
    },
  },
];

// ── agent-server MCP 工具集（39 个，全只读 L0）────────────────────────
// 独立进程（stdio MCP）由后端 spawn，本表只声明 name/description/schema，
// execute 统一走后端 agent_call_tool 桥接。描述文案与参数与
// crates/agent-server/src/tools/mod.rs 保持一致（文档见 docs/tools-agent/TOOL_CATALOG.md）。
function mcpTool(
  name: string,
  description: string,
  properties: Record<string, unknown> = {},
  required: string[] = [],
  confirmPerm?: PermId,
  confirmWhen?: (args: Record<string, unknown>) => boolean,
): ToolDef {
  return {
    name,
    description,
    properties,
    required,
    tauriOnly: true,
    execute: async (args) => {
      // 写工具（process_kill / service_control 等）先过权限确认门；
      // confirmWhen 提供条件确认：只有满足条件（如 bench_disk 真跑）才弹确认，
      // 其余调用（如 dry-run 只出计划）无需打扰用户。
      // 权限未开 / 用户没确认 → 直接返回取消说明，绝不把未确认的写请求发给后端。
      if (confirmPerm && (!confirmWhen || confirmWhen(args))) {
        const permTitle =
          confirmPerm === 'file.recycle' ? '回收文件到回收站'
          : confirmPerm === 'app.uninstall' ? '卸载软件'
          : confirmPerm === 'hw.stress' ? '压力测试'
          : confirmPerm === 'fan.control' ? '风扇控制'
          : '控制系统状态';
        const g = await promptSessionConfirm(
          confirmPerm,
          permTitle,
          name,
          `AI 要调用 ${name}（${description.slice(0, 80)}…），这会改变系统状态，需要你确认。`,
        );
        if (!g.granted) return `调用 ${name} 需要用户确认后执行，已取消。`;
      }
      const r = await api.callTool(name, args, confirmPerm && (!confirmWhen || confirmWhen(args)) ? true : undefined);
      if (r.is_error) return `（被拒/出错）${r.text}`;
      return r.text;
    },
  };
}

// 75 个 MCP 工具（64 只读 + 11 写操作，与 crates/agent-server/src/tools/mod.rs
// 及 apps/desktop/src-tauri/src/agent.rs WRITE_TOOLS 同源对齐）注册进
// toolRegistry（execTool / buildToolDefs 查表自动生效）。写工具带 confirmPerm，
// execute 时先过 sys.control 权限门 + 确认弹窗，confirmed=true 才真正调用。
export const mcpToolRegistry: ToolDef[] = [
  // A. 文件/磁盘（8）
  mcpTool('disk_health',
    '读取本机所有磁盘的总/已用/可用空间与使用率（只读，实时）。回答磁盘空间/哪块盘快满了/该清哪里之前先调用。'),
  mcpTool('disk_partition_usage',
    '读取某路径所在卷（分区）的用量：总/已用/可用字节与使用率（只读）。比 disk_health 更细粒度——同一盘上不同挂载点用量不同。',
    { path: { type: 'string', description: '要查的路径（返回该路径所在卷的用量），如 C:\\Users' } },
    ['path']),
  mcpTool('disk_volume_meta',
    '列出每个分区的类型元数据（只读）：本地磁盘/可移动/网络/光驱 + 文件系统（NTFS/exFAT/FAT32）+ 卷标。回答「这个盘是什么盘/能不能插拔」时用。'),
  mcpTool('file_type_stats',
    '递归统计某目录下按扩展名聚合的文件数与占用字节（只读，20 万文件/20 层硬上限）。分析「什么文件占了空间」时用。',
    { path: { type: 'string', description: '要统计的目录绝对路径' }, top_n: { type: 'number', description: '返回前 N 大占用扩展名（默认 20）' } },
    ['path']),
  mcpTool('file_tree',
    '列出目录树（深度限制，带文件大小；只读）。想一眼看清「某目录下结构长什么样」时用。安全边界：盘根/系统目录/主目录根会被拒绝。',
    { path: { type: 'string', description: '目录绝对路径' }, max_depth: { type: 'number', description: '最大深度（默认 3，上限 10）' }, max_nodes: { type: 'number', description: '最多节点数（默认 100，上限 500）' } },
    ['path']),
  mcpTool('list_dir',
    '列出某目录的直接子项（不递归）：名称/是否目录/文件大小（只读）。核实「目录里到底有什么」时用。安全边界：盘根/系统目录/主目录根会被拒绝。',
    { path: { type: 'string', description: '目录绝对路径' }, limit: { type: 'number', description: '最多返回多少项（默认 100）' } },
    ['path']),
  mcpTool('read_file',
    '读取文本文件内容（只读，UTF-8/UTF-16 自动识别，超 256KB 截断）。核实某个配置文件/脚本/日志内容时用。只接受文件，拒绝目录。',
    { path: { type: 'string', description: '文件绝对路径' } },
    ['path']),
  mcpTool('find_files',
    '在指定目录下按文件名关键字递归查找文件/目录（只读，大小写不敏感，带深度/数量上限）。找「某个文件在哪」时用。',
    { root: { type: 'string', description: '搜索根目录绝对路径' }, keyword: { type: 'string', description: '文件名关键字（大小写不敏感）' }, max_hits: { type: 'number', description: '最多返回多少条命中（默认 50）' } },
    ['root', 'keyword']),

  // B. 磁盘深入（3）
  mcpTool('disk_find_biggest_files',
    '列出某目录下最大的文件（递归，只读）。回答「什么文件占了空间/哪个文件最大」时用。安全边界：盘根/系统目录/主目录根会被拒绝。',
    { path: { type: 'string', description: '要搜索的目录绝对路径' }, top_n: { type: 'number', description: '最多返回多少个文件（默认 20）' } },
    ['path']),
  mcpTool('disk_find_duplicate_files',
    '找出某目录下疑似重复的文件（递归，按大小分组并按头部 64KB 哈希比对；只读）。回答「哪些文件重复了/有什么可删的重复文件」时用。',
    { path: { type: 'string', description: '要搜索的目录绝对路径' }, top_n: { type: 'number', description: '最多返回多少组重复（默认 20）' } },
    ['path']),
  mcpTool('disk_io_usage',
    '读取磁盘实时 IO 使用率：每块盘的活动时间百分比与读写速率（只读，需跑一次 PowerShell 采样约 1 秒）。回答「磁盘是不是满了/卡了/谁在读写盘」时用。',
    { sample_ms: { type: 'number', description: '采样窗口毫秒（默认 1000）' } }),

  // C. 系统/进程/程序（7）
  mcpTool('system_info',
    '读取本机系统信息与实时状态：系统版本/CPU 逻辑核心/内存总量与使用率/开机时长（只读）。回答「电脑什么配置/现在卡不卡/帮我看看电脑状态」时调用。'),
  mcpTool('list_processes',
    '列出本机进程（只读）：PID/名称/内存占用/可执行文件路径，按内存占用降序。回答「有什么程序在跑/哪个进程占内存/某进程存在吗」时调用。',
    { top_n: { type: 'number', description: '最多返回多少个进程（默认 50）' } }),
  mcpTool('process_info',
    '查询单个进程的详情（只读）：PID/名称/内存/可执行文件路径/父进程 PID。回答「这个进程是什么/谁拉起的」时用。',
    { pid: { type: 'number', description: '进程 PID，如 1234' } },
    ['pid']),
  mcpTool('app_list',
    '列出本机已安装程序（只读，注册表 Uninstall 键）：名称/版本/发布者/安装位置/预估大小。回答「这台电脑装了什么软件/这个软件能卸载吗」时调用。',
    { keyword: { type: 'string', description: '按名称/发布者关键字过滤（可选）' }, limit: { type: 'number', description: '最多返回多少个（默认 50，上限 200）' } }),
  mcpTool('sys_services',
    '列出 Windows 服务：服务名/显示名/状态/启动类型/进程 PID（只读）。可按关键字过滤、可含已禁用服务。回答「什么服务在跑/某服务是干嘛的」时调用。',
    { keyword: { type: 'string', description: '按服务名/显示名关键字过滤（可选）' }, show_disabled: { type: 'boolean', description: '是否包含已禁用服务（默认 false）' }, top_n: { type: 'number', description: '最多返回多少个（默认 50）' } }),
  mcpTool('sys_drivers',
    '列出驱动（设备驱动）：名称/类型/厂商/驱动日期与版本/设备状态（只读）。回答「装了什么驱动/某驱动是干嘛的/驱动是不是太旧」时调用。',
    { keyword: { type: 'string', description: '按驱动名关键字过滤（可选）' }, top_n: { type: 'number', description: '最多返回多少个（默认 50）' } }),
  mcpTool('sys_boot_items',
    '列出开机启动项：注册表 Run 键 + 启动文件夹（只读，不删除不禁用），可附带计划任务。僵尸启动项（文件已不存在）会标红。回答「开机自动启动了什么/怎么精简开机项」时调用。',
    { include_scheduled: { type: 'boolean', description: '是否附带读取计划任务启动项（较慢，默认 false）' } }),

  // D. 硬件（8）
  mcpTool('hw_cpu',
    '读取 CPU 信息：型号/核心/线程数/基频与当前频率/缓存/实时占用率（只读，WMI，不需要管理员）。回答「CPU 是什么/几个核/占用高不高」时调用。'),
  mcpTool('hw_gpu',
    '读取显卡信息：型号/显存/驱动版本与日期/分辨率/刷新率，NVIDIA 卡附实时温度与占用（nvidia-smi）、AMD 卡附实时温度与占用（atiadlxx ADL）（只读，不需要管理员）。回答「什么显卡/显存多大/GPU 占用」时调用。'),
  mcpTool('hw_memory',
    '读取内存信息：每根内存条容量/频率/厂商/型号 + 总容量 + XMP/EXPO 是否生效诊断（只读，不需要管理员）。回答「内存多大/频率多少/该不该开 XMP」时调用。'),
  mcpTool('hw_motherboard',
    '读取主板/整机信息：主板厂商与型号/BIOS 版本与日期/整机品牌型号（只读，不需要管理员）。回答「这是什么主机/主板是什么」时调用。'),
  mcpTool('hw_temperature',
    '读取本机温度：CPU/磁盘等 ACPI 热区温度 + GPU 实时温度（NVIDIA 走 nvidia-smi，AMD 走 atiadlxx ADL，只读，传感器读不到会明确说明，不会假装有数据）。回答「电脑温度/烫不烫」时调用。'),
  mcpTool('hw_sensors',
    '读取全量硬件传感器快照（AIDA64 同类）：CPU/GPU/主板温度、风扇转速、电压、功耗、频率、负载，按硬件分组输出（只读）。优先走 LibreHardwareMonitor 内核（fancmd sensors），可读 CPU 核心温度/主板温度/风扇/电压/功耗；读不到（缺 Ring0 驱动/管理员）时自动降级到 ACPI 热区 + GPU + SMART 通道。回答「温度/风扇/电压/功耗/整机健康」时调用，比 hw_temperature 更全。'),
  mcpTool('hw_battery',
    '读取电池信息：电量百分比/电池健康度/充电状态/循环次数/设计容量（只读，不需要管理员；台式机返回「未检测到电池」）。回答「笔记本电池健康吗/要不要换电池」时调用。'),
  mcpTool('hw_disk_smart',
    '读取磁盘健康（SMART）：每块物理磁盘的容量/接口/健康状态/通电时间/温度/磨损与错误计数（只读，Win10 1607+ 普通权限即可，不需要管理员）。回答「硬盘健康吗/通电多久/要坏了吗」时调用。'),
  mcpTool('hw_superio',
    '自研 SuperIO 直读（只读 L0，复刻 HWiNFO/LibreHardwareMonitor 的纯端口直读）：动态加载本机已装的端口驱动（inpoutx64 优先，普通权限即可，无需管理员/Ring0），扫描 0x2E/0x4E 双端口识别 ITE/Nuvoton/Winbond SuperIO 芯片，直读环境控制器寄存器返回主板/CPU 温度 + 风扇转速 + PWM 占空比。HWiNFO 没装/没开共享内存且不想依赖 LHM 时用它兜底。回答「主板读到的温度/风扇转速是什么」或 HWiNFO 不可用时调用。纯只读，不写寄存器、不调速。'),

  // E. 网络（5）
  mcpTool('net_status',
    '读取本机网络状态：每个网卡名称/连接状态/IP/链接速率/是否虚拟（只读，不需要管理员）。回答「电脑联网了吗/网卡是什么/IP 多少/网速多少」时调用。'),
  mcpTool('net_connections',
    '读取网络连接列表：本地/远端地址与端口/状态/所属进程（只读，不需要管理员）。默认只看已建立的连接；可按 PID 或关键字过滤。回答「谁在连网/哪个程序在联网」时调用。',
    { pid: { type: 'number', description: '按进程 PID 过滤（可选）' }, keyword: { type: 'string', description: '按关键字过滤（可选）：匹配进程名或远端 IP' }, top_n: { type: 'number', description: '最多返回多少条（默认 30）' } }),
  mcpTool('net_speed',
    '实测网络实时速率：采集 interval_ms 毫秒窗口内的收发流量并换算为 KB/s（只读，需跑一次 PowerShell 采样约 2-10 秒）。回答「当前网速快不快/下载多少兆」时调用。',
    { interval_ms: { type: 'number', description: '采样窗口毫秒（1000-10000，默认 2000）' } }),
  mcpTool('net_share',
    '读取本机共享文件夹（SMB 共享）列表：共享名/路径/权限类型（只读；会话/打开文件统计需管理员，非管理员会降级说明）。回答「电脑共享了什么文件夹/开了哪些共享」时调用。'),
  mcpTool('net_wifi',
    '读取无线网络信息：本机无线网卡状态 + 已保存的 WiFi 网络名称列表（绝不读取密码，只读）。回答「WiFi 连的是什么/保存了哪些 WiFi」时调用。'),

  // F. 游戏/清理/CPU/外设/磁盘深挖（6，2026-09-09 新增）
  mcpTool('steam_games',
    '读取本机 Steam 游戏库：库路径/每个游戏的安装大小/上次游玩时间/是否「幽灵安装」（文件已不存在）与清理建议理由（只读，不需要管理员）。回答「Steam 装了哪些游戏/哪个游戏占空间/哪些游戏可以清理」时调用。',
    { top_n: { type: 'number', description: '最多返回多少个游戏（默认 20）' } }),
  mcpTool('cleanup_suggestions',
    '扫描指定目录，与内置「已知可清理软件清单」（scaffolds/）匹配，给出可清理的缓存/临时文件占用统计（只读，绝不删除任何文件；AI 不会自动执行清理）。回答「这个文件夹里有什么可以清理的」时调用。',
    { root: { type: 'string', description: '要扫描的目录绝对路径' }, top_n: { type: 'number', description: '最多返回多少条建议（默认 20）' } },
    ['root']),
  mcpTool('disk_top_directories',
    '列出某目录下第一层各子目录的占用大小与文件数（递归，只读），回答「哪个子目录占了空间」时用。安全边界：盘根/系统目录/主目录根会被拒绝。',
    { path: { type: 'string', description: '要统计的目录绝对路径' }, top_n: { type: 'number', description: '最多返回多少个（默认 20）' } },
    ['path']),
  mcpTool('process_cpu_usage',
    '读取进程 CPU 实时占用（两次 200ms 采样差值，只读）：PID/名称/单核占用百分比/内存。回答「哪个进程在吃 CPU/电脑卡是哪个程序导致」时调用。',
    { top_n: { type: 'number', description: '最多返回多少个进程（默认 30）' } }),
  mcpTool('usb_devices',
    '列出本机 USB 设备：名称/实例 ID/设备类/厂商/当前状态（只读元数据，不读任何设备内容与配置，不需要管理员）。回答「电脑上插了哪些 USB 设备/某个 USB 设备是干嘛的」时调用。'),
  mcpTool('disk_smart_raw_attributes',
    '读取磁盘 SMART 原始可靠性计数器：每块物理磁盘的通电时长/温度原始值/磨损/读写错误计数/启停循环（只读，Win10 1607+ 普通权限即可）。比 hw_disk_smart 更原始，适合「盘是不是快坏了/用了多久」的深入诊断。'),

  // G. 安全/审计（3）
  mcpTool('security_event_logs',
    '读取系统事件日志：指定日志名（逗号分隔，默认 System）、最近 hours 小时、最多 max_events 条（只读）。回答「最近系统出了什么错误/蓝屏/警告/日志」时调用。',
    { logs: { type: 'string', description: '日志名（逗号分隔），如 System 或 System,Application（默认 System）' }, hours: { type: 'number', description: '回溯小时数（默认 24）' }, max_events: { type: 'number', description: '每个日志最多返回多少条（默认 20）' } }),
  mcpTool('security_firewall_rules',
    '列出防火墙规则：方向（默认 Inbound 入站）与动作（默认 Allow 允许）过滤，最多 top_n 条（只读，不需要管理员）。回答「防火墙开了什么/某程序被放行了吗」时调用。',
    { direction: { type: 'string', description: '方向过滤：Inbound / Outbound / Any（默认 Inbound）' }, action: { type: 'string', description: '动作过滤：Allow / Block / Any（默认 Allow）' }, top_n: { type: 'number', description: '最多返回多少条（默认 20）' } }),
  mcpTool('security_login_events',
    '读取最近的登录/注销事件（安全日志 4624/4625 + PowerShell 脚本块日志 4104，只读）。需要管理员权限才读得到安全日志，无权限会明确降级。回答「谁登录过这台电脑/有没有异常登录」时调用。',
    { max_events: { type: 'number', description: '最多返回多少条（默认 20）' } }),

  // H. 系统环境（2）
  mcpTool('recycle_bin_stats',
    '查询本机回收站状态（只读，Windows API）：逐卷文件数与总大小 + 合计。回答「回收站占了多少空间/回收站里有多少东西/能不能清回收站腾空间」时调用。只读状态，不执行清空。'),
  mcpTool('env_vars',
    '读取环境变量值（只读）：传 name 查指定变量；不传列出常用子集（PATH/TEMP/USERPROFILE/APPDATA 等）。回答「某个路径在哪/环境变量配没配/命令找不到是不是 PATH 问题」时调用。',
    { name: { type: 'string', description: '要查的环境变量名（可选，省略列出常用子集）' } }),

  // I. 系统控制 + 应用卸载（写操作）——confirmPerm 走权限确认门
  // （L2 默认关，每次确认；与后端 agent.rs WRITE_TOOLS 白名单 + agent-server
  //  PathGuard/服务黑名单守卫构成三层防线）。
  mcpTool('process_kill',
    '结束指定 PID 的进程（**写操作**）。安全守卫：系统关键进程（PID<5）与系统目录中的进程会被拒绝。AI 在用户明确要求『把某程序/某进程结束掉』时才调用，结束后建议用 list_processes 复查。',
    { pid: { type: 'number', description: '要结束的进程 PID，如 1234' } },
    ['pid'],
    'sys.control'),
  mcpTool('service_control',
    '控制 Windows 服务：启动 / 停止 / 重启（**写操作，可逆**）。系统关键服务会被安全守卫拒绝。用户明确要求『停掉/启动/重启某服务』时调用；参数 action 取 start / stop / restart。',
    { name: { type: 'string', description: '服务名（先调 sys_services 确认名字）' }, action: { type: 'string', description: 'start 启动 / stop 停止 / restart 重启' } },
    ['name', 'action'],
    'sys.control'),
  mcpTool('process_start',
    '启动一个已安装程序（**写操作，白名单执行**）。只允许系统目录或已安装程序目录（System32 / Program Files / LocalAppData 等）下的 .exe，命令必须存在，参数原样传入。用户明确要求『打开/启动某程序』（如记事本/计算器/某已装软件）时调用；启动即返回，不等待进程退出。',
    { command: { type: 'string', description: '程序绝对路径，如 C:\\Windows\\System32\\notepad.exe' }, args: { type: 'array', items: { type: 'string' }, description: '传给程序的参数列表（可选）' }, cwd: { type: 'string', description: '进程工作目录（可选）' } },
    ['command'],
    'sys.control'),
  mcpTool('file_recycle',
    '把单个文件/目录移入系统回收站（**写操作，可逆**）。安全守卫：盘根/系统目录/用户主目录根会被拒绝。用户明确要求『把某个文件/文件夹扔进回收站』（如确认是垃圾的临时文件）时调用；只进回收站绝不直接删除，误删可从回收站还原。',
    { path: { type: 'string', description: '要回收的文件/目录绝对路径' } },
    ['path'],
    'file.recycle'),
  mcpTool('scheduled_task_manage',
    '管理计划任务（**写操作，仅可逆动作**）：action 取 enable 启用 / disable 禁用 / query 只读查询状态。系统内置任务（\\Microsoft\\ / \\Windows\\ / \\System32\\ 路径下）拒绝修改；不开放 delete/run；目标任务不存在则拒绝。用户明确要求『禁用/启用某个计划任务』时调用；dry_run=true 只出计划不执行。',
    { name: { type: 'string', description: '计划任务名（先调 sys_boot_items include_scheduled=true 确认）' }, action: { type: 'string', description: 'enable 启用 / disable 禁用 / query 查询' }, dry_run: { type: 'boolean', description: '只出计划不执行（默认 false）' } },
    ['name', 'action'],
    'sys.control'),
  mcpTool('uninstall_app',
    '卸载已安装程序（**写操作**，启动其官方卸载器）。白名单：仅 Program Files / LocalAppData 等已安装目录下的卸载器，静默参数白名单（/S /silent /quiet /qn 等），拒绝 runas/delete 等破坏性参数。用户明确要求『卸载某软件』时先调 app_list 确认名称再调用；卸载器分离启动（界面由用户操作），完成后 app_list 复查确认。',
    { name: { type: 'string', description: '程序 DisplayName（先调 app_list 确认）' } },
    ['name'],
    'app.uninstall'),

  // J. 风扇通道自修复（2）——跨机器环境匹配：诊断只读恒开，修复写走 fan.control
  mcpTool('fan_selfheal_diag',
    '诊断风扇写控通道（只读）：调用 fancmd diag json，返回端口驱动是否加载/驱动名/RTC 端口验证/SuperIO 芯片与基址/风扇列表/通道可写性，并给出明确可修复点（缺驱动？芯片不可识别？）。在新机器上发现风扇控制不可用时先调它拿诊断。'),
  mcpTool('fan_selfheal_fix',
    '修复风扇写控通道（**写操作 L2**，用户确认后执行）：action=install_driver 安装并启动 inpoutx64 端口驱动（需管理员，复制 DLL 到 System32 + sc create/start，绝不覆盖已存在文件）；action=reset_fan <idx> 恢复风扇为主板自动控制（安全可逆）。dry_run=true 只出计划不执行。AI 先跑 fan_selfheal_diag 确认根因，dry-run 出计划给用户确认后再真正执行。',
    { action: { type: 'string', description: 'install_driver（装驱动）/ reset_fan <idx>（恢复风扇 idx 主板控制）' }, dry_run: { type: 'boolean', description: '只出计划不执行（默认 true）' } },
    ['action'],
    'fan.control'),
  mcpTool('fan_control',
    '直接调速风扇（**写操作 L2**，用户确认后执行）：把指定风扇设为软件控制的固定转速百分比。idx=风扇索引（先 fan_selfheal_diag 查 fans 列表确认）、pct=目标转速 0-100（越界自动钳制）。执行前自动校验写通道（驱动未加载/芯片不可识别时拒绝）。dry_run=true 只出计划不执行。软件接管后可随时 fan_selfheal_fix reset_fan 恢复主板自动控制。AI 先 fan_selfheal_diag 看可写性，dry-run 给用户确认后再传 dry_run=false 真正调速。',
    { idx: { type: 'string', description: '风扇索引（数字，先 fan_selfheal_diag 查 fans 列表）' }, pct: { type: 'number', description: '目标转速 0-100' }, dry_run: { type: 'boolean', description: '只出计划不执行（默认 true）' } },
    ['idx', 'pct'],
    'fan.control'),

  // K. 工具箱（toolbelt）接入 AI：30 个工具，只读清单恒开，执行走 toolbelt.run 权限门
  mcpTool('toolbelt_list',
    '列出图吧工具箱集成工具清单（只读）：工具名/分类/CLI|GUI/权限级/风险/用途。判断「这个场景该用工具箱哪个工具」时先调它。CLI 工具可被 AI 直接执行（toolbelt_run），GUI 工具只能启动提示用户。',
    {}),
  mcpTool('toolbelt_run',
    '执行图吧工具箱的 CLI 工具（**写操作 L2**，用户确认后执行）：tool=工具名（先 toolbelt_list 确认）、args=参数列表（透传不拼 shell）、confirmed=用户确认（medium/high 风险必须 true）、timeout_secs=超时秒。三层防线：L3（如 FPT64 刷 BIOS）永久禁止；medium/high 需 confirmed=true；cli.run 权限未开启拒绝。AI 先 toolbelt_list 确认工具与命令，medium+ 风险先展示命令给用户确认再传 confirmed=true。',
    { tool: { type: 'string', description: '工具箱工具名，如 crystaldiskinfo / wiztree / defraggler' }, args: { type: 'array', items: { type: 'string' }, description: '传给工具的 CLI 参数（可选，透传不拼 shell）' }, confirmed: { type: 'boolean', description: '用户已确认执行（medium/high 风险必须 true）' }, timeout_secs: { type: 'number', description: '执行超时秒数（默认 30，上限 300）' } },
    ['tool'],
    'cli.run'),

  // L. AIDA64 核心复刻（9 只读 L0）：CPU/内存/磁盘基准 + 系统体检报告 +
  // 传感器趋势/阈值告警 + 信息补全（CPU 指令集/DRAM 时序/显示器）
  mcpTool('bench_cpu',
    'CPU 基准测试（只读）：整数质数筛（ops/s）+ 浮点 π 莱布尼茨级数（iters/s），多核并行。纯计算无副作用，用于横向比较 CPU 计算能力；结果受睿频/散热/负载影响，短跑取中位更稳。secs=测试秒数（默认 3，上限 20），threads=线程数（默认全部逻辑核）。'),
  mcpTool('bench_memory',
    '内存基准测试（只读）：读/写/复制带宽（MB/s）+ 指针追逐延迟（ns），固定 64MB 缓冲避免全缓存命中。用于横向比较内存子系统性能；参考 DDR4-3200 双通道 ≈ 25-40GB/s 读、20-30GB/s 写、60-90ns 延迟。secs=测试秒数（默认 3，上限 20）。'),
  // bench_disk：默认 dry_run=true 只出计划（纯只读，不弹确认）；只有用户明确要求
  // 测磁盘速度（dry_run=false 真跑，会在目标目录写临时文件）才过 hw.stress 确认门——
  // 与后端 agent.rs 的 bench_disk 特判同一条边界（纵深防御）。
  mcpTool('bench_disk',
    '磁盘基准测试（只读，dry-run 默认）：顺序读/随机读/顺序写/随机写，从 4KB 与 128KB 块开始，用临时文件测后即删。**默认 dry_run=true 只出计划**；用户明确要求测磁盘速度时传 dry_run=false 真正执行（需用户确认）。path=被测目录（默认系统临时目录），size_mb=测试文件大小 MB（默认 32）。注意临时目录所在盘即为被测盘。',
    { path: { type: 'string', description: '被测目录（默认系统临时目录，该目录所在盘即为被测盘）' }, size_mb: { type: 'number', description: '测试文件大小 MB（默认 32）' }, dry_run: { type: 'boolean', description: '只出计划不创建文件（默认 true）；true 不触发确认' } },
    [],
    'hw.stress',
    (args) => args.dry_run === false),
  mcpTool('system_report',
    '一键系统体检报告（只读）：汇总 CPU/GPU/内存/主板/温度/磁盘健康/系统信息/电池为 Markdown 报告，含型号/核心数/频率/序列号/容量/SMART 状态/电源等。用于「帮我看下这台电脑配置怎么样」「生成一份体检报告」等场景，结果可直接发给用户。'),
  mcpTool('sensor_trend',
    '传感器趋势（只读）：追加记录当前 CPU 最高温/内存占用/磁盘健康到 JSONL 历史，返回最近 n 条趋势。每次调用会写一条快照到 %LOCALAPPDATA%/diskpilot/agent-server/sensor-history.jsonl（最多保留 500 条），用于「温度/占用是不是在变高」的趋势分析。n=返回最近条数（默认 20）。'),
  mcpTool('sensor_alert',
    '传感器阈值告警（只读）：检查 CPU 温度/磁盘温度/GPU 温度/内存占用/磁盘剩余空间是否超过阈值，返回告警清单或「全部正常」。阈值可自定义：cpu_temp（默认 85℃）/disk_temp（默认 55℃）/gpu_temp（默认 85℃）/mem_percent（默认 90%）/disk_free_gb（默认 20GB）。persist=true 时告警追加到 sensor-alerts.jsonl。'),
  mcpTool('hw_cpu_features',
    'CPU 指令集与虚拟化特性（只读）：型号/核心线程/硬件虚拟化（VT-x/AMD-V）状态/二级地址翻译 SLAT/VM 监控模式 VMX/SVM/用户态已确认指令集（SSE2/SSE4.2/AVX/AVX2/FMA/AES-NI）。用于「这台电脑能开虚拟机吗」「支持哪些指令集」等场景。'),
  mcpTool('hw_dram_timings',
    'DRAM 内存时序信息（只读）：每条内存条的槽位/容量/厂商/型号/标称频率/实际运行频率，含 XMP 一致性诊断（多根频率不一致会提示）。完整 CL-tRCD-tRP 时序需 SPD 读取工具，本工具不编造具体数值；检测到 Thaiphoon Burner 会提示。'),
  mcpTool('hw_displays',
    '显示器信息（只读）：物理尺寸（cm/英寸）/设备型号/输入类型/激活状态。用于「显示器多大」「几台显示器」等场景；分辨率/刷新率/EDID 序列号需专用工具解析。'),
  mcpTool('stress_test',
    'CPU 稳定性压测 + 超温熔断（**写操作 L1**，会占满 CPU）：多线程满载整数运算，每秒采样 CPU 温度，达到 max_temp 自动熔断停止。用于「帮我烤机看看稳不稳」「测下散热」等场景。secs=压测秒数（默认 5，上限 600），max_temp=熔断温度阈值（默认 95℃，范围 60-110），threads=压测线程数（默认全部）。**无温度读取时熔断护栏失效**，会如实报告「无温度熔断护栏运行」；hw.stress 权限默认开启，每次执行前用户确认。',
    { secs: { type: 'number', description: '压测持续秒数（默认 5，上限 600）' }, max_temp: { type: 'number', description: '超温熔断阈值℃（默认 95，范围 60-110）' }, threads: { type: 'number', description: '压测线程数（默认全部逻辑核）' } },
    ['secs'],
    'hw.stress'),
  mcpTool('stress_cancel',
    '停止当前压测（只读 L0 控制信号，无参数）：向正在运行的 stress_test / stress_test_gpu 发送停止信号（进程内 CANCEL flag + 跨进程停止文件，两种通道任一命中即退出），压测循环检测到后立即退出并报告「已按停止信号结束」。前端「停止」按钮 / 用户在 AI 对话里说「停」时调用。无压测运行时是安全空操作（返回「当前无压测在跑」）。'),

  // L2. 装机验收新工具（3 个）：内存稳定性 / GPU 压测温度守门 / 蓝屏分析
  mcpTool('mem_test',
    '内存稳定性测试（只读 L0，MemTest86 简化版）：进程内分配 size_mb 缓冲，用 全零/全一/AA55/递增/伪随机 5 种 pattern 反复写入并读回校验，发现位翻转/不稳定内存。用于「内存条稳不稳」「超频是否稳定」场景。size_mb=缓冲大小（默认 256，64-4096），rounds=轮数（默认 1，1-10）。只测进程堆内存读写正确性，无法替代 pre-boot 版 MemTest86 的硬件寻址层，如实说明。'),
  mcpTool('stress_test_gpu',
    'GPU 稳定性压测（**写操作 L1**，复刻 FurMark 的温度熔断守门版）：纯后端无 OpenCL/CUDA/WebGL 引擎无法让 GPU 满载（深度压载需厂商 SDK，同 GPGPU 基准结论），本工具承担温度熔断守门——GPU 压测程序（前端甜甜圈页/外挂 FurMark/游戏/渲染）运行时，在 secs 秒内持续采样 GPU 温度（N 卡 nvidia-smi，A 卡 atiadlxx ADL），超 max_temp 即报熔断信号保护硬件。secs=守门秒数（默认 5，上限 120），max_temp=熔断阈值℃（默认 90，范围 60-110）。无 N/A 卡温度通道时如实降级「无温度护栏」。hw.stress 权限，每次执行前用户确认。',
    { secs: { type: 'number', description: '温度熔断守门秒数（默认 5，上限 120）' }, max_temp: { type: 'number', description: 'GPU 超温熔断阈值℃（默认 90，范围 60-110）' } },
    ['secs'],
    'hw.stress'),
  mcpTool('bsod_analyze',
    '蓝屏转储分析（只读 L0，复刻 BlueScreenView）：扫描 C:\\Windows\\Minidump\\*.dmp 与根目录 MEMORY.DMP，解析每个转储的崩溃时间/bugcheck 代码/4 参数，映射中文含义（0x124 WHEA=CPU/内存硬件、0x116 VIDEO_TDR=显卡驱动、0x1A MEMORY_MANAGEMENT=内存、0x7F=CPU 超频/过热等 24 个常见代码）。用于「为什么蓝屏了」「蓝屏代码什么意思」场景。max_dumps=最多解析几个（默认 10，上限 30），include_memory_dmp=是否读根目录 MEMORY.DMP（默认 true）。无转储返回「未记录到蓝屏」。纯只读不改删文件。',
    { max_dumps: { type: 'number', description: '最多解析几个转储（默认 10，上限 30）' }, include_memory_dmp: { type: 'boolean', description: '是否读根目录 MEMORY.DMP（默认 true）' } }),

  // L3. 信息补全（4 只读 L0）：软件许可证 / 网卡明细 / Defender 状态 / 用户账户
  mcpTool('app_licenses',
    '软件许可证（只读 L0）：Windows 激活状态（slmgr /xpr）+ Office 激活状态（订阅版/永久版）。用于「这电脑正版吗」「Windows/Office 激活了吗」场景。Windows 激活查询可能需要管理员权限，无权限如实返回。纯只读不改系统。'),
  mcpTool('net_adapter_detail',
    '网卡详细配置（只读 L0）：每张网卡的状态/速率/逐 IP 地址（IPv4 带子网掩码、IPv6 带前缀长度）/MAC 地址/网关/DNS 服务器。用于「我的 IP 是什么」「子网掩码是多少」「MAC 地址/网关 DNS」场景。'),
  mcpTool('sys_defender_status',
    'Windows Defender 安全中心状态（只读 L0）：服务状态 + 实时保护开关 + 病毒库/引擎版本 + 最近快速扫描时间。用于「杀毒软件开没开」「Windows 安全中心状态」「病毒库新不新」场景；若系统用第三方杀软（Defender 被替换）会如实说明。'),
  mcpTool('sys_user_accounts',
    '本机用户账户清单（只读 L0）：账户名/全名/是否禁用/是否锁定/本地或域账户/SID 尾段，排除内置系统账户。用于「这台电脑有哪些用户」「有几个账户」场景；隐私红线：绝不输出密码/token 信息。'),

  // M. AIDA64 长尾（3 只读 L0）：GPU 快照 / IPMI 探测 / ACPI 固件
  mcpTool('bench_gpu',
    'GPU 快照（只读，复刻 AIDA64 GPGPU Benchmark 只读部分）：显卡型号/显存/驱动/分辨率/温度（N 卡 nvidia-smi / A 卡 atiadlxx ADL）。深度 GPGPU 基准（像素填充率/OpenCL）需 GPU 厂商 SDK 未实现，如实标注。用于「显卡参数/驱动/温度」或用户要求 GPU 基准时先调用。'),
  mcpTool('hw_ipmi',
    'IPMI 服务器接口探测（只读）：检测主板是否带 BMC/IPMI 接口（服务器/HEDT 板才有，家用台式机通常无——如实返回未检测到，不编造传感器）。用于「这板子支不支持 IPMI/远程管理」场景。'),
  mcpTool('hw_acpi',
    'ACPI 固件信息（只读，复刻 AIDA64 ACPI Browser 简化版）：BIOS 厂商/版本/发布日期/序列号 + 机箱厂商/型号/系统类型。完整 ACPI 表浏览（FADT/DSDT 反汇编）需 Ring0 工具属专业向；用于「BIOS 版本/主板固件」场景。'),
];
// MCP 工具并进 AI 工具表；凡自定义工具已覆盖同能力（get_disk_health 已改指
// MCP disk_health）的，跳过 MCP 副本，避免 AI 同时看到两个同名工具。
const DEDUP_MCP = new Set(['disk_health']);
toolRegistry.push(...mcpToolRegistry.filter((t) => !DEDUP_MCP.has(t.name)));

// ── 用户配置的 MCP 服务器工具（动态，可插拔）───────────────────────
// 与静态 mcpToolRegistry 分离：不污染 43 断言；每次 refreshMcpTools()
// 从后端 mcp_list_tools 拉取用户添加的 MCP 服务器工具，重建本表。
// execute 走 api.mcpCallTool（带 server_id）；writable 服务器工具先走
// promptSessionConfirm('mcp.manage') 确认门再执行。
export let dynamicMcpTools: ToolDef[] = [];

/** 拉取用户 MCP 服务器工具清单并重建 dynamicMcpTools（幂等，非 Tauri 直接清空）。 */
export async function refreshMcpTools(): Promise<void> {
  try {
    const tools = await api.mcpListTools();
    dynamicMcpTools = tools.map((t) => ({
      name: t.name,
      description: t.description
        + (t.writable ? '（此工具来自可写 MCP 服务器，调用需用户确认）' : '（只读 MCP 服务器工具）'),
      properties: (t.input_schema as { properties?: Record<string, unknown> })?.properties ?? {},
      required: ((t.input_schema as { required?: string[] })?.required ?? []).map(String),
      tauriOnly: true,
      execute: async (args) => {
        // 工具级权限判定与后端 resolve_perm 同源：L0 直接执行；L1/L2 需确认。
        // （writable 只影响缺省推断，显式 permission_map 可把某工具降到 L0/L1。）
        const needConfirm = t.perm === 'L1' || t.perm === 'L2';
        if (needConfirm) {
          const g = await promptSessionConfirm('mcp.manage', '调用 MCP 服务器工具', t.name,
            `工具 ${t.name} 权限级别 ${t.perm}（${t.writable ? '可写服务器' : '只读服务器'}），调用需用户确认。`);
          if (!g.granted) return `调用 ${t.name} 需要用户确认后执行，已取消。`;
        }
        const r = await api.mcpCallTool(t.server_id, t.name, args, needConfirm);
        if (r.is_error) return `（被拒/出错）${r.text}`;
        return r.text;
      },
    }));
  } catch {
    dynamicMcpTools = [];
  }
}
// 把模型给的一份提议（wrapped 或扁平）规整成 { title, items }，每项带
// risk + 三段式 what/purpose/impact。工具调用与正文兜底共用。
function normalizeProposal(src: Record<string, unknown>): { title: string; items: AgentProposal['items'] } | null {
  const items = Array.isArray(src.items) ? (src.items as Array<Record<string, unknown>>) : [];
  const parsed = items
    .filter((it) => it && typeof it === 'object')
    .map((it) => ({
      path: String(it.path ?? '').trim(),
      reason: String(it.reason ?? '').slice(0, 200),
      risk: (String(it.risk ?? 'caution') === 'safe' ? 'safe' : String(it.risk) === 'danger' ? 'danger' : 'caution') as 'safe' | 'caution' | 'danger',
      what: String(it.what ?? '').slice(0, 200),
      purpose: String(it.purpose ?? '').slice(0, 200),
      impact: String(it.impact ?? '').slice(0, 200),
    }))
    .filter((it) => it.path.length > 0)
    .slice(0, 10);
  if (parsed.length === 0) return null;
  return { title: String(src.title ?? '清理清单').slice(0, 60), items: parsed };
}

// 兜底解析：有些模型无视 function-calling 协议，把提议写成 ```json 代码块
// 塞进正文。识别出合规结构同样弹确认窗，不让功能取决于模型是否支持 tools。
export function parseProposalFromText(text: string): AgentProposal | null {
  if (!text || (!text.includes('"propose_cleanup"') && !text.includes('"propose_cleanup_plan"'))) return null;
  const candidates: string[] = [];
  const fence = /```(?:json)?\s*([\s\S]*?)```/gi;
  let m: RegExpExecArray | null;
  while ((m = fence.exec(text)) !== null) candidates.push(m[1]);
  candidates.push(text);
  for (const c of candidates) {
    try {
      const j = parseFirstJson(c) as Record<string, unknown>;
      const src = (j?.propose_cleanup_plan ?? j?.propose_cleanup ?? j) as Record<string, unknown>;
      const norm = normalizeProposal(src);
      if (norm) return norm;
    } catch { /* 尝试下一个候选 */ }
  }
  return null;
}

// 一次 agent 运行里最后一次成功的清理提议；agentChat 结束时交给调用方
// （UI）弹出确认窗，由用户逐项确认后才执行。
let pendingProposal: AgentProposal | null = null;

// 一次 agent 运行里待用户确认的 CLI 工具调用（run_cli_tool 中/高风险）。
// 与 pendingStress / pendingSessionConfirm 同理：命中确认门后挂起，等 UI
// 通过 resolveCliTool() 回填决策，agent 的工具调用链继续并把真实执行结果
// 作为 tool_result 回给模型——不让模型误以为「已经发了确认请求就没下文了」。
let pendingCliTool: PendingCliTool | null = null;
let pendingCliResolve: ((r: { run: boolean; remember: boolean }) => void) | null = null;

// 一次 agent 运行里待用户确认的硬件压测（run_hardware_test）。
// 与 pendingCliTool 不同：压测确认后 agentChat 的工具调用链要继续往下走，
// 所以这里存一个可 resolve 的 Promise，UI 确认后回填结果。
let pendingStress: PendingStress | null = null;
let pendingStressResolve: ((r: { run: boolean; remember: boolean }) => void) | null = null;

// 一次 agent 运行里待用户确认的 L2 会话操作（插件管理 / 电源计划等）。
// 与 pendingStress 同理：确认后 execute 要继续，返回结果回填给 agent。
let pendingSessionConfirm: PendingSessionConfirm | null = null;
let pendingSessionResolve: ((r: { run: boolean; remember: boolean }) => void) | null = null;

/** UI 在确认卡上点「确认」或「取消」后调用，让 agent 的工具调用链继续。 */
export function resolveCliTool(action: { run: boolean; remember: boolean }): void {
  if (pendingCliResolve) {
    const r = pendingCliResolve;
    pendingCliResolve = null;
    pendingCliTool = null;
    r(action);
  }
}

/** UI 在会话确认卡上点「确认」或「取消」后调用，让 agent 的工具调用链继续。 */
export function resolveSessionConfirm(action: { run: boolean; remember: boolean }): void {
  if (pendingSessionResolve) {
    const r = pendingSessionResolve;
    pendingSessionResolve = null;
    pendingSessionConfirm = null;
    r(action);
  }
}

/**
 * L2 授权操作的统一会话确认门（插件管理 / 电源计划等）。
 * 权限中心开启后仍需会话确认（弹确认卡），勾过「本次会话免确认」后
 * 本会话直接执行。确认后由调用方真正执行并返回结果，保证 agent 的
 * 工具调用链不断（与 runHardwareTest 同一模式）。
 */
async function promptSessionConfirm(
  permId: PermId,
  title: string,
  target: string,
  detail: string,
): Promise<{ granted: boolean; remembered: boolean }> {
  if (!isPermEnabled(permId)) {
    return { granted: false, remembered: false };
  }
  if (hasSessionAuth(permId)) {
    return { granted: true, remembered: false };
  }
  const decision: { run: boolean; remember: boolean } = await new Promise((resolve) => {
    pendingSessionConfirm = { permId, title, target, detail };
    pendingSessionResolve = resolve;
  });
  if (!decision.run) return { granted: false, remembered: false };
  if (decision.remember) grantSessionAuth(permId);
  return { granted: true, remembered: decision.remember };
}

/** UI 在确认面板上点「确认」或「取消」后调用，让 agentChat 的工具调用链继续。 */
export function resolveStress(action: { run: boolean; remember: boolean }): void {
  if (pendingStressResolve) {
    const r = pendingStressResolve;
    pendingStressResolve = null;
    pendingStress = null;
    r(action);
  }
}

// —— pending 状态访问器：agentChat 开始时清空、结束时取走，
// 让 agent 层不需要直接摸这些模块私有变量。

export function resetAgentPending(): void {
  pendingProposal = null;
  pendingCliTool = null;
  pendingCliResolve = null;
  pendingStress = null;
  pendingStressResolve = null;
  pendingSessionConfirm = null;
  pendingSessionResolve = null;
}

export function takeProposal(): AgentProposal | null {
  const p = pendingProposal;
  pendingProposal = null;
  return p;
}

// CLI 确认已改为挂起 + resolveCliTool() 回填（与压测/会话确认同模式），
// agentChat 不再取走；保留 takeCliTool 仅防残留。
export function takeCliTool(): PendingCliTool | null {
  const t = pendingCliTool;
  pendingCliTool = null;
  return t;
}

/** 只看不取：压测确认由 UI 回调 resolveStress() 清，agentChat 不重复清空。 */
export function peekStress(): PendingStress | null {
  return pendingStress;
}

/** 只看不取：会话确认由 UI 回调 resolveSessionConfirm() 清。 */
export function peekSessionConfirm(): PendingSessionConfirm | null {
  return pendingSessionConfirm;
}

// 工具箱工具索引（分类 / 工具名 —— 简介），懒加载缓存：agentChat 首轮注入
// system prompt，让模型知道有哪些 CLI 工具可用、且用法要先查后跑。
// 无可用工具时返回 null，调用方不注入（避免把"未安装"噪音塞进提示词）。
let cliIndexPromise: Promise<string | null> | null = null;

// Tool Manifest 懒加载缓存（W3）：AI 路由/执行时按工具名查能力说明书。
// 与 toolbeltStatus 的索引分开缓存——manifest 是独立后端调用，数据更全。
let manifestsPromise: Promise<ToolManifest[]> | null = null;

function getManifests(): Promise<ToolManifest[]> {
  if (manifestsPromise) return manifestsPromise;
  manifestsPromise = api.toolbeltManifests().catch(() => [] as ToolManifest[]);
  return manifestsPromise;
}

async function getManifestFor(tool: string): Promise<ToolManifest | undefined> {
  const all = await getManifests();
  const q = tool.trim().toLowerCase();
  if (!q) return undefined;
  return all.find(
    (m) => {
      const n = m.name.toLowerCase();
      return n === q || n.includes(q) || q.includes(n);
    },
  );
}

export async function getCliIndexContext(): Promise<string | null> {
  if (cliIndexPromise) return cliIndexPromise;
  cliIndexPromise = (async () => {
    try {
      const st = await api.toolbeltStatus();
      if (!st.tools || st.tools.length === 0) return null;
      const groups = new Map<string, string[]>();
      for (const t of st.tools) {
        const line = t.installed
          ? `- ${t.name} —— ${t.description}`
          : `- ${t.name} —— ${t.description}（未安装）`;
        if (!groups.has(t.category)) groups.set(t.category, []);
        groups.get(t.category)!.push(line);
      }
      const root = st.tools_root ? `工具箱 Tools 目录：\`${st.tools_root}\`` : '';
      // W3：注入 manifest 索引（含风险级/权限级），替代纯名称列表——
      // 模型先按 risk 决定要不要问用户，再按需调 get_cli_tool_usage 查参数。
      const ms = await getManifests();
      return ['## 工具箱命令行工具（可用 run_cli_tool 执行）', root,
        ...Array.from(groups.entries()).map(([cat, lines]) => `### ${cat}\n${lines.join('\n')}`),
        '',
        '### 风险等级（risk/权限级，L3 不可执行）',
        ...ms.map((m) => `- ${m.name}：${m.risk}/${m.permission_level}${m.invocation.mode === 'gui' ? '（GUI，仅可双击启动）' : ''}`),
      ].filter(Boolean).join('\n\n');
    } catch { /* 预览/非 Tauri 环境 */ }
    return null;
  })();
  return cliIndexPromise;
}

// 实际执行 toolbelt_run(confirmed=true) 并拼报告（execCliTool 确认后 / UI 直调共用）。
async function doCliRun(tool: string, argv: string[], timeoutSecs: number | undefined, manifest?: ToolManifest): Promise<string> {
  const report = await api.toolbeltRun(tool, argv, timeoutSecs, true);
  const o = report.outcome;
  const head = [`工具「${report.command_line}」执行完成`,
    `退出码：${o.exit_code ?? '（超时被终止）'}${o.timed_out ? '（超时）' : ''}`,
  ];
  const parser = manifest?.output?.parser ?? 'stdout';
  if (o.stdout.trim() && parser !== 'stdout') {
    try {
      const parsed = JSON.parse(o.stdout.trim());
      head.push(`结构化输出（${parser}）：`);
      head.push(JSON.stringify(parsed, null, 2).slice(0, 3000));
    } catch {
      head.push(o.stdout.trim().slice(0, 3000));
    }
  } else {
    head.push(o.stdout.trim() ? o.stdout.trim().slice(0, 3000) : '');
  }
  head.push(o.stderr.trim() ? `[stderr] ${o.stderr.trim().slice(0, 1000)}` : '');
  return head.filter(Boolean).join('\n');
}

// 图吧工具箱工具名 → 后端 toolbelt_run 的参数（未确认前走 false 触发确认门）。
// 安全接线（W3）：
//   1. 权限门：cli.run 未开启（L1 权限中心关闭）→ 直接拒绝，绝不执行；
//   2. Manifest 校验：L3 永久禁止（如 FPT64 BIOS 刷写）→ 直接拒绝；
//   3. 按 manifest.output.parser 把 stdout 转结构化 JSON，便于 AI 解读。
async function execCliTool(
  tool: string,
  rawArgs: string,
  timeoutSecs: number | undefined,
  manifest?: ToolManifest,
  o?: AgentOpts,
): Promise<string> {
  if (!isPermEnabled('cli.run')) {
    return '运行工具箱 CLI 工具未授权：请前往「设置 → AI 权限中心」开启「运行工具箱 CLI 工具」后重试。';
  }
  const m = manifest ?? (await getManifestFor(tool));
  if (m?.permission_level === 'L3') {
    return `「${tool}」为 L3 永久禁止操作（${m.side_effects || '高危写入'}），不可解锁执行。`;
  }
  // 与后端 parse_args 同语义的切词：双引号内保留空格、支持 \" 转义。
  // AI 传 `WizTree "C:\My Files" /export=x` 这类带引号参数时不再被拆碎。
  const argv = rawArgs.trim() ? splitArgs(rawArgs) : [];
  // 会话免确认：用户在确认卡上勾过「本次会话免确认」后，中/高风险工具
  // 直接带 confirmed=true 执行（用户已经为本会话决定过是否信任这个工具）。
  const sessionGranted = hasSessionAuth('cli.run');
  try {
    const report = await api.toolbeltRun(tool, argv, timeoutSecs, sessionGranted);
    const o = report.outcome;
    const head = [`工具「${report.command_line}」执行完成`,
      `退出码：${o.exit_code ?? '（超时被终止）'}${o.timed_out ? '（超时）' : ''}`,
    ];
    if (o.stdout.trim() && m?.output.parser && m.output.parser !== 'stdout') {
      // 结构化解析：smart_csv / kv_lines → JSON，比原始文本更利于 AI 解读
      try {
        const parsed = JSON.parse(o.stdout.trim());
        head.push(`结构化输出（${m.output.parser}）：`);
        head.push(JSON.stringify(parsed, null, 2).slice(0, 3000));
      } catch {
        head.push(o.stdout.trim().slice(0, 3000));
      }
    } else {
      head.push(o.stdout.trim() ? o.stdout.trim().slice(0, 3000) : '');
    }
    head.push(o.stderr.trim() ? `[stderr] ${o.stderr.trim().slice(0, 1000)}` : '');
    return head.filter(Boolean).join('\n');
  } catch (e) {
    // 后端 toolbelt_run 以稳定前缀 `toolbelt:confirm:` 标记中/高风险确认门；
    // 命中即挂起等 UI 确认（与 runHardwareTest / promptSessionConfirm 同模式），
    // 确认后真实执行并把 stdout 作为 tool_result 回填给模型——不让模型以为
    // 「已发确认请求就没下文」。
    if (api.isToolbeltConfirmError(e)) {
      const msg = String(e);
      const usage = await api.toolbeltUsage(tool).catch(() => null);
      pendingCliTool = {
        tool,
        args: argv,
        timeoutSecs,
        risk: usage?.risk ?? 'medium',
        reason: msg.slice(0, 300),
      };
      // 同步把确认卡交 UI 弹出来（setState 放微任务，agent 的 await 不会立即
      // 返回，UI 有空渲染确认卡再 resolve）。
      o?.onToolConfirm?.(pendingCliTool);
      const decision = await new Promise<{ run: boolean; remember: boolean }>((resolve) => {
        pendingCliResolve = resolve;
      });
      if (!decision.run) return '用户取消了该工具的执行，未运行。';
      if (decision.remember) grantSessionAuth('cli.run');
      return doCliRun(tool, argv, timeoutSecs, m);
    }
    return `执行失败：${String(e).slice(0, 300)}`;
  }
}

// 硬件压测（L1 受控）：权限未开 → 直接拒绝；已开但无会话授权 → 弹确认面板
// 等用户确认后再调后端 hw_run_test(confirmed=true)。确认后可选「本次会话免确认」。
async function runHardwareTest(args: Record<string, unknown>): Promise<string> {
  if (!isTauri) return '硬件压测仅在桌面端可用';
  const testType = String(args.testType ?? '').trim();
  if (!testType) return '请提供 testType（cpu_stress / gpu_stress / gpu_benchmark / cpu_benchmark）';
  if (!isPermEnabled('hw.stress')) {
    return '压力测试未授权：请前往「设置 → AI 权限中心」开启「压力测试」后重试。';
  }
  const durationSecs = typeof args.durationSeconds === 'number' && args.durationSeconds > 0
    ? Math.round(args.durationSeconds) : undefined;
  const tempLimit = typeof args.tempLimitCelsius === 'number' ? args.tempLimitCelsius : undefined;
  const label = testType === 'cpu_stress' ? 'CPU 烤机（Prime95）'
    : testType === 'gpu_stress' ? 'GPU 烤机（FurMark）'
    : testType === 'gpu_benchmark' ? 'GPU 基准（FurMark）'
    : testType === 'cpu_benchmark' ? 'CPU 基准（AIDA64）'
    : testType;

  if (hasSessionAuth('hw.stress')) {
    return executeHardwareTest(testType, durationSecs, tempLimit);
  }

  // 无会话授权：弹确认面板，等用户在 UI 里点「确认」或「取消」。
  const action: { run: boolean; remember: boolean } = await new Promise((resolve) => {
    pendingStress = {
      testType,
      durationSeconds: durationSecs ?? 300,
      tempLimitCelsius: tempLimit ?? 90,
      label,
      tool: testType === 'cpu_benchmark' ? 'AIDA64' : 'FurMark',
    };
    pendingStressResolve = resolve;
  });
  if (!action.run) return '用户取消了压力测试，未执行。';
  if (action.remember) grantSessionAuth('hw.stress');
  return executeHardwareTest(testType, durationSecs, tempLimit);
}

async function executeHardwareTest(testType: string, durationSecs?: number, tempLimit?: number): Promise<string> {
  const report = await api.hwRunTest(testType, durationSecs, tempLimit, true);
  // 后端 stop_reason 是英文枚举，报告里映射成中文，避免用户看到生词。
  const reasonLabel: Record<string, string> = {
    finished: '自然结束',
    user_stopped: '用户停止',
    deadline: '到达设定时长',
    temp_limit: '温度熔断',
    crashed: '异常退出',
  };
  const lines = [
    `已启动：\`${report.command_line}\``,
    `测试类型：${report.test_type} · 工具：${report.tool}`,
    `实际运行：${report.duration_run_secs} 秒 · 结束原因：${reasonLabel[report.stop_reason] ?? report.stop_reason}`,
    `退出码：${report.exit_code ?? '（被强杀）'}`,
    `温度峰值：${report.temp_peak_c != null ? `${report.temp_peak_c}℃` : '（传感器不可读，熔断未生效）'}`,
    `温度熔断：${report.temp_monitor_active ? '已启用' : '未启用（传感器读不到）'}`,
    report.note,
  ];
  if (report.bench_excerpt) {
    lines.push('—— 基准结果 ——', report.bench_excerpt.slice(0, 1500));
  }
  return lines.join('\n');
}

export async function execTool(name: string, args: Record<string, unknown>, o: AgentOpts): Promise<string> {
  try {
    // 注册表查表分发；propose_cleanup 是 propose_cleanup_plan 的历史别名。
    const def = name === 'propose_cleanup'
      ? toolRegistry.find((t) => t.name === 'propose_cleanup_plan')
      : toolRegistry.find((t) => t.name === name)
        ?? dynamicMcpTools.find((t) => t.name === name);
    if (!def) return `未知工具: ${name}`;
    return await def.execute(args, o);
  } catch (e) {
    return `工具执行失败: ${String(e)}`;
  }
}

export function buildToolDefs(): { openai: unknown[]; anthropic: unknown[]; gemini: unknown[] } {
  const openai: unknown[] = [];
  const anthropic: unknown[] = [];
  const gemini: unknown[] = [];
  const add = (name: string, description: string, properties: object, required: string[]) => {
    openai.push({ type: 'function', function: { name, description, parameters: { type: 'object', properties, required } } });
    anthropic.push({ name, description, input_schema: { type: 'object', properties, required } });
    gemini.push({ name, description, parameters: { type: 'object', properties, required } });
  };
  // 遍历注册表生成各 provider 的 tool defs：开关（webGated / tauriOnly）在
  // 注册时声明，新增工具自动出现在所有 provider，无需再逐处 add。
  // 用户 MCP 服务器工具（dynamicMcpTools）也一并暴露给模型。
  for (const t of [...toolRegistry, ...dynamicMcpTools]) {
    if (t.webGated && !prefs.webEnabled) continue;
    if (t.tauriOnly && !isTauri) continue;
    add(t.name, t.description, t.properties, t.required);
  }
  return { openai, anthropic, gemini };
}
