#!/usr/bin/env node
// 文档链接检查（R330）：README / docs / crates 与 .github 下的 markdown 里，
// 所有相对链接都必须指向仓库里真实存在的文件或目录。
//
// 为什么单独一个脚本而不是塞进 vitest：前端测试跑在 apps/desktop 的 Vite 沙箱里，
// 没有 node 类型也没有文件系统 API；文档是全仓资产，用 node 脚本最直接，
// CI 的 frontend job 会跑它（node scripts/check-docs-links.mjs）。
//
// 只查相对链接；http(s)/(mailto)/页内 #锚点 跳过；`file.md#section` 只校验文件，
// 不校验锚点（GitHub 的标题 slug 规则随文案变，校验它得复制一份 slug 算法）。
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const SKIP_DIRS = new Set(['node_modules', 'target', 'dist', '.git', '.pnpm-store', 'gen']);

function walkMarkdown(dir, out = []) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    const abs = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (!SKIP_DIRS.has(e.name)) walkMarkdown(abs, out);
    } else if (e.isFile() && e.name.toLowerCase().endsWith('.md')) {
      out.push(abs);
    }
  }
  return out;
}

// 扫描范围：仓库根的 *.md、docs/、crates/*/README.md、.github/ 下的 md。
function targets() {
  const list = [];
  for (const e of fs.readdirSync(ROOT, { withFileTypes: true })) {
    if (e.isFile() && e.name.toLowerCase().endsWith('.md')) list.push(path.join(ROOT, e.name));
  }
  walkMarkdown(path.join(ROOT, 'docs'), list);
  walkMarkdown(path.join(ROOT, '.github'), list);
  const crates = path.join(ROOT, 'crates');
  if (fs.existsSync(crates)) {
    for (const e of fs.readdirSync(crates, { withFileTypes: true })) {
      if (!e.isDirectory()) continue;
      const readme = path.join(crates, e.name, 'README.md');
      if (fs.existsSync(readme)) list.push(readme);
    }
  }
  return list;
}

const LINK_RE = /!?\[[^\]\n]*\]\(([^)\n]+)\)/g;

function normalizeTarget(raw) {
  let t = raw.trim();
  if (t.startsWith('<') && t.endsWith('>')) t = t.slice(1, -1).trim();
  t = t.split(/\s+/)[0]; // 去掉 `path "title"` 的标题
  return t;
}

function skipTarget(t) {
  if (!t) return true;
  if (/^(https?:|mailto:|ftp:|data:|#)/i.test(t)) return true;
  if (t.startsWith('//')) return true;
  if (t.includes('{') || t.includes('%')) return true; // 占位符 / URL 编码，无法也不该本地校验
  return false;
}

const broken = [];
let checked = 0;

for (const file of targets()) {
  const text = fs.readFileSync(file, 'utf8');
  const dir = path.dirname(file);
  LINK_RE.lastIndex = 0;
  let m;
  while ((m = LINK_RE.exec(text))) {
    const t = normalizeTarget(m[1]);
    if (skipTarget(t)) continue;
    const target = t.split('#')[0];
    if (!target) continue; // 纯锚点
    checked += 1;
    const abs = path.resolve(dir, target);
    if (!fs.existsSync(abs)) {
      const line = text.slice(0, m.index).split('\n').length;
      broken.push({
        file: path.relative(ROOT, file).replace(/\\/g, '/'),
        line,
        target: m[1].trim(),
      });
    }
  }
}

if (broken.length > 0) {
  console.error(`docs 链接检查失败：${broken.length} 条指向不存在的文件（共检查 ${checked} 条）`);
  for (const b of broken) {
    console.error(`  ${b.file}:${b.line}  →  ${b.target}`);
  }
  console.error('修法：改链接指向真实文件，或删掉已不存在的条目。');
  process.exit(1);
}

console.log(`docs 链接检查通过：${checked} 条相对链接全部命中真实文件。`);
