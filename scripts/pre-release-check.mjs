#!/usr/bin/env node
// R514 发布前完整验收清单：一键跑完本地全部质量门禁，任一失败 exit 1。
//
// 用法:
//   node scripts/pre-release-check.mjs            完整跑（含 audit/test/self-test，约数分钟）
//   node scripts/pre-release-check.mjs --quick    快速跑（跳过 audit/cargo test/self-test，约 30s）
//   node scripts/pre-release-check.mjs --skip-plugins   跳过 plugins/* 独立构建
//
// 门禁清单（与 CI .github/workflows/ci.yml 保持同口径）:
//   1. cargo fmt --all -- --check
//   2. cargo clippy --workspace --all-targets -- -D warnings
//   3. cargo audit（只允许 RUSTSEC-2024-0370 / RUSTSEC-2024-0429 两条已知告警）
//   4. cargo test --workspace
//   5. plugins/* 各自 cargo test --release + build --release（独立 workspace，主门禁不覆盖）
//   6. 前端 pnpm -C apps/desktop: tsc -b + eslint + vitest run
//   7. docs 守卫 check-changelog.mjs + check-docs-links.mjs
//   8. diskpilot-desktop --self-test（无头冒烟 5 项断言）

import { spawnSync } from 'node:child_process';
import { readdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';

const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const args = process.argv.slice(2);
const quick = args.includes('--quick');
const skipPlugins = args.includes('--skip-plugins');

const ALLOWED_AUDIT = ['RUSTSEC-2024-0370', 'RUSTSEC-2024-0429'];

function run(label, cmd, cmdArgs, opts = {}) {
  console.log(`\n=== ${label} ===`);
  console.log(`$ ${cmd} ${cmdArgs.join(' ')}`);
  const r = spawnSync(cmd, cmdArgs, { cwd: root, stdio: 'inherit', shell: false, ...opts });
  return r.status === 0;
}

function runSh(label, sh, opts = {}) {
  console.log(`\n=== ${label} ===`);
  console.log(`$ ${sh}`);
  const r = spawnSync('bash', ['-c', sh], { cwd: root, stdio: 'inherit', ...opts });
  return r.status === 0;
}

const results = [];
const record = (name, ok) => { results.push([name, ok]); console.log(`${ok ? '✅' : '❌'} ${name}`); };

// 环境探测：cargo 必须在 PATH（Windows 常见坑：新开 shell 没 source cargo env）。
const cargoProbe = spawnSync('cargo', ['--version'], { stdio: 'pipe' });
if (cargoProbe.status !== 0) {
  console.error('❌ 找不到 cargo——请先 source cargo 环境（如 ~/.cargo/env 或设置 PATH 含 ~/.cargo/bin）');
  process.exit(1);
}

// 1-2. fmt + clippy
record('cargo fmt --check', run('fmt', 'cargo', ['fmt', '--all', '--', '--check']));
record('cargo clippy -D warnings', run('clippy', 'cargo', ['clippy', '--workspace', '--all-targets', '--', '-D', 'warnings']));

if (!quick) {
  // 3. cargo audit：抓 RUSTSEC 行，过滤白名单
  console.log('\n=== cargo audit ===');
  const a = spawnSync('cargo', ['audit'], { cwd: root, encoding: 'utf8' });
  const out = (a.stdout || '') + (a.stderr || '');
  const advisories = [...new Set([...out.matchAll(/RUSTSEC-\d{4}-\d{4}/g)].map(m => m[0]))];
  const unexpected = advisories.filter(x => !ALLOWED_AUDIT.includes(x));
  console.log(`发现告警: ${advisories.join(', ') || '(无)'}`);
  record(`cargo audit（白名单 ${ALLOWED_AUDIT.join('/')}）`, a.status === 0 && unexpected.length === 0);
  if (unexpected.length) console.log(`  ❌ 白名单外告警: ${unexpected.join(', ')}`);

  // 4. workspace 测试
  record('cargo test --workspace', run('test', 'cargo', ['test', '--workspace']));
}

// 5. plugins 独立构建+测试
if (!skipPlugins && existsSync(join(root, 'plugins'))) {
  for (const id of readdirSync(join(root, 'plugins'))) {
    const dir = join(root, 'plugins', id);
    if (!existsSync(join(dir, 'Cargo.toml'))) continue;
    const env = { ...process.env, CARGO_TARGET_DIR: 'C:/t-plugins-check' };
    console.log(`\n=== plugin ${id} ===`);
    const t = spawnSync('cargo', ['test', '--release'], { cwd: dir, stdio: 'inherit', env });
    const b = spawnSync('cargo', ['build', '--release'], { cwd: dir, stdio: 'inherit', env });
    record(`plugin ${id} test+build`, t.status === 0 && b.status === 0);
  }
}

// 6. 前端门禁（Windows 下 pnpm 是 pnpm.cmd，spawnSync 无 shell 找不到，需 shell:true）
const isWin = process.platform === 'win32';
record('frontend tsc', run('tsc', 'pnpm', ['-C', 'apps/desktop', 'exec', 'tsc', '-b'], { shell: isWin }));
record('frontend eslint', run('eslint', 'pnpm', ['-C', 'apps/desktop', 'exec', 'eslint', 'src', '--ext', '.ts,.tsx'], { shell: isWin }));
record('frontend vitest', run('vitest', 'pnpm', ['-C', 'apps/desktop', 'exec', 'vitest', 'run'], { shell: isWin }));

// 7. docs 守卫
record('check-changelog', run('changelog', 'node', ['scripts/check-changelog.mjs']));
record('check-docs-links', run('docs-links', 'node', ['scripts/check-docs-links.mjs']));

// 8. 无头 self-test（完整模式才跑，编译 desktop 较慢）
if (!quick) {
  record('desktop --self-test', run('self-test', 'cargo', ['run', '-p', 'diskpilot-desktop', '--', '--self-test']));
}

// 汇总
const failed = results.filter(([, ok]) => !ok);
console.log('\n================ 验收汇总 ================');
for (const [name, ok] of results) console.log(`${ok ? '✅' : '❌'} ${name}`);
console.log(`共 ${results.length} 项，失败 ${failed.length} 项${quick ? '（--quick 模式）' : ''}`);
process.exit(failed.length ? 1 : 0);
