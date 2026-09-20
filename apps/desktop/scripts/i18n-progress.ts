// 一次性进度盘点脚本：列出每个源文件还剩多少行硬编码中文文案。
// 跑法：pnpm -C apps/desktop exec vite-node scripts/i18n-progress.ts
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { cjkLines } from '../src/i18n/scan.ts';

function walk(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (/\.(ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name) && !p.includes(`${join('src', 'i18n')}`)) out.push(p);
  }
  return out;
}

const rows = walk('src')
  .map((f) => [f.replace(/\\/g, '/'), cjkLines(readFileSync(f, 'utf8'))] as const)
  .filter(([, n]) => n > 0)
  .sort((a, b) => b[1] - a[1]);

for (const [f, n] of rows) console.log(String(n).padStart(4), f);
console.log('TOTAL', rows.reduce((s, [, n]) => s + n, 0), 'lines /', rows.length, 'files');
