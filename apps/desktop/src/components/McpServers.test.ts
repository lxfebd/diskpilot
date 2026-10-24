import { describe, expect, it } from 'vitest';
import { auditNames, mcpFieldError } from './McpServers';
import type { McpAuditEntry } from '../api';

const base = (op: string, extra: Partial<McpAuditEntry> = {}): McpAuditEntry => ({
  ts: 1_700_000_000,
  op,
  server_id: 'srv-1',
  ...extra,
});

describe('审计行归属（R339 auditNames）', () => {
  it('call_tool：工具名是主语，服务器名经 id 从列表翻出', () => {
    const n = auditNames(
      base('call_tool', { tool: 'fs_read', perm: 'L0', granted: true, ok: true }),
      [{ id: 'srv-1', name: 'git-mcp' }],
    );
    expect(n).toEqual({ subject: 'fs_read', server: 'git-mcp' });
  });

  it('call_tool：后端给 server_name 时优先用它（不依赖列表）', () => {
    const n = auditNames(
      base('call_tool', { tool: 'fs_read', server_name: 'git-mcp' }),
      null,
    );
    expect(n.server).toBe('git-mcp');
  });

  it('call_tool：服务器已删（列表里找不到 id）→ 如实留空，不猜', () => {
    const n = auditNames(base('call_tool', { tool: 'gone' }), [{ id: 'other', name: 'x' }]);
    expect(n).toEqual({ subject: 'gone', server: null });
    expect(auditNames(base('call_tool', { tool: 'gone' }), null).server).toBeNull();
  });

  it('add_server / update_server：主语是后端记下的 server_name', () => {
    expect(auditNames(base('add_server', { server_name: 'git-mcp' }), null)).toEqual(
      { subject: 'git-mcp', server: null },
    );
    expect(auditNames(base('update_server', { server_name: 'git-mcp' }), null).subject).toBe('git-mcp');
  });

  it('remove_server：后端不记名字（配置已删）→ 主语为空', () => {
    expect(auditNames(base('remove_server'), null)).toEqual({ subject: null, server: null });
  });
});
describe('字段级校验（R391 mcpFieldError）', () => {
  it('空名字（含纯空白）→ name', () => {
    expect(mcpFieldError('', 'stdio', 'npx', '')).toBe('name');
    expect(mcpFieldError('   ', 'http', '', 'https://x/mcp')).toBe('name');
  });

  it('stdio：名字有了、命令为空 → command', () => {
    expect(mcpFieldError('git', 'stdio', '', '')).toBe('command');
  });

  it('http：名字有了、url 为空 → url（命令空不算错）', () => {
    expect(mcpFieldError('git', 'http', 'npx', '')).toBe('url');
  });

  it('stdio：命令有了，url 空不算错', () => {
    expect(mcpFieldError('git', 'stdio', 'npx', '')).toBeNull();
  });

  it('http：url 有了，命令空不算错', () => {
    expect(mcpFieldError('git', 'http', '', 'https://x/mcp')).toBeNull();
  });

  it('全部填好 → null', () => {
    expect(mcpFieldError('git', 'http', 'npx', 'https://x/mcp')).toBeNull();
  });
});
