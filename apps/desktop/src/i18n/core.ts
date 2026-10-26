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

/** <html lang> 的取值：中文用 zh-CN、英文用 en（读屏软件与拼写检查都靠它）。 */
export function htmlLangOf(lang: Lang): string {
  return lang === 'zh' ? 'zh-CN' : 'en';
}

/** 把当前生效语言同步到 <html lang>——切到英文后读屏软件不该再拿中文音去念英文。 */
function syncDocumentLang() {
  if (typeof document === 'undefined') return;
  document.documentElement.lang = htmlLangOf(effectiveLang());
}

function emit() {
  syncDocumentLang();
  for (const l of listeners) l();
}

// 首次加载也同步一次：index.html 里写死的是 zh-CN，「跟随系统」为英文时得改过来。
syncDocumentLang();

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

// R350：英文单复数。en 表里写 `{n:day|days}`——n 为 1 时出 “1 day”，其余出 “3 days”，
// 于是不用再手写 “day(s)” 这种两头不讨好的 hack。中文表不写这个语法
// （「1 天 / 3 天」不分单复数），t() 对两种写法一视同仁。
const PLURAL_RE = /\{(\w+):([^}|]*)\|([^}|]*)\}/g;

/** 把数量合成带单位的短语；“1 day” / “0 days” / “3 days”。单/复数文案留空时只给数字。 */
export function formatPluralUnit(n: number, one: string, other: string): string {
  const form = n === 1 ? one : other;
  return form === '' ? String(n) : `${n} ${form}`;
}

/** 取文案：en 缺失时回退中文原文（默认中文零回归）。支持 {name} 占位符替换与 {n:one|other} 复数。 */
export function t(key: string, params?: Record<string, string | number>): string {
  const dict = DICTS[effectiveLang()] ?? ZH;
  let v = dict[key] ?? ZH[key] ?? key;
  if (params) {
    for (const [k, val] of Object.entries(params)) {
      v = v.split(`{${k}}`).join(String(val));
    }
    v = v.replace(PLURAL_RE, (m, k: string, one: string, other: string) => {
      const raw = params[k];
      // R471：参数没传就保留原样（开发期一眼能看见漏传）；但「传了、只是不是数字」
      // （例如调用方已把数字本地化成「1,234」）必须按复数渲染 —— 否则 {n:x|xs}
      // 会原样漏到界面上，比单复数用错还难看。
      if (raw === undefined || raw === null) return m;
      const n = typeof raw === 'number' ? raw : Number(raw);
      if (Number.isFinite(n)) return formatPluralUnit(n, one, other);
      const text = String(raw);
      return other === '' ? text : text + ' ' + other;
    });
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
