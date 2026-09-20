import { invoke } from '@tauri-apps/api/core';
import { isTauri } from '../env';
import { t } from '../i18n';

/**
 * AI 域：联网搜索 / 服务商代理 / 扫描树上下文摘要。
 * AI 的密钥与脱敏策略在后端 ai.rs 收口，前端只做 invoke 透传。
 * 由 api.ts 聚合进 `api` 对象，保持 `api.webSearch()` 等调用面不变。
 */
export const aiApi = {
  webSearch: (query: string) =>
    isTauri
      ? invoke<WebSearchHit[]>('web_search', { query })
      : Promise.resolve([] as WebSearchHit[]),

  aiProxy: (url: string, method: string, headers: Record<string, string>, body: string, timeoutSecs?: number, cancelKey?: string) =>
    isTauri
      ? invoke<{ status: number; body: string }>('ai_proxy', {
          url,
          method,
          headers,
          body,
          timeoutSecs: timeoutSecs ?? null,
          cancelKey: cancelKey ?? null,
        })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'ai_proxy' }))),

  aiCancel: (cancelKey: string) =>
    isTauri
      ? invoke<void>('ai_cancel', { cancelKey })
      : Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'ai_cancel' }))),

  // AI 总览上下文：Rust 端从最近一棵扫描树算结构化摘要（Top 目录/文件/可回收字节）
  chatScanContext: (topN?: number) =>
    isTauri
      ? invoke<ChatScanContext | null>('chat_scan_context', { topN: topN ?? null })
      : Promise.resolve(null),
};

/** Rust 端 chat_scan_context 返回：最近一棵扫描树的结构化 AI 上下文 */
export interface ChatScanContext {
  root: string;
  root_name: string;
  total_size: number;
  total_files: number;
  top_entries: { path: string; name: string; size: number; is_dir: boolean; depth: number }[];
  top_dirs: { path: string; name: string; size: number; is_dir: boolean; depth: number }[];
  reclaimable_bytes: number;
}

export interface WebSearchHit {
  title: string;
  url: string;
  snippet: string;
}
