// R473：toast 的第一参数必须是 i18n 文案，错误提示要带「哪一步失败了」的上下文——
// 历史上有 6 处直接 toast(String(e), 'err')，用户只看到裸 os error（例如 os error 5），
// 完全不知道是读用量、跑工具、启动工具、读硬件还是网络检测失败。
import { describe, expect, it } from 'vitest';

const RAW_TS = import.meta.glob('./**/*.ts', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const RAW_TSX = import.meta.glob('./**/*.tsx', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

function sources(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [p, src] of Object.entries({ ...RAW_TS, ...RAW_TSX })) {
    if (p.endsWith('.test.ts') || p.endsWith('.test.tsx')) continue;
    out[p] = src;
  }
  return out;
}

// 判断空白用 trim()：源码字符串里的反斜杠正则在工具链里容易被吞，用字符判断最稳。
function skipSpace(src: string, from: number): number {
  let j = from;
  while (j < src.length && src[j].trim() === '') j += 1;
  return j;
}

/** toast( 之后第一个参数若以裸 String( 开头，说明错误原文直接进了提示条（没有动作上下文）。 */
function rawErrorToasts(src: string): number[] {
  const hits: number[] = [];
  let i = 0;
  while ((i = src.indexOf('toast(', i)) !== -1) {
    const j = skipSpace(src, i + 6);
    if (src.startsWith('String(', j)) hits.push(src.slice(0, i).split(String.fromCharCode(10)).length);
    i += 6;
  }
  return hits;
}

const SRC = sources();

describe('toast 文案的动作上下文（R473）', () => {
  it('解析器不空转：源码与非测试文件数量合理', () => {
    const files = Object.keys(SRC);
    expect(files.length).toBeGreaterThan(50);
    // 两个 glob 都真的取到东西（任一漏了会静默减少覆盖面）
    expect(files.some((p) => p === './store.ts')).toBe(true);
    expect(files.some((p) => p === './App.tsx')).toBe(true);
    const total = files.reduce((n, p) => n + SRC[p].split('toast(').length - 1, 0);
    expect(total).toBeGreaterThan(50);
  });

  it('没有一处把裸错误串直接扔给用户（必须走 t(...) 带动作名）', () => {
    const offenders: string[] = [];
    for (const [p, src] of Object.entries(SRC)) {
      for (const line of rawErrorToasts(src)) offenders.push(p + ':' + line);
    }
    expect(offenders).toEqual([]);
  });

  it('错误信息是表达式而不是被引号包住的字面量', () => {
    // 踩过的坑：写成 { msg: 'String(e instanceof Error ? e.message : e)' } 时，
    // 界面会原样显示 String(e instanceof Error ? e.message : e) 这一行代码。
    const bad = 'msg: ' + "'" + 'String(';
    const offenders: string[] = [];
    for (const [p, src] of Object.entries(SRC)) {
      if (src.includes(bad)) offenders.push(p);
    }
    expect(offenders).toEqual([]);
  });
  it('五个动作上下文文案中英成对', () => {
    const raw = (import.meta.glob('./i18n/namespaces/toolbelt.ts', { query: '?raw', import: 'default', eager: true }) as Record<string, string>)['./i18n/namespaces/toolbelt.ts'];
    for (const k of ['toolbelt.net.probeFailed', 'toolbelt.cli.runFailed', 'toolbelt.wall.launchFailed', 'toolbelt.wall.usageFailed', 'toolbelt.hw.loadFailed']) {
      expect(raw.split(k).length - 1, k).toBe(2);
    }
    expect(raw).toContain('网络连通性测试失败：{msg}');
    expect(raw).toContain('Network probe failed: {msg}');
  });
});
