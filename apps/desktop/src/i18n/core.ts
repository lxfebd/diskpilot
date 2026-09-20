// ── 国际化 i18n 运行时（R9）────────────────────────────────────────
// 轻量文案表 + useT()：切换语言即时生效（useSyncExternalStore 驱动，不刷新）。
// 设计约束（铁律：默认中文零回归）：
// - 默认语言 = 中文；未翻译的 key 一律回退中文原文，绝不显示 key 本身。
// - 后端错误文案保持中文（工具输出语义化在前端翻译，见 advisor/tools.ts）。
// - 语言选择持久化到 localStorage；「跟随系统」读 navigator.language。
// - 文案按界面表面分命名空间放在 ./namespaces/，这里只做合并与查表。
import { useMemo, useSyncExternalStore } from 'react';
import { NAMESPACES } from './namespaces';

export type Lang = 'zh' | 'en';

type Dict = Record<string, string>;

const ZH: Dict = {};
const EN: Dict = {};
for (const ns of NAMESPACES) {
  Object.assign(ZH, ns.zh);
  Object.assign(EN, ns.en);
}

const DICTS: Record<Lang, Dict> = { zh: ZH, en: EN };

const STORAGE_KEY = 'diskpilot.lang.v1';

function detectSystemLang(): Lang {
  if (typeof navigator !== 'undefined') {
    const nl = navigator.language?.toLowerCase() ?? '';
    if (nl.startsWith('zh')) return 'zh';
  }
  return 'en';
}

function loadStored(): Lang | 'system' {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === 'en' || raw === 'zh') return raw;
    if (raw === 'system') return 'system';
  } catch { /* localStorage 不可用时静默跟随系统 */ }
  return 'system';
}

// 模块级状态：语言选择 + 生效语言。用 useSyncExternalStore 让所有
// 调用 useT() 的组件在切换瞬间重渲染（不刷新全量生效）。
let mode: 'system' | Lang = loadStored();
const listeners = new Set<() => void>();

function effectiveLang(): Lang {
  return mode === 'system' ? detectSystemLang() : mode;
}

function emit() {
  for (const l of listeners) l();
}

/** 切换语言（'zh' | 'en' | 'system'），持久化 + 通知所有组件。 */
export function setLang(next: 'zh' | 'en' | 'system') {
  mode = next;
  try { localStorage.setItem(STORAGE_KEY, next); } catch { /* 静默 */ }
  emit();
}

/** 当前语言选择（含 'system'）。 */
export function getLangMode(): 'system' | Lang {
  return mode;
}

/** 当前生效语言（system 时已解析为具体语言）。 */
export function getLang(): Lang {
  return effectiveLang();
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => { listeners.delete(cb); };
}

/** 取文案：en 缺失时回退中文原文（默认中文零回归）。支持 {name} 占位符替换。 */
export function t(key: string, params?: Record<string, string | number>): string {
  const dict = DICTS[effectiveLang()] ?? ZH;
  let v = dict[key] ?? ZH[key] ?? key;
  if (params) {
    for (const [k, val] of Object.entries(params)) {
      v = v.split(`{${k}}`).join(String(val));
    }
  }
  return v;
}

/**
 * 取**中文原文**，不受当前语言影响。
 * 只给「历史上拿文案做判定」的地方用（如权限说明里的「（即将上线）」标记）——
 * 判定条件应当逐步搬到结构化字段，不要拿它写新逻辑。
 */
export function zhText(key: string): string {
  return ZH[key] ?? key;
}

/**
 * React hook：返回当前语言绑定的取文案函数。
 * 语言切换时组件重渲染，且返回的函数**身份随之改变** —— 这样
 * `useMemo(() => t('...'), [t])` / `useCallback` / `memo` 的比较才会把
 * 语言变更算作依赖变化，切语言后 memo 出来的文案才会更新。
 */
export function useT(): (key: string, params?: Record<string, string | number>) => string {
  const lang = useSyncExternalStore(subscribe, getLang, getLang);
  // lang 不进闭包是故意的：它只负责在切语言时换掉函数身份，触发下游 memo/useMemo 更新。
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => (key: string, params?: Record<string, string | number>) => t(key, params), [lang]);
}

/** 守卫测试用：中文全集 / 英文全集（生产代码不要引用）。 */
export function zhKeys(): string[] {
  return Object.keys(ZH);
}
export function enKeys(): string[] {
  return Object.keys(EN);
}
