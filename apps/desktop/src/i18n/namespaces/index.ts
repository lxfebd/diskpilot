// ── 命名空间注册表 ─────────────────────────────────────────────────
// 一个界面表面 = 一个命名空间文件（同文件里 zh/en 并排，避免两本字典走偏）。
//
// 加文案：在对应命名空间同时补 zh 与 en，键前缀必须等于命名空间名（守卫测试强制），
// 然后在这里 import + 塞进 NAMESPACES。zh/en 键集不一致 = 测试红。
//
// 用法：组件里 `const t = useT()`（语言切换会自动重渲染）；非组件模块（常量表、
// 纯函数）`import { t } from '../i18n'` 并**在调用点求值**——模块顶层求值会把中文
// 烤进首次 import，切换语言不再生效，所以常量表要改成函数。
//
// 不译的东西（保持中文，属设计而非欠账）：
// - 发给模型的 prompt / 工具描述与工具返回文本（advisor/**、agent-server 侧）；
// - mocks.ts（浏览器 mock 后端，与真后端同口径）；
// - 后端 error string 的原文与任何用于匹配的字符串常量（如 `scan:cancelled:` 前缀）。
import { chat } from './chat';
import { cleanup } from './cleanup';
import { common } from './common';
import { errors } from './errors';
import { hw } from './hw';
import { mcp } from './mcp';
import { overview } from './overview';
import { perm } from './perm';
import { settings } from './settings';
import { shell } from './shell';
import { steam } from './steam';
import { studio } from './studio';
import { system } from './system';
import { theme } from './theme';
import { toolbelt } from './toolbelt';

export interface Namespace {
  /** 命名空间前缀，用于守卫测试校验键名前缀一致 */
  ns: string;
  zh: Record<string, string>;
  en: Record<string, string>;
}

export const NAMESPACES: Namespace[] = [
  chat, cleanup, common, errors, hw, mcp, overview, perm, settings, shell, steam, studio, system, theme, toolbelt,
];
