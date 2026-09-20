import { useCallback, useEffect, useState } from 'react';
import { Plus, Trash2, RefreshCw, CheckCircle2, XCircle, AlertTriangle, Info, Code2, ShieldCheck, FileClock } from 'lucide-react';
import {
  api,
  type McpAuditEntry,
  type McpServerPayload,
  type McpServerStatus,
  type McpToolMeta,
} from '../api';
import { isPermEnabled } from '../permissions';
import { isTauri } from '../env';
import { ErrorBoundary } from './ErrorBoundary';
import { refreshMcpTools } from '../advisor/tools';
import { useT } from '../i18n';

/**
 * 设置页「MCP」标签：管理用户添加的通用 MCP 服务器（stdio 进程 / 远程
 * streamable HTTP）。服务器工具会自动注册进 AI 工具表（dynamicMcpTools）。
 * 写操作（添加/编辑/删除/写工具调用）均需 L2 `mcp.manage` 权限 + 确认。
 */
export function McpServers() {
  return (
    <ErrorBoundary>
      <McpServersInner />
    </ErrorBoundary>
  );
}

function McpServersInner() {
  const t = useT();
  // 服务器列表三态：null = 加载中；[] = 已加载但为空；数组 = 正常。
  const [servers, setServers] = useState<McpServerStatus[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  // 测试结果弹层。
  const [testingId, setTestingId] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<{ id: string; tools: McpToolMeta[] } | null>(null);
  // 删除两步确认：pendingDelId 点了删除先变「确认？」。
  const [pendingDelId, setPendingDelId] = useState<string | null>(null);
  // 添加/编辑表单弹层：null = 关闭；'new' = 新增；id = 编辑该服务器。
  const [editing, setEditing] = useState<string | 'new' | null>(null);
  // JSON 导入：粘贴 Claude Desktop 风格配置一键加服务器。
  const [showJson, setShowJson] = useState(false);
  const [jsonText, setJsonText] = useState('');
  const [jsonErr, setJsonErr] = useState<string | null>(null);
  const [parsedJson, setParsedJson] = useState<Array<{ name: string; payload: McpServerPayload; error?: string }> | null>(null);
  const [jsonBusy, setJsonBusy] = useState(false);
  // 审计日志面板。
  const [showAudit, setShowAudit] = useState(false);
  const [audit, setAudit] = useState<McpAuditEntry[] | null>(null);
  const [auditErr, setAuditErr] = useState<string | null>(null);

  const manageOk = isPermEnabled('mcp.manage');

  const flash = (s: string) => {
    setMsg(s);
    setTimeout(() => setMsg(null), 3500);
  };

  const loadAudit = useCallback(async () => {
    setAuditErr(null);
    try {
      setAudit(await api.mcpAuditTail(80));
    } catch (e) {
      setAuditErr(String(e instanceof Error ? e.message : e));
      setAudit([]);
    }
  }, []);

  const load = useCallback(async () => {
    setBusy(true);
    setErr(null);
    try {
      setServers(await api.mcpListServers());
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
      setServers([]);
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const toggleEnabled = async (id: string, on: boolean) => {
    const s = servers?.find((x) => x.id === id);
    if (!s || !manageOk) return;
    setErr(null);
    try {
      const updated = await api.mcpUpdateServer(id, {
        name: s.name,
        enabled: on,
        writable: s.writable,
        tool_prefix: s.tool_prefix ?? null,
        transport: s.transport,
      }, true);
      // 后端返回的是完整 config（transport 字段），前端 status 列表只有摘要——
      // 用返回的 enabled 回填这一行。
      setServers((prev) => prev?.map((x) => (x.id === id ? { ...x, enabled: updated.enabled } : x)) ?? null);
      flash(on ? t('mcp.enabled') : t('mcp.disabled'));
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    }
  };

  const toggleWritable = async (id: string, on: boolean) => {
    const s = servers?.find((x) => x.id === id);
    if (!s || !manageOk) return;
    setErr(null);
    try {
      const updated = await api.mcpUpdateServer(id, {
        name: s.name,
        enabled: s.enabled,
        writable: on,
        tool_prefix: s.tool_prefix ?? null,
        transport: s.transport,
      }, true);
      setServers((prev) => prev?.map((x) => (x.id === id ? { ...x, writable: updated.writable } : x)) ?? null);
      flash(on ? t('mcp.markedWritable') : t('mcp.markedReadonly'));
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    }
  };

  const doRemove = async (id: string) => {
    if (!manageOk) return;
    setErr(null);
    try {
      await api.mcpRemoveServer(id, true);
      setServers((prev) => prev?.filter((x) => x.id !== id) ?? null);
      setPendingDelId(null);
      flash(t('mcp.removed'));
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    }
  };

  const doTest = async (id: string) => {
    setTestingId(id);
    setTestResult(null);
    try {
      const tools = await api.mcpTestServer(id);
      setTestResult({ id, tools });
    } catch (e) {
      setTestResult({ id, tools: [] });
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setTestingId(null);
    }
  };

  const onSaved = async (_savedServer: McpServerPayload) => {
    // 保存后重拉列表（拿到新 id + 健康探测），并刷新 AI 工具表。
    await load();
    void refreshMcpTools().catch(() => {});
    setEditing(null);
  };

  // ── JSON 导入 ─────────────────────────────────────────────
  // 支持两种格式：
  // 1) Claude Desktop 风格：{ "mcpServers": { "name": { "command": "...", "args": [...], "env": {...} } } }
  // 2) 直接数组：[{ "name": "...", "transport": { "type": "stdio"|"http", ... } }]
  const parseJsonImport = () => {
    setJsonErr(null);
    setParsedJson(null);
    let raw: unknown;
    try {
      raw = JSON.parse(jsonText);
    } catch (e) {
      setJsonErr(t('mcp.jsonParseFailed', { msg: String(e instanceof Error ? e.message : e) }));
      return;
    }
    const entries: Array<{ name: string; src: Record<string, unknown> }> = [];
    const obj = raw as Record<string, unknown>;
    if (raw && typeof raw === 'object' && !Array.isArray(raw) && obj.mcpServers && typeof obj.mcpServers === 'object') {
      // Claude Desktop 风格：mcpServers 对象，key = 服务器名。
      for (const [name, v] of Object.entries(obj.mcpServers as Record<string, unknown>)) {
        if (v && typeof v === 'object') entries.push({ name, src: v as Record<string, unknown> });
      }
    } else if (Array.isArray(raw)) {
      // 数组风格：[{ name?, command?, args?, env?, url? } | { name, transport: {...} }]
      for (const v of raw) {
        if (v && typeof v === 'object') {
          const src = v as Record<string, unknown>;
          const name = String(src.name ?? src.command ?? '');
          if (name) entries.push({ name, src });
        }
      }
    } else if (raw && typeof raw === 'object' && !obj.mcpServers) {
      // 单服务器对象：{ name, transport } 或 { command, ... }。
      const name = String(obj.name ?? obj.command ?? '');
      if (name) entries.push({ name, src: obj });
    }
    if (entries.length === 0) {
      setJsonErr(t('mcp.jsonNoServers'));
      return;
    }
    const out = entries.map(({ name, src }) => {
      // 显式 transport 优先；否则从 command/url 推断。
      const rawTransport = src.transport;
      if (rawTransport && typeof rawTransport === 'object') {
        const tt = rawTransport as Record<string, unknown>;
        const type = String(tt.type ?? '');
        if (type === 'stdio' || type === 'http') {
          return {
            name,
            payload: {
              name,
              enabled: src.enabled === false ? false : true,
              writable: src.writable === true,
              tool_prefix: src.tool_prefix ? String(src.tool_prefix) : null,
              transport: tt as unknown as McpServerPayload['transport'],
            },
          };
        }
      }
      const hasUrl = typeof src.url === 'string' && src.url.trim().length > 0;
      if (hasUrl) {
        return {
          name,
          payload: {
            name,
            enabled: src.enabled === false ? false : true,
            writable: src.writable === true,
            tool_prefix: src.tool_prefix ? String(src.tool_prefix) : null,
            transport: {
              type: 'http' as const,
              url: String(src.url),
              headers: (src.headers as Record<string, string> | undefined) ?? {},
              timeout_secs: typeof src.timeout_secs === 'number' ? src.timeout_secs : null,
            },
          },
        };
      }
      const hasCommand = typeof src.command === 'string' && src.command.trim().length > 0;
      if (hasCommand) {
        return {
          name,
          payload: {
            name,
            enabled: src.enabled === false ? false : true,
            writable: src.writable === true,
            tool_prefix: src.tool_prefix ? String(src.tool_prefix) : null,
            transport: {
              type: 'stdio' as const,
              command: String(src.command),
              args: Array.isArray(src.args) ? src.args.map(String) : [],
              env: (src.env as Record<string, string> | undefined) ?? {},
              cwd: typeof src.cwd === 'string' ? src.cwd : null,
            },
          },
        };
      }
      return { name, payload: null as unknown as McpServerPayload, error: t('mcp.errNoTransport') };
    });
    const mapped = out.map((x) => ({ name: x.name, payload: x.payload, error: x.error }));
    setParsedJson(mapped);
  };

  const importJson = async () => {
    if (!parsedJson || parsedJson.length === 0) return;
    setJsonBusy(true);
    setJsonErr(null);
    try {
      for (const item of parsedJson) {
        if (!item.payload) continue;
        await api.mcpAddServer(item.payload, true);
      }
      await load();
      void refreshMcpTools().catch(() => {});
      setShowJson(false);
      setJsonText('');
      setParsedJson(null);
      flash(t('mcp.imported', { n: parsedJson.filter((x) => x.payload).length }));
    } catch (e) {
      setJsonErr(t('mcp.importFailed', { msg: String(e instanceof Error ? e.message : e) }));
    } finally {
      setJsonBusy(false);
    }
  };

  return (
    <div className="settings-pane">
      <p className="hint">
        <Info size={12} />
        <span>{t('mcp.hint')}</span>
      </p>

      <div className="settings-group-title" style={{ marginTop: 12 }}>
        {t('mcp.serverCount', { n: servers?.length ?? 0 })}
        <button
          className="ghost small"
          style={{ float: 'right' }}
          onClick={() => setEditing('new')}
          disabled={!manageOk}
          title={manageOk ? t('mcp.addTitle') : t('mcp.needPermTitle')}
        >
          <Plus size={12} style={{ verticalAlign: '-2px', marginRight: 3 }} />
          {t('mcp.add')}
        </button>
        <button
          className="ghost small"
          style={{ float: 'right', marginRight: 8 }}
          onClick={() => setShowJson((v) => !v)}
          disabled={!manageOk}
          title={t('mcp.jsonImportTitle')}
        >
          <Code2 size={12} style={{ verticalAlign: '-2px', marginRight: 3 }} />
          {t('mcp.jsonImport')}
        </button>
        <button
          className="ghost small"
          style={{ float: 'right', marginRight: 8 }}
          onClick={() => void load()}
          disabled={busy}
          title={t('mcp.reloadTitle')}
        >
          <RefreshCw size={12} className={busy ? 'spin' : undefined} />
        </button>
        <button
          className="ghost small"
          style={{ float: 'right', marginRight: 8 }}
          onClick={() => {
            setShowAudit((v) => !v);
            if (!showAudit) void loadAudit();
          }}
          title={t('mcp.auditTitle')}
        >
          <FileClock size={12} style={{ verticalAlign: '-2px', marginRight: 3 }} />
          {t('mcp.audit')}
        </button>
      </div>

      {showAudit && (
        <div className="mcp-test-panel" style={{ marginBottom: 4 }}>
          <div className="mcp-name-line" style={{ justifyContent: 'space-between' }}>
            <span className="switch-label">{t('mcp.auditPanelTitle')}</span>
            <button className="ghost icon" onClick={() => setShowAudit(false)} title={t('mcp.close')}>
              <XCircle size={14} />
            </button>
          </div>
          {auditErr && <div className="error" style={{ marginTop: 4 }}>{auditErr}</div>}
          {audit === null && <p className="muted small">{t('mcp.loading')}</p>}
          {audit !== null && audit.length === 0 && (
            <p className="muted small">{t('mcp.auditEmpty')}</p>
          )}
          {audit !== null && audit.length > 0 && (
            <ul className="mcp-tools-list" style={{ maxHeight: 220, overflowY: 'auto' }}>
              {audit.map((e, i) => {
                const time = new Date(e.ts * 1000).toLocaleTimeString();
                const tag = e.op === 'call_tool'
                  ? t('mcp.auditCall', { tool: e.tool ?? '' })
                  : e.op === 'add_server' ? t('mcp.auditAddServer')
                    : e.op === 'update_server' ? t('mcp.auditUpdateServer')
                      : e.op === 'remove_server' ? t('mcp.auditRemoveServer') : e.op;
                return (
                  <li key={i} style={{ display: 'flex', gap: 8, alignItems: 'baseline', flexWrap: 'wrap' }}>
                    <code className="muted small">{time}</code>
                    <span className="muted small">{tag ?? ''}</span>
                    {e.tool && <code>{e.tool}</code>}
                    {e.perm && <span className={`badge ${e.perm === 'L0' ? 'ok' : e.perm === 'L3' ? '' : 'warn'}`} style={{ marginLeft: 2 }}>{e.perm}</span>}
                    {e.granted === false && <span className="badge" style={{ borderColor: '#e05c51' }}>{t('mcp.auditDenied')}</span>}
                    {e.args != null && typeof e.args === 'object' && 'keys' in (e.args as object) && (
                      <span className="muted small">{t('mcp.auditArgs', { keys: (e.args as { keys?: string[] }).keys?.join(', ') ?? '' })}</span>
                    )}
                    {e.ok === false && e.error && <span className="error-inline small">{e.error}</span>}
                    {e.ok === true && <CheckCircle2 size={12} style={{ color: 'var(--accent-strong)' }} />}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}

      {showJson && (
        <div className="mcp-json-import">
          <div className="switch-label">{t('mcp.jsonPanelTitle')}</div>
          <p className="muted small" style={{ marginTop: 2 }}>
            {t('mcp.jsonHelpIntro')}
            <code>{t('mcp.jsonShapeExample')}</code>
            {t('mcp.jsonHelpOrArray')}<code>[{'{'} name, command, url … {'}'}]</code>{t('mcp.jsonHelpWritableLead')}<code>writable: true</code>{t('mcp.jsonHelpWritableTail')}
          </p>
          <textarea
            className="mcp-json-input"
            value={jsonText}
            onChange={(e) => { setJsonText(e.target.value); setParsedJson(null); }}
            placeholder={'{\n  "mcpServers": {\n    "filesystem": {\n      "command": "npx",\n      "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:/Users/me/Documents"]\n    }\n  }\n}'}
            rows={7}
            spellCheck={false}
          />
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginTop: 8 }}>
            <button type="button" className="ghost small" onClick={parseJsonImport} disabled={!jsonText.trim()}>
              {t('mcp.parsePreview')}
            </button>
            {parsedJson && parsedJson.length > 0 && (
              <button type="button" className="primary small" onClick={() => void importJson()} disabled={jsonBusy}>
                {jsonBusy ? t('mcp.importing') : t('mcp.importN', { n: parsedJson.filter((x) => x.payload).length })}
              </button>
            )}
            <button type="button" className="ghost small" onClick={() => { setShowJson(false); setJsonText(''); setParsedJson(null); setJsonErr(null); }}>
              {t('mcp.collapse')}
            </button>
          </div>
          {jsonErr && <div className="error" style={{ marginTop: 6 }}>{jsonErr}</div>}
          {parsedJson && parsedJson.length > 0 && (
            <ul className="mcp-json-preview" style={{ marginTop: 6 }}>
              {parsedJson.map((item, i) => (
                <li key={i}>
                  {item.error ? (
                    <>
                      <code>{item.name}</code>
                      <span className="error-inline"> — {item.error}</span>
                    </>
                  ) : (
                    <>
                      <code>{item.name}</code>
                      <span className="muted small">
                        {' '}— {item.payload.transport.type === 'http' ? `https ${item.payload.transport.url}` : `stdio ${item.payload.transport.command}`}
                        {item.payload.writable ? t('mcp.writableTag') : t('mcp.readonlyTag')}
                      </span>
                    </>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {err && <div className="error">{err}</div>}
      {msg && <div className="ok">{msg}</div>}

      {servers === null && <p className="muted small">{t('mcp.loading')}</p>}
      {servers !== null && servers.length === 0 && (
        <p className="muted small" style={{ marginTop: 8 }}>
          {t('mcp.emptyServers')}
          <code> npx -y @modelcontextprotocol/server-filesystem </code>{t('mcp.emptyServersTail')}
        </p>
      )}

      {servers?.map((s) => (
        <div key={s.id} className="mcp-row">
          <div className="mcp-main">
            <div className="mcp-name-line">
              {s.name}
              {s.tool_prefix && <span className="badge" style={{ marginLeft: 4 }}>{t('mcp.prefix', { p: s.tool_prefix })}</span>}
              <span className={`badge ${s.enabled ? 'ok' : 'muted'}`} style={{ marginLeft: 4 }}>
                {s.enabled ? t('mcp.enabled') : t('mcp.disabled')}
              </span>
              <span className="badge" style={{ marginLeft: 4 }}>{s.transport_type}</span>
              {s.perm_scope && (
                <span className="badge" style={{ marginLeft: 4, borderColor: 'var(--accent-strong)' }} title={t('mcp.permScopeTitle')}>
                  <ShieldCheck size={10} style={{ verticalAlign: '-1px', marginRight: 3 }} />
                  {s.perm_scope}
                </span>
              )}
              {s.writable && (
                <span className="badge warn" style={{ marginLeft: 4 }}>{t('mcp.writableBadge')}</span>
              )}
            </div>
            <div className="switch-desc">
              {s.transport_type === 'stdio' ? t('mcp.transportStdio') : t('mcp.transportHttp')}
              {s.enabled
                ? s.tool_count !== null
                  ? t('mcp.toolsAttached', { n: s.tool_count })
                  : s.error
                    ? t('mcp.connFailed', { msg: s.error })
                    : t('mcp.probing')
                : t('mcp.disabledNotAttached')}
            </div>
          </div>

          <div className="mcp-actions">
            {s.enabled && (
              <button
                className="ghost small"
                onClick={() => void doTest(s.id)}
                disabled={testingId === s.id || !isTauri}
                title={t('mcp.testTitle')}
              >
                <CheckCircle2 size={12} style={{ verticalAlign: '-2px', marginRight: 3 }} />
                {testingId === s.id ? t('mcp.testing') : t('mcp.test')}
              </button>
            )}
            <button
              className="ghost small"
              onClick={() => setEditing(s.id)}
              disabled={!manageOk}
              title={manageOk ? t('mcp.edit') : t('mcp.needPermShort')}
            >
              {t('mcp.edit')}
            </button>
            {pendingDelId === s.id ? (
              <>
                <button className="danger small" onClick={() => void doRemove(s.id)} disabled={!manageOk}>{t('mcp.confirmDelete')}</button>
                <button className="ghost small" onClick={() => setPendingDelId(null)}>{t('mcp.cancel')}</button>
              </>
            ) : (
              <button
                className="ghost small"
                onClick={() => setPendingDelId(s.id)}
                disabled={!manageOk}
                title={t('mcp.removeTitle')}
              >
                <Trash2 size={12} style={{ verticalAlign: '-2px', marginRight: 3 }} />
                {t('mcp.remove')}
              </button>
            )}
          </div>

          {s.enabled && (
            <div className="mcp-toggles">
              <div className="switch-row">
                <div className="mcp-toggle-text">
                  <div className="switch-desc">{t('mcp.attachToggle')}</div>
                </div>
                <input
                  type="checkbox"
                  className="switch"
                  checked={s.enabled}
                  onChange={(e) => void toggleEnabled(s.id, e.target.checked)}
                  disabled={!manageOk}
                />
              </div>
              <div className="switch-row">
                <div className="mcp-toggle-text">
                  <div className="switch-desc">{t('mcp.writableToggle')}</div>
                </div>
                <input
                  type="checkbox"
                  className="switch"
                  checked={s.writable}
                  onChange={(e) => void toggleWritable(s.id, e.target.checked)}
                  disabled={!manageOk}
                />
              </div>
            </div>
          )}
        </div>
      ))}

      {testResult && (
        <div className="mcp-test-panel">
          <div className="mcp-name-line" style={{ justifyContent: 'space-between' }}>
            <span className="switch-label">{t('mcp.testResultTitle')}</span>
            <button className="ghost icon" onClick={() => setTestResult(null)} title={t('mcp.close')}>
              <XCircle size={14} />
            </button>
          </div>
          {testResult.tools.length === 0 ? (
            <p className="muted small">{t('mcp.testNoTools')}</p>
          ) : (
            <ul className="mcp-tools-list">
              {testResult.tools.map((tool) => (
                <li key={tool.name}>
                  <code>{tool.name}</code>
                  <span className={`badge ${tool.perm === 'L0' ? 'ok' : tool.perm === 'L3' ? '' : 'warn'}`} style={{ marginLeft: 6 }}>{tool.perm}</span>
                  <span className="muted small"> — {tool.description.slice(0, 90)}{tool.description.length > 90 ? '…' : ''}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {editing !== null && (
        <ServerForm
          editingId={editing === 'new' ? null : editing}
          servers={servers ?? []}
          onCancel={() => setEditing(null)}
          onSaved={onSaved}
        />
      )}

      {!manageOk && (
        <p className="muted small" style={{ marginTop: 10 }}>
          <AlertTriangle size={12} style={{ verticalAlign: '-2px', marginRight: 3, color: 'var(--warn, #ea8600)' }} />
          {t('mcp.manageNeededLead')}<code>mcp.manage</code>{t('mcp.manageNeededTail')}
        </p>
      )}
    </div>
  );
}

/** 添加/编辑表单：name + transport 选择（stdio 命令/args/env/cwd 或 http url/headers/timeout）。 */
function ServerForm(props: {
  editingId: string | null;
  servers: McpServerStatus[];
  onCancel: () => void;
  onSaved: (cfg: McpServerPayload) => void;
}) {
  const { editingId, servers, onCancel, onSaved } = props;
  const t = useT();
  const existing = editingId ? servers.find((s) => s.id === editingId) : undefined;

  const [name, setName] = useState(existing?.name ?? '');
  const [enabled, setEnabled] = useState(existing?.enabled ?? true);
  const [writable, setWritable] = useState(existing?.writable ?? false);
  const [toolPrefix, setToolPrefix] = useState(existing?.tool_prefix ?? '');
  const [transportType, setTransportType] = useState<'stdio' | 'http'>(existing?.transport_type === 'http' ? 'http' : 'stdio');
  const [command, setCommand] = useState('');
  const [args, setArgs] = useState('');
  const [env, setEnv] = useState('');
  const [cwd, setCwd] = useState('');
  const [url, setUrl] = useState('');
  const [headers, setHeaders] = useState('');
  const [timeoutSecs, setTimeoutSecs] = useState('');
  // 工具级权限映射：工具名 → L0/L1/L2/L3（或具体 perm id）。以「name=level」行编辑。
  const [permMapText, setPermMapText] = useState('');
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // 编辑已有服务器时，把它的 transport 拆回表单字段。
  const prefillTransport = () => {
    if (!existing) return;
    const cfg = existing.transport;
    if (cfg.type === 'stdio') {
      setTransportType('stdio');
      setCommand(cfg.command ?? '');
      setArgs((cfg.args ?? []).join('\n'));
      setEnv(Object.entries(cfg.env ?? {}).map(([k, v]) => `${k}=${v}`).join('\n'));
      setCwd(cfg.cwd ?? '');
    } else {
      setTransportType('http');
      setUrl(cfg.url ?? '');
      setHeaders(Object.entries(cfg.headers ?? {}).map(([k, v]) => `${k}: ${v}`).join('\n'));
      setTimeoutSecs(cfg.timeout_secs != null ? String(cfg.timeout_secs) : '');
    }
    if (existing.permission_map && Object.keys(existing.permission_map).length > 0) {
      setPermMapText(
        Object.entries(existing.permission_map)
          .map(([k, v]) => `${k}=${v}`)
          .join('\n')
      );
    }
  };
  // 只在打开表单时预填一次。
  const [prefilled] = useState(() => { prefillTransport(); return true; });
  void prefilled;

  const buildPayload = (): McpServerPayload | null => {
    if (!name.trim()) { setErr(t('mcp.errName')); return null; }
    let transport: McpServerPayload['transport'];
    if (transportType === 'stdio') {
      if (!command.trim()) { setErr(t('mcp.errCommand')); return null; }
      const envObj: Record<string, string> = {};
      for (const line of env.split('\n')) {
        const i = line.indexOf('=');
        if (line.trim() && i > 0) envObj[line.slice(0, i).trim()] = line.slice(i + 1).trim();
      }
      transport = {
        type: 'stdio',
        command: command.trim(),
        args: args.split('\n').map((a) => a.trim()).filter(Boolean),
        env: envObj,
        cwd: cwd.trim() || null,
      };
    } else {
      if (!url.trim()) { setErr(t('mcp.errUrl')); return null; }
      const headersObj: Record<string, string> = {};
      for (const line of headers.split('\n')) {
        const i = line.indexOf(':');
        if (line.trim() && i > 0) headersObj[line.slice(0, i).trim()] = line.slice(i + 1).trim();
      }
      const secs = Number(timeoutSecs);
      transport = {
        type: 'http',
        url: url.trim(),
        headers: headersObj,
        timeout_secs: Number.isFinite(secs) && secs > 0 ? secs : null,
      };
    }
    const permMap: Record<string, string> = {};
    for (const line of permMapText.split('\n')) {
      const i = line.indexOf('=');
      const tool = line.slice(0, i).trim();
      const lvl = line.slice(i + 1).trim().toUpperCase();
      if (line.trim() && i > 0 && tool && lvl) permMap[tool] = lvl;
    }
    return {
      name: name.trim(),
      enabled,
      writable,
      tool_prefix: toolPrefix.trim() || null,
      permission_map: Object.keys(permMap).length > 0 ? permMap : null,
      transport,
    };
  };

  const save = async () => {
    const payload = buildPayload();
    if (!payload) return;
    setSaving(true);
    setErr(null);
    try {
      if (editingId) {
        await api.mcpUpdateServer(editingId, payload, true);
      } else {
        await api.mcpAddServer(payload, true);
      }
      onSaved(payload);
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="mcp-form">
      <div className="switch-label">{editingId ? t('mcp.formEditTitle') : t('mcp.formAddTitle')}</div>
      {err && <div className="error" style={{ marginTop: 6 }}>{err}</div>}

      <label className="field">
        <span>{t('mcp.fName')}</span>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder={t('mcp.fNamePlaceholder')} />
      </label>

      <label className="field">
        <span>{t('mcp.fTransport')}</span>
        <div className="seg seg-2">
          <button type="button" className={`seg-opt${transportType === 'stdio' ? ' active' : ''}`} onClick={() => setTransportType('stdio')}>{t('mcp.optStdio')}</button>
          <button type="button" className={`seg-opt${transportType === 'http' ? ' active' : ''}`} onClick={() => setTransportType('http')}>{t('mcp.optHttp')}</button>
        </div>
      </label>

      {transportType === 'stdio' ? (
        <>
          <label className="field">
            <span>{t('mcp.fCommand')}</span>
            <input value={command} onChange={(e) => setCommand(e.target.value)} placeholder="npx -y @modelcontextprotocol/server-filesystem" />
          </label>
          <label className="field">
            <span>{t('mcp.fArgs')}</span>
            <textarea value={args} onChange={(e) => setArgs(e.target.value)} rows={2} placeholder={editingId ? t('mcp.argsOptional') : t('mcp.argsPlaceholder')} />
          </label>
          <label className="field">
            <span>{t('mcp.fEnv')}</span>
            <textarea value={env} onChange={(e) => setEnv(e.target.value)} rows={2} placeholder="API_KEY=sk-xxx" />
          </label>
          <label className="field">
            <span>{t('mcp.fCwd')}</span>
            <input value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder={t('mcp.fCwdPlaceholder')} />
          </label>
        </>
      ) : (
        <>
          <label className="field">
            <span>{t('mcp.fUrl')}</span>
            <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://mcp.example.com/mcp" />
          </label>
          <label className="field">
            <span>{t('mcp.fHeaders')}</span>
            <textarea value={headers} onChange={(e) => setHeaders(e.target.value)} rows={2} placeholder="Authorization: Bearer sk-xxx" />
          </label>
          <label className="field">
            <span>{t('mcp.fTimeout')}</span>
            <input value={timeoutSecs} onChange={(e) => setTimeoutSecs(e.target.value)} placeholder={t('mcp.fTimeoutPlaceholder')} />
          </label>
        </>
      )}

      <div className="switch-row">
        <div>
          <div className="switch-label">{t('mcp.fEnabled')}</div>
          <div className="switch-desc">{t('mcp.fEnabledDesc')}</div>
        </div>
        <input type="checkbox" className="switch" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
      </div>
      <div className="switch-row">
        <div>
          <div className="switch-label">{t('mcp.fWritable')}</div>
          <div className="switch-desc">{t('mcp.fWritableDesc')}</div>
        </div>
        <input type="checkbox" className="switch" checked={writable} onChange={(e) => setWritable(e.target.checked)} />
      </div>
      <label className="field">
        <span>{t('mcp.fPrefix')}</span>
        <input value={toolPrefix} onChange={(e) => setToolPrefix(e.target.value)} placeholder={t('mcp.fPrefixPlaceholder')} />
      </label>

      <label className="field">
        <span>{t('mcp.fPermMap')}</span>
        <textarea
          value={permMapText}
          onChange={(e) => setPermMapText(e.target.value)}
          rows={3}
          placeholder={'read_dir=L0\nwrite_file=L2\nbios_flash=L3'}
          spellCheck={false}
        />
        <span className="muted small">{t('mcp.permMapHint')}</span>
      </label>

      {writable && (
        <p className="hint" style={{ background: '#fff3cd', borderLeft: '4px solid #ffc107', padding: '8px 12px', marginTop: 6 }}>
          <AlertTriangle size={12} style={{ verticalAlign: '-2px', marginRight: 4 }} />
          {t('mcp.writableWarn')}
        </p>
      )}

      <div className="modal-actions" style={{ marginTop: 12 }}>
        <button className="ghost" onClick={onCancel} disabled={saving}>{t('mcp.cancel')}</button>
        <button className="primary" onClick={() => void save()} disabled={saving}>
          {saving ? t('mcp.saving') : editingId ? t('mcp.saveChanges') : t('mcp.btnAdd')}
        </button>
      </div>
    </div>
  );
}
