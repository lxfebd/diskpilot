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
});
