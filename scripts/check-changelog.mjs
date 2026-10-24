// CHANGELOG 结构守卫：简介在首行且只出现一次、只有一个 [未发布]、
// 所有 ### 标题前有空行、标题不重复、结尾无多余空行。
// 与 check-docs-links.mjs 同风格：node ESM、零依赖、违规即 exit 1。
import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const file = path.join(root, 'CHANGELOG.md');
const raw = fs.readFileSync(file, 'utf8');
const text = raw.replace(/\r\n/g, '\n');
const lines = text.split('\n');
const violations = [];

// 1) 简介只在首行出现一次
const introHits = lines
  .map((l, i) => (l.startsWith('DiskPilot 变更日志') ? i : -1))
  .filter((i) => i >= 0);
if (introHits.length !== 1 || introHits[0] !== 0) {
  violations.push('文件头简介必须且只能在第 1 行出现一次（当前行: ' + (introHits.length ? introHits.map((i) => i + 1).join(',') : '无') + '）');
}

// 2) 恰好一个 [未发布] 区
const unreleased = lines
  .map((l, i) => (/^## \[未发布\]$/.test(l) ? i : -1))
  .filter((i) => i >= 0);
if (unreleased.length !== 1) {
  violations.push('必须有且只有一个 ## [未发布] 区（当前: ' + unreleased.length + ' 个，行 ' + unreleased.map((i) => i + 1).join(',') + '）');
}

// 2.5) [未发布] 必须是第一个条目之前的位置（R406：曾经把新条目写在标题上面，
//      结构照样「一个 [未发布]、标题前有空行」，只能靠这条抓出来）。
const firstH3 = lines.findIndex((l) => /^### /.test(l));
if (unreleased.length === 1 && firstH3 >= 0 && unreleased[0] > firstH3) {
  violations.push('## [未发布] 必须在所有 ### 条目之前（当前在第 ' + (unreleased[0] + 1) + ' 行，第一条目在第 ' + (firstH3 + 1) + ' 行）');
}

// 3) ### 标题前必须是空行（或文件首行）
lines.forEach((l, i) => {
  if (/^### /.test(l) && i > 0 && lines[i - 1].trim() !== '') {
    violations.push('### 标题前缺空行: 第 ' + (i + 1) + ' 行 ' + l.slice(0, 40));
  }
});

// 4) ### 标题文本不得重复
const seen = new Set();
lines.forEach((l, i) => {
  if (/^### /.test(l)) {
    if (seen.has(l.trim())) violations.push('重复的 ### 标题: 第 ' + (i + 1) + ' 行 ' + l.slice(0, 40));
    seen.add(l.trim());
  }
});

// 5) 结尾只能有一个换行（没有多余空行）
if (!/\n$/.test(text)) violations.push('文件必须以单个换行结尾');
if (/\n\n+$/.test(text)) violations.push('文件结尾有多余空行');

if (violations.length) {
  console.error('CHANGELOG 结构守卫失败：');
  for (const v of violations) console.error('  - ' + v);
  process.exit(1);
}
console.log('CHANGELOG 结构守卫通过（' + lines.length + ' 行）。');