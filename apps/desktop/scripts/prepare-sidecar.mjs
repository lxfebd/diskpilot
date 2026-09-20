// 打包前准备 tauri sidecar：把 agent-server 编到「与应用同一个 target triple」，
// 并按 tauri 的 `<name>-<triple>[.exe]` 命名约定放进 src-tauri/binaries/。
//
// 为什么不沿用「cargo build 落在 target/release，再由 build.rs 复制成 triple 名」：
// - `tauri build --target <triple>` 时主程序在 target/<triple>/release，而
//   `cargo build -p agent-server --release` 用的是 host 默认 target ——
//   在 macOS arm64 runner 上打 x86_64 包会塞进错架构的 sidecar；
// - 旧 build.rs 那段复制只认 `.exe`，macOS / Linux 上源文件名不带扩展名，
//   复制永远不触发，打包直接「externalBin not found」。
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const appDir = resolve(here, '..');
const repoRoot = resolve(appDir, '..', '..');
const exe = process.platform === 'win32' ? '.exe' : '';

function targetTriple() {
  const i = process.argv.indexOf('--target');
  if (i >= 0 && process.argv[i + 1]) return process.argv[i + 1];
  // release.yml 里显式给出，保证 sidecar 与应用同架构
  if (process.env.TAURI_TARGET_TRIPLE) return process.env.TAURI_TARGET_TRIPLE;
  const out = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  const m = /^host:\s*(\S+)/m.exec(out);
  if (!m) throw new Error('rustc -vV 未输出 host triple');
  return m[1];
}

const triple = targetTriple();
const args = ['build', '-p', 'agent-server', '--release', '--target', triple];
console.log(`[prepare-sidecar] cargo ${args.join(' ')}`);
execFileSync('cargo', args, { cwd: repoRoot, stdio: 'inherit' });

const built = join(repoRoot, 'target', triple, 'release', `agent-server${exe}`);
if (!existsSync(built)) throw new Error(`找不到构建产物：${built}`);

const binDir = join(appDir, 'src-tauri', 'binaries');
mkdirSync(binDir, { recursive: true });
const dst = join(binDir, `agent-server-${triple}${exe}`);
// 先删后拷：Windows 上覆盖仍在运行的 sidecar 会 EBUSY，报错也比静默留旧包清楚。
rmSync(dst, { force: true });
copyFileSync(built, dst);
console.log(`[prepare-sidecar] ${built} → ${dst}`);
