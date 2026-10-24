import { describe, it, expect } from 'vitest';

// R413：凭据与代码类输入框的表单卫生——
// ① 密码类输入框必须显式声明 autoComplete，且不许是 "on"：否则浏览器的密码管理器
//    会把 API key 当成站点登录口令跨站保存、并在别的表单里自动建议（设置页的 key 框
//    默认 type=password，正是重灾区）。
// ② 除聊天输入外，所有 textarea 必须 spellCheck={false}：JSON / args / env / headers /
//    TOML / winapp2.ini 里全是代码，浏览器的红波浪线只会添乱。
// ③ .tb-args 的 CLI 参数框同样关掉拼写检查（--depth 这类参数不该被画红线）。
// 聊天输入是给人写自然语言的，保留拼写检查——所以它是唯一的例外。
const RAW_TSX = import.meta.glob('./**/*.tsx', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

const TAG = /<(input|textarea)\b[\s\S]*?\/>/g;

function lineOf(src: string, idx: number): number {
  return src.slice(0, idx).split('\n').length;
}

type Src = Record<string, string>;

const isPassword = (tag: string) => /type="password"|type=\{[^}]*password/.test(tag);
const isChatInput = (tag: string) => /chat\.input\.aria|className="[^"]*chat-input/.test(tag);

function passwordOffenders(tsx: Src): string[] {
  const bad: string[] = [];
  for (const [path, raw] of Object.entries(tsx)) {
    const src = raw.replace(/\r\n/g, '\n');
    for (const m of src.matchAll(TAG)) {
      if (m[1] !== 'input' || !isPassword(m[0])) continue;
      const ac = m[0].match(/autoComplete="([^"]*)"/);
      if (!ac) {
        bad.push(path + ':' + lineOf(src, m.index ?? 0) + ' 密码类输入框缺 autoComplete（会被浏览器当成登录口令）');
      } else if (ac[1] === 'on') {
        bad.push(path + ':' + lineOf(src, m.index ?? 0) + ' 密码类输入框 autoComplete="on" 会让浏览器接管（应为 new-password 或 off）');
      }
    }
  }
  return bad;
}

function textareaOffenders(tsx: Src): string[] {
  const bad: string[] = [];
  for (const [path, raw] of Object.entries(tsx)) {
    const src = raw.replace(/\r\n/g, '\n');
    for (const m of src.matchAll(TAG)) {
      if (m[1] !== 'textarea' || isChatInput(m[0])) continue;
      if (!/spellCheck=\{false\}/.test(m[0])) {
        bad.push(path + ':' + lineOf(src, m.index ?? 0) + ' 代码/配置 textarea 缺 spellCheck={false}（会画红波浪线）');
      }
    }
  }
  return bad;
}

function tbArgsOffenders(tsx: Src): string[] {
  const bad: string[] = [];
  for (const [path, raw] of Object.entries(tsx)) {
    const src = raw.replace(/\r\n/g, '\n');
    for (const m of src.matchAll(TAG)) {
      if (m[1] !== 'input' || !/className="[^"]*tb-args/.test(m[0])) continue;
      if (!/spellCheck=\{false\}/.test(m[0])) {
        bad.push(path + ':' + lineOf(src, m.index ?? 0) + ' CLI 参数框缺 spellCheck={false}');
      }
    }
  }
  return bad;
}

describe('凭据与代码类输入框的表单卫生（R413）', () => {
  it('扫到了足够多源码（守卫不是空转）', () => {
    expect(Object.keys(RAW_TSX).length).toBeGreaterThan(20);
    const textareas = Object.values(RAW_TSX).flatMap((raw) =>
      [...raw.replace(/\r\n/g, '\n').matchAll(TAG)].filter((m) => m[1] === 'textarea'),
    );
    expect(textareas.length).toBeGreaterThanOrEqual(8);
  });

  it('密码类输入框显式声明 autoComplete 且不是 on', () => {
    expect(passwordOffenders(RAW_TSX)).toEqual([]);
  });

  it('代码/配置 textarea 全关拼写检查，只有聊天输入保留', () => {
    expect(textareaOffenders(RAW_TSX)).toEqual([]);
  });

  it('CLI 参数框关掉拼写检查', () => {
    expect(tbArgsOffenders(RAW_TSX)).toEqual([]);
  });

  it('设置页的 API key 走 new-password，不被浏览器密码管理器认领', () => {
    const src = (RAW_TSX['./components/Settings.tsx'] ?? '').replace(/\r\n/g, '\n');
    const at = src.indexOf('placeholder="sk-..."');
    expect(at).toBeGreaterThan(0);
    const block = src.slice(Math.max(0, at - 400), at + 400);
    expect(block).toContain('autoComplete="new-password"');
    expect(block).toContain('spellCheck={false}');
    expect(block).toContain('autoCapitalize="off"');
    expect(block).toContain('autoCorrect="off"');
  });

  it('能抓到违规写法（自检）', () => {
    const fake: Src = {
      './bad.tsx': [
        '<input type="password" value={k} />',
        '<input type={"show" ? "text" : "password"} autoComplete="on" />',
        '<textarea value={json} />',
        '<textarea aria-label={t("chat.input.aria")} />',
        '<input className="tb-args" value={a} />',
      ].join('\n'),
    };
    expect(passwordOffenders(fake).length).toBe(2);
    expect(textareaOffenders(fake).length).toBe(1);
    expect(tbArgsOffenders(fake).length).toBe(1);

    const ok: Src = {
      './ok.tsx': [
        '<input type="password" autoComplete="new-password" spellCheck={false} />',
        '<textarea spellCheck={false} />',
        '<textarea aria-label={t("chat.input.aria")} />',
        '<input className="tb-args" spellCheck={false} />',
      ].join('\n'),
    };
    expect(passwordOffenders(ok)).toEqual([]);
    expect(textareaOffenders(ok)).toEqual([]);
    expect(tbArgsOffenders(ok)).toEqual([]);
  });
});
