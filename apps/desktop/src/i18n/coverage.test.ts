// ── i18n 文案表守卫测试 ────────────────────────────────────────────
// 铺开过程中的回归闸：任何一处键漏翻、前缀写错、拼 key 打错字都在这儿拦下，
// 而不是等用户在英文界面看到一个「键名」或半中半英的文案。
// 源码用 ?raw 抓取（vite 原生能力，不引 node:fs / @types/node）。
import { describe, it, expect } from 'vitest';
import { NAMESPACES } from './namespaces';
import { zhKeys, enKeys } from './core';
import { cjkLines, codeOnly } from './scan';
import { MIGRATED_FILES } from './migrated-files';

const RAW_SOURCES = import.meta.glob('../**/*.{ts,tsx}', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

/** 待扫描的源码：排除 i18n 目录自身（文案表/运行时，glob 键以 `./` 开头）和测试文件。 */
function sourceEntries(): Array<[string, string]> {
  return Object.entries(RAW_SOURCES).filter(
    ([path]) => !path.startsWith('./') && !path.includes('/i18n/') && !/\.test\.tsx?$/.test(path),
  );
}

describe('i18n 文案表', () => {
  it('每个命名空间的 zh / en 键集一致（不许半翻）', () => {
    for (const ns of NAMESPACES) {
      const missingEn = Object.keys(ns.zh).filter((k) => !(k in ns.en));
      const orphanEn = Object.keys(ns.en).filter((k) => !(k in ns.zh));
      expect(missingEn, `${ns.ns} 缺英文译文`).toEqual([]);
      expect(orphanEn, `${ns.ns} 有英文无中文原文`).toEqual([]);
    }
  });

  it('键名必须带本命名空间前缀', () => {
    for (const ns of NAMESPACES) {
      const bad = Object.keys(ns.zh).filter((k) => !k.startsWith(`${ns.ns}.`));
      expect(bad, `${ns.ns} 有越界键名`).toEqual([]);
    }
  });

  it('跨命名空间不重复定义同一个键', () => {
    const seen = new Map<string, string>();
    const dup: string[] = [];
    for (const ns of NAMESPACES) {
      for (const k of Object.keys(ns.zh)) {
        if (seen.has(k)) dup.push(`${k}（${seen.get(k)} 与 ${ns.ns}）`);
        else seen.set(k, ns.ns);
      }
    }
    expect(dup).toEqual([]);
  });

  it('源码里所有静态 t(key) 都能在中文表命中（拼错 key 直接红）', () => {
    const dict = new Set(zhKeys());
    const misses: string[] = [];
    // 取文案函数惯例叫 `t`；与局部变量撞名时允许别名 `tr`（见 Toolbelt/HwPanels）。
    // 只看代码：注释里的示例 `t('...')` 不算调用。
    for (const [path, code] of sourceEntries()) {
      for (const m of codeOnly(code).matchAll(/\b(?:t|tr)\(\s*(['"])([^'"]+)\1/g)) {
        if (!dict.has(m[2])) misses.push(`${path} → ${m[2]}`);
      }
    }
    expect(misses).toEqual([]);
  });

  it('中文表与英文表规模一致', () => {
    expect(enKeys().sort()).toEqual(zhKeys().sort());
  });

  // 清单里的文件必须一句硬编码中文都不剩（注释除外）：之后有人再往里写中文文案会被拦。
  // 迁移完一个文件再往里加一行，别提前加——测试会告诉你还剩几行。
  it('已迁移文件不再残留硬编码中文文案', () => {
    const dirty: string[] = [];
    for (const rel of MIGRATED_FILES) {
      const code = RAW_SOURCES[`../${rel}`];
      if (code === undefined) {
        dirty.push(`${rel}：找不到源码（文件改名或删除了？请同步 MIGRATED_FILES）`);
        continue;
      }
      const left = cjkLines(code);
      if (left > 0) dirty.push(`${rel}：还剩 ${left} 行中文文案`);
    }
    expect(dirty).toEqual([]);
  });

  // R313：键集一致只保证「有这个键」，不保证英文句子还用得上参数。zh 写了 {n}、
  // en 忘了 {n} 时，英文界面会静默丢掉那个数字（反之会漏出裸 {n}）。占位符集合
  // 必须逐键一致——这是文案层最容易在翻译/改词时丢掉的一层。
  it('同一键的 zh / en 占位符集合一致', () => {
    // R350 起英文可写复数语法 `{n:day|days}`：占位符名字仍只有第一个捕获组，
    // 冒号后的单/复数文案不参与比对（中文表一律只写 {n}）。
    const placeholderOf = (s: string): string[] =>
      [...s.matchAll(/\{(\w+)(?::[^}|]*\|[^}|]*)?\}/g)].map((m) => m[1]).sort();
    const drift: string[] = [];
    for (const ns of NAMESPACES) {
      for (const [k, zhText] of Object.entries(ns.zh) as [string, string][]) {
        const enText = (ns.en as Record<string, string>)[k];
        if (typeof enText !== 'string') continue; // 键缺失由上一条用例负责
        const zh = placeholderOf(zhText);
        const en = placeholderOf(enText);
        if (zh.join(',') !== en.join(',')) {
          drift.push(`${k}：zh [${zh.join(',')}] vs en [${en.join(',')}]`);
        }
      }
    }
    expect(drift).toEqual([]);
  });

  // R350：英文的单复数不许再手写 "(s)"——"1 day(s)" 是语法错误，读着也别扭。
  // 复数交给 {n:day|days} 语法（core.ts 的 formatPluralUnit），单数/复数各写一遍。
  // R471：数字后面直接跟名词时，英文必须写复数语法 `{n:one|other}`——否则界面会出现
  // 「1 files」「3 day ago」这种每个人都会截图发来的错。守卫连「名词短语」一起管：
  // `{n} hardware snapshots` / `{n} cleanup scopes` 这类中间夹修饰语的也要写成复数语法。
  it('英文表里数字后面不许再裸接数量名词', () => {
    const NOUNS = [
      'days', 'months', 'years', 'weeks', 'hours', 'items', 'files', 'entries', 'scripts',
      'steps', 'points', 'samples', 'snapshots', 'tools', 'plugins', 'games', 'drives', 'drivers',
      'servers', 'runs', 'locations', 'copies', 'versions', 'downloads', 'sensors', 'sticks',
      'scopes', 'matches', 'results', 'scans', 'tasks', 'images', 'messages', 'processes',
      'duplicates', 'rules', 'categories', 'folders', 'updates', 'cores', 'threads',
      'image', 'match', 'sample',
    ];
    const offenders: string[] = [];
    for (const ns of NAMESPACES) {
      for (const [k, v] of Object.entries(ns.en) as [string, string][]) {
        if (typeof v !== 'string') continue;
        // 一个占位符后面可能跟一小段修饰语（hardware / built-in cleanup），取最后那个词判名词。
        // 短语最多 3 个词：够覆盖「hardware snapshots」「built-in cleanup rules」，又不会把整句尾巴当成名词短语。
        for (const m of v.matchAll(/\{(\w+)\} ((?:[a-z][a-z-]* ){0,2}[a-z]+)\b/g)) {
          const phrase = m[2];
          const noun = phrase.split(' ')[phrase.split(' ').length - 1];
          if (!NOUNS.includes(noun)) continue;
          offenders.push(`${k}：“${m[0]}” 改成 {${m[1]}:<单数>|${phrase}}`);
        }
      }
    }
    expect(offenders).toEqual([]);
  });
  it('英文表不再手写 (s) 单复数', () => {
    const bad: string[] = [];
    for (const ns of NAMESPACES) {
      for (const [k, v] of Object.entries(ns.en) as [string, string][]) {
        if (typeof v === 'string' && /\(s\)/.test(v)) bad.push(`${k}：${v}`);
      }
    }
    expect(bad).toEqual([]);
  });
  // R449：键集一致只保证「有这条键」，不代表英文真的翻译了——en 值仍带汉字的
  // 键必须逐项落在允许清单里（两类：给机器/数据用的常值——多盘合并的虚拟根
  // path、mock 冒充后端的说明、喂给 AI 模型的会话上下文模板、提示用户照打的
  // 确认词、中文产品名「图吧工具箱」的目录与提示——以及作者名等专名）。
  // 哪条不再需要就删掉对应允许项，别让清单纯当免罪金牌。
  // 覆盖范围：CJK 表意区 + CJK 标点（U+3000–303F）+ 全角形式（U+FF01–FF60）。
  const ch = (n: number) => String.fromCharCode(n);
  const EN_CJK_ALLOW: ReadonlySet<string> = new Set([
    'common.data.allDrivesRoot',
    'common.mock.fanReadonlyChannel',
    'common.chatModel.procCalls',
    'common.chatModel.procResults',
    'common.chatModel.procWrap',
    'common.chatModel.procJoin',
    'common.chatModel.noteJoin',
    'common.chatModel.earlierUser',
    'common.chatModel.earlierAssistant',
    'hw.stressInputHint',
    'shell.disclaimer.authorName',
    'shell.disclaimer.p5',
    'toolbelt.cli.noRootA',
    'toolbelt.cli.rootPlaceholder',
  ]);
  const EN_CJK = new RegExp(
    '[' + ch(0x3000) + '-' + ch(0x303f) + ch(0x3400) + '-' + ch(0x4dbf) +
    ch(0x4e00) + '-' + ch(0x9fff) + ch(0xff01) + '-' + ch(0xff60) + ']',
  );
  it('英文表不再留下未翻译的中文（允许清单逐项核验）', () => {
    const bad: string[] = [];
    for (const ns of NAMESPACES) {
      for (const [k, v] of Object.entries(ns.en) as [string, string][]) {
        if (typeof v !== 'string' || !EN_CJK.test(v)) continue;
        if (!EN_CJK_ALLOW.has(k)) bad.push(k + ': ' + v);
      }
    }
    expect(bad).toEqual([]);
    // 允许清单本身不许腐化：条目必须恰好存在于某个命名空间，且确实还含汉字。
    for (const k of EN_CJK_ALLOW) {
      const owners = NAMESPACES.filter((ns) => k in (ns.en as Record<string, string>));
      expect(owners.length, k + ' 允许项必须在且仅在某个命名空间里').toBe(1);
      if (owners.length === 1) {
        const v = (owners[0].en as Record<string, string>)[k];
        expect(v, k + ' 允许项失踪').toBeDefined();
        expect(EN_CJK.test(v), k + ' 已不再含汉字——请从允许清单移除').toBe(true);
      }
    }
  });
});
