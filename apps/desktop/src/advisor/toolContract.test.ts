// 跨语言契约守卫：agent-server 的 #[tool] 后端工具 ↔ 前端 AI 工具表（mcpToolRegistry）。
//
// 为什么要这条测试：后端每加一个 MCP 工具，前端 AI 工具表必须同步加一条 mcpTool(...)
// 镜像（带中文场景描述），否则那个工具只有工具墙能看见、AI 对话里永远调不到——
// 这种「功能存在但 AI 用不上」的缺口不会编译报错、不会 lint 报错，只能靠契约测试挡。
// 反向同样守：镜像里有、后端没有 = AI 会调用一个不存在的工具，运行时才炸。
//
// 本测试直接读 Rust 源码（不依赖 agent-server.exe 是否构建），纯静态解析。
// 路径层级：src/advisor → src → apps/desktop → apps → 仓库根。
import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { mcpToolRegistry } from './tools';

/** 递归收集 crates/agent-server/src 下所有 `#[tool(...)] fn 名字`（测试文件除外）。 */
function backendToolNames(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) {
      out.push(...backendToolNames(p));
      continue;
    }
    if (!entry.endsWith('.rs') || entry.includes('test')) continue;
    const src = readFileSync(p, 'utf8');
    for (const m of src.matchAll(/#\[tool\([\s\S]*?\)\]\s*(?:pub\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/g)) {
      out.push(m[1]);
    }
  }
  return [...new Set(out)];
}

const BACKEND = backendToolNames(
  join(fileURLToPath(new URL('../../../../crates/agent-server/src', import.meta.url))),
);

describe('agent-server 工具 ↔ AI 工具表契约', () => {
  it('后端工具清单可读且非空', () => {
    expect(BACKEND.length).toBeGreaterThan(50);
  });

  it('每个后端 MCP 工具都接进了 AI 工具表（AI 调得到）', () => {
    const mirrored = new Set(mcpToolRegistry.map((t) => t.name));
    const missing = BACKEND.filter((n) => !mirrored.has(n));
    expect(missing, `后端有但 AI 工具表没有：${missing.join(', ')}`).toEqual([]);
  });

  it('AI 工具表里没有后端不存在的工具（不会调用到空气）', () => {
    const backend = new Set(BACKEND);
    const orphan = mcpToolRegistry.map((t) => t.name).filter((n) => !backend.has(n));
    expect(orphan, `AI 工具表有但后端没有：${orphan.join(', ')}`).toEqual([]);
  });
});
