// Provider 层：AI 协议设置（localStorage 持久化）+ HTTP 通道
// （Tauri 走后端 ai_proxy 绕 CORS / 浏览器直连 fetch）+ 单轮聊天。
// Agent 多轮循环在 agent.ts，工具定义与执行在 tools.ts。
//
// Settings persist to localStorage under "diskpilot.advisor".

import { api } from '../api';
import { isTauri } from '../env';

export type Provider = 'openai' | 'anthropic' | 'gemini' | 'ollama';

export interface AdvisorSettings {
  provider: Provider;
  model: string;
  apiKey: string;
  baseUrl: string;
  /** 用户在 Settings 里手动指定的协议，覆盖 detectProvider 的自动判断；不存在则走自动识别。 */
  providerOverride?: Provider;
}

const STORAGE_KEY = 'diskpilot.advisor';

// 从 Base URL 猜协议，让用户不用管"协议"这个概念。覆盖的是用户实际会遇到的
// case；其余一律落到 OpenAI（中转 / 国产大模型事实上的通用标准）。
// 11434 是 Ollama 默认端口；本机跑的 OpenAI 兼容中转（one-api/new-api/vLLM/
// LM Studio 等，如 http://localhost:20128/v1）同样常绑在 localhost，不能靠
// "本地地址"判 ollama——之前 localhost/127.0.0.1 判据把这类中转错判成
// ollama，请求打到 /api/chat 404。误判后用户在 Settings 里手动指定协议兜底。
export function detectProvider(baseUrl: string): Provider {
  const u = baseUrl.toLowerCase();
  if (!u) return 'openai';
  if (u.includes('11434') || u.includes('/api/chat')) return 'ollama';
  // 识别带 anthropic 字样的代理子域名（如 anthropic.novadiffusion.com），
  // 不仅是官方 anthropic.com。误识别风险极小——OpenAI 协议代理几乎不会
  // 把 anthropic 写进域名里。
  if (u.includes('anthropic') || u.includes('/v1/messages')) return 'anthropic';
  if (u.includes('googleapis.com') || u.includes('generativelanguage')) return 'gemini';
  return 'openai';
}

export function loadSettings(): AdvisorSettings | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as AdvisorSettings;
    if (!parsed.provider || !parsed.model) return null;
    // 收紧 detectProvider 之前，本机中转（如 localhost:20128/v1）会被误判
    // 存成 provider: 'ollama'。没手动 override 的老数据在这里按新规则
    // 用 baseUrl 重判一次，让升级后的用户自动痊愈，不用手动去 Settings 改。
    if (!parsed.providerOverride) parsed.provider = detectProvider(parsed.baseUrl);
    return parsed;
  } catch {
    return null;
  }
}

export function saveSettings(s: AdvisorSettings) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
}

export function clearSettings() {
  localStorage.removeItem(STORAGE_KEY);
}

export function isConfigured(s: AdvisorSettings | null): s is AdvisorSettings {
  if (!s) return false;
  if (s.provider === 'ollama') return Boolean(s.model);
  return Boolean(s.apiKey && s.model);
}

export interface ChatImage {
  /** Full data URL (e.g. `data:image/png;base64,...`). */
  dataUrl: string;
  /** Mime type — `image/png`, `image/jpeg`, etc. Used by Anthropic / Gemini
   *  which need it as a separate field. */
  mimeType: string;
}

// agent 循环（agent.ts）也要把图片转 base64 塞进各家协议，导出共用。
export function dataUrlBase64(dataUrl: string): string {
  const i = dataUrl.indexOf(',');
  return i >= 0 ? dataUrl.slice(i + 1) : dataUrl;
}

// Tauri 生产模式下页面 origin 是 tauri.localhost，浏览器直接 fetch 外部
// AI API 会被 CORS 拦截（"TypeError: Failed to fetch"，但同地址 PowerShell
// 直连却通——纯粹是跨域）。统一经后端 ai_proxy 转发（reqwest 无 CORS），
// 返回结构对齐浏览器 Response 的核心字段，其余调用方代码不用改。
interface HttpResponseLike {
  ok: boolean;
  status: number;
  text(): Promise<string>;
  json(): Promise<any>;
}

// 部分中转 API 在正常 JSON 响应后还会拼接尾随内容（第二个 JSON / usage /
// 换行脏数据），JSON.parse 整串会报 "Unexpected non-whitespace character
// after JSON"。这里从第一个 `{`/`[` 起做括号深度扫描，只取第一个完整
// JSON 值，尾随内容直接丢弃——对标准单一 JSON 无副作用，对"双 JSON"响应
// 恢复可用。
export function parseFirstJson(text: string): any {
  const t = text.trimStart();
  const open = t[0];
  if (open === '{' || open === '[') {
    const close = open === '{' ? '}' : ']';
    let depth = 0;
    let inStr = false;
    let esc = false;
    for (let i = 0; i < t.length; i++) {
      const ch = t[i];
      if (inStr) {
        if (esc) esc = false;
        else if (ch === '\\') esc = true;
        else if (ch === '"') inStr = false;
        continue;
      }
      if (ch === '"') { inStr = true; continue; }
      if (ch === open) depth++;
      else if (ch === close) {
        depth--;
        if (depth === 0) return JSON.parse(t.slice(0, i + 1));
      }
    }
  }
  return JSON.parse(t);
}

async function aiRequest(url: string, init: RequestInit): Promise<HttpResponseLike> {
  // 超时兜底：AI 接口挂起时不能把 UI 永久锁在 busy。Tauri 模式把超时
  // 传给后端（reqwest 请求级 timeout）；浏览器模式用 AbortController 真正
  // 掐断 fetch。120s 覆盖大多数慢模型，agent 多轮各自独立计时。
  const timeoutMs = 120_000;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const origSignal = init.signal;
  const abortFromOutside = () => controller.abort();
  origSignal?.addEventListener('abort', abortFromOutside, { once: true });
  // 用户停止：Tauri 后端用 cancel_key 按 key 掐 reqwest（浏览器分支直接
  // 靠 AbortController 会中断下面的 fetch）。
  const cancelKey = `ai-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
  try {
    if (isTauri) {
      const headers: Record<string, string> = {};
      if (init.headers) {
        const h = init.headers as Record<string, string>;
        for (const k of Object.keys(h)) headers[k] = String(h[k]);
      }
      const bodyStr = typeof init.body === 'string' ? init.body : '';
      const promise = api.aiProxy(url, init.method ?? 'POST', headers, bodyStr, timeoutMs / 1000, cancelKey);
      // 外部 abort（用户点停止）→ 后端掐断请求，与浏览器版行为对齐
      const stopCancel = () => { void api.aiCancel(cancelKey).catch(() => {}); };
      origSignal?.addEventListener('abort', stopCancel, { once: true });
      try {
        const res = await promise;
        return {
          ok: res.status >= 200 && res.status < 300,
          status: res.status,
          text: async () => res.body,
          json: async () => parseFirstJson(res.body),
        };
      } finally {
        origSignal?.removeEventListener('abort', stopCancel);
      }
    }
    const res = await fetch(url, { ...init, signal: controller.signal });
    return {
      ok: res.ok,
      status: res.status,
      text: () => res.text(),
      json: () => res.json(),
    };
  } catch (e) {
    if (controller.signal.aborted) {
      throw new Error(`AI 请求超时（${timeoutMs / 1000}s）或已取消`);
    }
    throw e;
  } finally {
    clearTimeout(timer);
    origSignal?.removeEventListener('abort', abortFromOutside);
  }
}

// Anthropic 响应里 content 是 block 数组，extended-thinking 模型（如 DeepSeek
// 的 anthropic 兼容端点）会先返一个 {type:"thinking",...} 再返 {type:"text",...}，
// 不能假设 content[0] 是 text。stop_reason="max_tokens" 时还可能根本没 text
// block（thinking 把额度吃光），给明确错误而不是静默返回空串。
export function extractAnthropicText(data: unknown): string {
  const d = data as { content?: Array<{ type?: string; text?: string }>; stop_reason?: string };
  const blocks = d?.content ?? [];
  const text = blocks
    .filter((b) => b?.type === 'text')
    .map((b) => b?.text ?? '')
    .join('')
    .trim();
  if (!text) {
    const stop = d?.stop_reason ?? 'unknown';
    if (stop === 'max_tokens') {
      throw new Error('AI 在 thinking 阶段被截断（max_tokens 太小，思考把额度吃光了）。把 max_tokens 调大重试。');
    }
    throw new Error(`Anthropic: 没拿到 text block（stop_reason=${stop}）`);
  }
  return text;
}

// 聊天主 system prompt：freeChat（本文件）与 agentChat（agent.ts 的默认
// system）共用，所以放 provider 层导出。
export const CHAT_SYSTEM = `你是 DiskPilot 的 AI 磁盘顾问，帮用户搞清楚磁盘上的文件夹是什么、能不能删。
实事求是原则（最重要）：
- 只根据提供的元数据和工具返回的结果下结论；绝对不编造文件名、路径、软件行为或网络信息。
- 拿不准就先调用工具核实，再回答：list_dir / path_size 查本机真实情况，get_disk_health 读实时磁盘容量，get_cleanup_suggestions 看后端算好的清理建议，get_system_info 查系统配置，web_search 查不确定或较新的客观事实（软件是什么、缓存机制、最新版本行为等）。
- 问"磁盘还剩多少空间 / 哪块盘满了 / 该清理什么"时，先调 get_disk_health / get_cleanup_suggestions 拿真实数字，再回答。
- 引用联网结果时给出来源链接；把「已核实的事实」和「你的推测」分开说。
- 建议删除时说清楚删哪个范围、用什么方式（回收站 / 手动整理 / 卸载应用）；绝不建议对系统路径跑 rm -rf。
- 你不能直接删除、回收、移动任何文件。你的职责是分析并输出清理建议清单（propose_cleanup_plan），由用户逐项确认后才执行。用户明确想清理 / 释放空间时，先核实路径，再调用 propose_cleanup_plan 生成清单。清单里每项必须写清：是什么 / 干什么用的 / 删了会怎样，并标注风险等级（safe/caution/danger）。危险路径（盘根、Windows、Program Files、用户主目录）后端会拦截，切勿尝试规避；系统目录、软件安装目录、用户文档/照片/下载一律不要列入。
- 中文回答，简洁（2-5 句），列表优先。`

const OVERVIEW_SYSTEM = `You are DiskPilot's AI advisor. The user just finished scanning their disk. You receive a JSON summary of the largest folders. Write a friendly Chinese overview (~180-220 字) covering, in order, with empty lines between sections:

【整体】 一句话概括磁盘的整体结构（操作系统 / 用户数据 / 应用 各占多少）。

【这里都有什么】 点名 4-6 个最大的目录，每个一行：名字、大小、大致是什么 / 哪个软件的。要具体到软件名（例：WeChat Files = 微信聊天记录、node_modules = npm 包、HuggingFace = 模型权重）。

【可以删的】 直接列出 2-4 项可以删 / 可以清理的东西，每条说清楚 ① 路径或名字 ② 删了会怎样 ③ 怎么删（回收 / 卸载 / 跑脚本）。如果某个东西看起来可以删但有风险，就不要列在这里。

【不要动】 简短提一下扫描里看到的不该动的东西（系统目录 / 用户文档），一行带过。

口语化中文，不要 markdown bullet（用纯文本换行就行），不要客套话。`;

export async function overviewChat(summary: object, signal?: AbortSignal): Promise<string> {
  return runChatRaw(OVERVIEW_SYSTEM, JSON.stringify(summary, null, 2), undefined, signal);
}

export async function freeChat(
  context: string,
  userMessage: string,
  images?: ChatImage[],
): Promise<string> {
  const userText = context ? `${context}\n\n用户的问题：${userMessage}` : userMessage;
  return runChatRaw(CHAT_SYSTEM, userText, images);
}

async function runChatRaw(system: string, user: string, images?: ChatImage[], signal?: AbortSignal): Promise<string> {
  const settings = loadSettings();
  if (!isConfigured(settings)) {
    throw new Error('AI 未配置 — 在右上角的设置里填一个 API key');
  }
  const fullUser = user;
  const imgs = images ?? [];

  if (settings.provider === 'openai') {
    const url = (settings.baseUrl || 'https://api.openai.com/v1').replace(/\/$/, '');
    const userContent: unknown = imgs.length === 0
      ? fullUser
      : [
          { type: 'text', text: fullUser },
          ...imgs.map((img) => ({ type: 'image_url', image_url: { url: img.dataUrl } })),
        ];
    const r = await aiRequest(`${url}/chat/completions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${settings.apiKey}` },
      signal,
      body: JSON.stringify({
        model: settings.model,
        messages: [
          { role: 'system', content: system },
          { role: 'user', content: userContent },
        ],
      }),
    });
    if (!r.ok) throw new Error(`OpenAI ${r.status}: ${await r.text()}`);
    const data = await r.json();
    return data?.choices?.[0]?.message?.content?.trim() ?? '';
  }
  if (settings.provider === 'anthropic') {
    const url = (settings.baseUrl || 'https://api.anthropic.com').replace(/\/$/, '');
    const userContent: unknown = imgs.length === 0
      ? fullUser
      : [
          ...imgs.map((img) => ({
            type: 'image',
            source: { type: 'base64', media_type: img.mimeType, data: dataUrlBase64(img.dataUrl) },
          })),
          { type: 'text', text: fullUser },
        ];
    const r = await aiRequest(`${url}/v1/messages`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'x-api-key': settings.apiKey,
        'anthropic-version': '2023-06-01',
        'anthropic-dangerous-direct-browser-access': 'true',
      },
      signal,
      body: JSON.stringify({
        model: settings.model,
        max_tokens: 4096,
        system,
        messages: [{ role: 'user', content: userContent }],
      }),
    });
    if (!r.ok) throw new Error(`Anthropic ${r.status}: ${await r.text()}`);
    const data = await r.json();
    return extractAnthropicText(data);
  }
  if (settings.provider === 'gemini') {
    const url = (settings.baseUrl || 'https://generativelanguage.googleapis.com').replace(/\/$/, '');
    const parts: unknown[] = [{ text: fullUser }];
    for (const img of imgs) {
      parts.push({ inline_data: { mime_type: img.mimeType, data: dataUrlBase64(img.dataUrl) } });
    }
    const r = await aiRequest(
      `${url}/v1beta/models/${encodeURIComponent(settings.model)}:generateContent?key=${encodeURIComponent(settings.apiKey)}`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        signal,
        body: JSON.stringify({
          systemInstruction: { parts: [{ text: system }] },
          contents: [{ role: 'user', parts }],
          generationConfig: { temperature: 0.4 },
        }),
      },
    );
    if (!r.ok) throw new Error(`Gemini ${r.status}: ${await r.text()}`);
    const data = await r.json();
    return data?.candidates?.[0]?.content?.parts?.[0]?.text?.trim() ?? '';
  }
  // ollama — uses `images` field (array of base64) on the message.
  const url = (settings.baseUrl || 'http://localhost:11434').replace(/\/$/, '');
  const userMsg: Record<string, unknown> = { role: 'user', content: fullUser };
  if (imgs.length > 0) userMsg.images = imgs.map((i) => dataUrlBase64(i.dataUrl));
  const r = await aiRequest(`${url}/api/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    signal,
    body: JSON.stringify({
      model: settings.model,
      stream: false,
      messages: [
        { role: 'system', content: system },
        userMsg,
      ],
    }),
  });
  if (!r.ok) throw new Error(`Ollama ${r.status}: ${await r.text()}`);
  const data = await r.json();
  return data?.message?.content?.trim() ?? '';
}

// agent 循环（agent.ts）逐轮请求各家 API 用的 JSON 通道，错误信息带截断正文。
export async function fetchJson(url: string, init: RequestInit): Promise<any> {
  const r = await aiRequest(url, init);
  const text = await r.text();
  if (!r.ok) throw new Error(`HTTP ${r.status}: ${text.slice(0, 300)}`);
  try { return parseFirstJson(text); } catch { throw new Error(`响应不是 JSON: ${text.slice(0, 200)}`); }
}

/**
 * 判定错误是否表示「该模型不支持工具调用」（能力缺失，可退化为无工具重试）。
 *
 * 单源判定，三处 agent 循环共用。只认 400/422 状态码 + 响应体同时提到
 * tools/function 与「不支持」否定词——不靠任意响应体里出现 "tool" 就降级，
 * 否则会把参数格式错误 / tool_call_id 缺失 / 认证失败等真实 4xx 误判成
 * 能力缺失静默吞掉。各家服务商的不支持文案：OpenAI `'tools' is not
 * supported by this model`、Anthropic `does not support tool use`、
 * Gemini `Function calling is not enabled`，均命中。
 */
export function isToolsUnsupportedError(e: unknown): boolean {
  const msg = String(e ?? '');
  const m = /^HTTP (\d{3})/.exec(msg);
  if (!m || (m[1] !== '400' && m[1] !== '422')) return false;
  return /tool|function/i.test(msg) && /not\s+(support|enabled|available)|unsupported/i.test(msg);
}
