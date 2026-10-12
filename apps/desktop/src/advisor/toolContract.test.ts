// 跨语言契约守卫：agent-server 的 #[tool] 后端工具 ↔ 前端 AI 工具表（mcpToolRegistry）。
//
// 为什么要这条测试：后端每加一个 MCP 工具，前端 AI 工具表必须同步加一条 mcpTool(...)
// 镜像（带中文场景描述），否则那个工具只有工具墙能看见、AI 对话里永远调不到——
// 这种「功能存在但 AI 用不上」的缺口不会编译报错、不会 lint 报错、不会测试失败，
// 只能靠契约测试挡。反向同样守：镜像里有、后端没有 = AI 会调用一个不存在的工具，
// 运行时才炸。
//
// 实现口径：用 Vite 的 ?raw glob 直接读 Rust 源码（纯静态解析，不依赖
// agent-server.exe 是否构建，也不起进程）。本包没有 @types/node，测试文件又被
// `tsc -b` 纳入类型检查，所以不能用 node:fs / node:path / node:url。
import { describe, it, expect } from 'vitest';
import { mcpToolRegistry } from './tools';

const RUST_SOURCES = import.meta.glob(
  '../../../../crates/agent-server/src/**/*.rs',
  { query: '?raw', import: 'default', eager: true },
) as Record<string, string>;

/** 从 Rust 源码里收集 `#[tool(...)] fn 名字`（文件名含 test 的跳过）。 */
function backendToolNames(sources: Record<string, string>): string[] {
  const out = new Set<string>();
  for (const [file, src] of Object.entries(sources)) {
    if (file.includes('test')) continue;
    for (const m of src.matchAll(/#\[tool\([\s\S]*?\)\]\s*(?:pub\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/g)) {
      out.add(m[1]);
    }
  }
  return [...out];
}

const BACKEND = backendToolNames(RUST_SOURCES);

describe('agent-server 工具 ↔ AI 工具表契约', () => {
  it('后端工具源码可读且清单非空', () => {
    expect(Object.keys(RUST_SOURCES).length).toBeGreaterThan(10);
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
