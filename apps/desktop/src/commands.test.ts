// R460：Rust 命令面与前端 invoke 的契约对偶守卫。
//
// 三类漂移都只在运行时炸、编译期不报：
// 1. 加了 #[tauri::command] 却忘了挂进 lib.rs 的 generate_handler!——宏照样生成包装
//    函数，cargo 编译通过，前端一调就是 command not found；
// 2. 前端 invoke 的名字拼错（或 Rust 侧改名后忘了同步）——同样只在用户点下去那一刻失败；
// 3. Rust 命令没人调——改名/重构后留下的死命令与死代码。
//
// 所以这里对三张清单做双向相等断言：声明（#[tauri::command]）⇄ 注册（generate_handler!）
// ⇄ 前端字面量调用（api.ts / api/*.ts 里的 invoke(名字)）。
// 动态拼名字的 invoke（invoke(cmd, ...)）读不出版本，本守卫只钉字面量；真要引入就在
// ALLOW_UNUSED 旁边写清原因，别把守卫改成恒真。
import { describe, expect, it } from 'vitest';

// 源码一律用 ?raw 抓取：src 下的测试被 tsc -b 纳入类型检查，不能引 node:fs（见
// advisor/toolContract.test.ts 的同款口径）。glob 的键是相对本文件的路径。
const RUST_FILES = import.meta.glob('../src-tauri/src/**/*.rs', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

const API_FILES = import.meta.glob(['./api.ts', './api/**/*.ts'], {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

/** 所有 #[tauri::command] 标记的函数名。属性行（#[allow(...)] / #[cfg(windows)]）与
 *  注释行可以夹在标记与签名之间：只看第一条非属性、非注释行，最多看 12 行。 */
function declaredCommands(): string[] {
  const out: string[] = [];
  for (const src of Object.values(RUST_FILES)) {
    const lines = src.split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
      if (!lines[i].includes('#[tauri::command]')) continue;
      for (let j = i + 1; j < Math.min(i + 12, lines.length); j++) {
        const line = lines[j].trim();
        if (line === '' || line.startsWith('#') || line.startsWith('//')) continue;
        const m = /(?:pub(?:\(crate\))?\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/.exec(line);
        if (m) out.push(m[1]);
        break;
      }
    }
  }
  return out;
}

/** lib.rs 里 generate_handler![...] 的清单（条目形如 scan::scan_path 或 scan_path）。 */
function registeredCommands(): string[] {
  const libKey = Object.keys(RUST_FILES).find((k) => k.endsWith('/lib.rs')) ?? '';
  const lib = RUST_FILES[libKey] ?? '';
  expect(libKey).toContain('src-tauri/src/lib.rs');
  const at = lib.indexOf('generate_handler!');
  const open = lib.indexOf('[', at);
  let depth = 0;
  let end = -1;
  for (let i = open; i < lib.length; i += 1) {
    if (lib[i] === '[') depth += 1;
    else if (lib[i] === ']') {
      depth -= 1;
      if (depth === 0) { end = i; break; }
    }
  }
  expect(at).toBeGreaterThan(-1);
  expect(end).toBeGreaterThan(open);
  const body = lib.slice(open + 1, end);
  return [...body.matchAll(/([a-z0-9_]+)\s*,/g)].map((m) => m[1]);
}

function apiFiles(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, src] of Object.entries(API_FILES)) {
    if (!key.endsWith('.test.ts')) out[key] = src;
  }
  return out;
}

/** 前端字面量 invoke 的命令名。泛型里可能有嵌套尖括号
 *  （invoke<Record<string, SpacePoint[]>>('get_space_history')），先用 [^()] 整体吃掉。 */
function invokedCommands(): string[] {
  const out: string[] = [];
  for (const src of Object.values(apiFiles())) {
    const re = /invoke(?:<[^()]{0,400}?>)?\s*\(\s*(['"])([a-z0-9_]+)\1/g;
    let m: RegExpExecArray | null = re.exec(src);
    while (m !== null) {
      out.push(m[2]);
      m = re.exec(src);
    }
  }
  return out;
}

const declared = declaredCommands();
const registered = registeredCommands();
const invoked = [...new Set(invokedCommands())].sort();

/** 有意不给前端用的命令（例如只服务于托盘菜单或后台线程）写在这里并注明原因。 */
const ALLOW_UNUSED: string[] = [];

describe('Rust 命令面与前端 invoke 的契约（R460）', () => {
  it('解析器本身没空转（数量下限 + 已知命令在册）', () => {
    expect(Object.keys(apiFiles()).length).toBeGreaterThanOrEqual(6);
    expect(declared.length).toBeGreaterThanOrEqual(100);
    expect(registered.length).toBe(declared.length);
    expect(declared).toContain('scan_path');
    expect(invoked.length).toBeGreaterThanOrEqual(100);
  });

  it('每条 #[tauri::command] 都挂进了 generate_handler!（漏挂 = 运行时 command not found）', () => {
    expect(declared.filter((n) => !registered.includes(n)).sort()).toEqual([]);
  });

  it('前端 invoke 的名字都在 Rust 命令表里（拼错 = 用户点下去才炸）', () => {
    expect(invoked.filter((n) => !declared.includes(n)).sort()).toEqual([]);
  });

  it('Rust 命令都被前端用上了（否则就是改名后留下的死命令）', () => {
    expect(declared.filter((n) => !invoked.includes(n) && !ALLOW_UNUSED.includes(n)).sort()).toEqual([]);
  });
});
