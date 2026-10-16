import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { parseProposalFromText, renderArgsTemplate, validateManifestArgs, mcpToolRegistry, dynamicMcpTools, refreshMcpTools, clip, listNote, toolRegistry } from './tools';
import type { ToolManifestInvocation } from '../api';

const inv = (over: Partial<ToolManifestInvocation> = {}): ToolManifestInvocation =>
  ({
    mode: 'cli',
    launchable: true,
    args_template: '--demo {demo} {preset} --max-time {max_time}',
    params: [
      { name: 'demo', flag: '--demo', type: 'string', required: false, default: null, desc: '演示' },
      { name: 'preset', flag: '', type: 'enum', required: false, default: null, desc: '预设' },
      { name: 'max_time', flag: '--max-time', type: 'number', required: false, default: null, desc: '时长' },
      { name: 'benchmark', flag: '--benchmark', type: 'boolean', required: false, default: null, desc: '基准' },
    ],
    timeout_secs: 60,
    ...over,
  }) as ToolManifestInvocation;

describe('renderArgsTemplate', () => {
  it('renders non-boolean params by replacing placeholders in order', () => {
    const r = renderArgsTemplate(inv(), { demo: 'furmark-gl', max_time: 240 });
    expect(r).toBe('--demo furmark-gl --max-time 240');
  });

  it('returns null when no placeholder has a value', () => {
    const r = renderArgsTemplate(inv(), {});
    expect(r).toBeNull();
  });

  it('expands boolean flag when true and drops when absent', () => {
    const invB = inv({ args_template: '--demo {demo} {benchmark}',
      params: [
        { name: 'demo', flag: '--demo', type: 'string', required: false, default: null, desc: '演示' },
        { name: 'benchmark', flag: '--benchmark', type: 'boolean', required: false, default: null, desc: '基准' },
      ] });
    const r = renderArgsTemplate(invB, { demo: 'a', benchmark: true });
    expect(r).toBe('--demo a --benchmark');
    const r2 = renderArgsTemplate(invB, { demo: 'a' });
    expect(r2).toBe('--demo a');
  });

  it('renders eq-form boolean (flag=true) when template uses /admin={admin}', () => {
    const inv2 = inv({ args_template: '/export="{outfile}" /admin={admin}',
      params: [
        { name: 'outfile', flag: '/export', type: 'path', required: true, default: null, desc: '导出' },
        { name: 'admin', flag: '/admin', type: 'boolean', required: false, default: null, desc: '管理员' },
      ] });
    const r = renderArgsTemplate(inv2, { outfile: 'C:\\x.csv', admin: true });
    expect(r).toBe('/export="C:\\x.csv" /admin=true');
  });

  it('returns null for template without placeholders', () => {
    expect(renderArgsTemplate(inv({ args_template: '/CopyExit' }), {})).toBeNull();
  });
});

describe('validateManifestArgs', () => {
  it('rejects unknown parameter names', () => {
    const err = validateManifestArgs(inv(), { nope: 1 });
    expect(err).toContain('参数表里没有');
  });

  it('rejects missing required params', () => {
    const inv2 = inv({ params: [
      { name: 'target', flag: '', type: 'path', required: true, default: null, desc: '目标' },
      { name: 'outfile', flag: '/export', type: 'path', required: false, default: null, desc: '导出' },
    ] });
    const err = validateManifestArgs(inv2, { outfile: 'x' });
    expect(err).toContain('缺少必填参数');
  });

  it('passes valid args', () => {
    const inv2 = inv({ params: [
      { name: 'target', flag: '', type: 'path', required: true, default: null, desc: '目标' },
    ] });
    expect(validateManifestArgs(inv2, { target: 'C:\\' })).toBeNull();
  });
});

describe('parseProposalFromText', () => {
  it('returns null for text without a cleanup proposal marker', () => {
    expect(parseProposalFromText('你好，今天天气不错。')).toBeNull();
    expect(parseProposalFromText('')).toBeNull();
  });

  it('parses a fenced JSON with the propose_cleanup_plan wrapper', () => {
    const text = [
      '好的，这是我的清理建议：',
      '```json',
      JSON.stringify({
        propose_cleanup_plan: {
          title: '浏览器缓存',
          items: [
            { path: 'C:\\Users\\a\\AppData\\Local\\Temp', reason: '临时文件', risk: 'safe', what: '删除', purpose: '释放空间', impact: '无影响' },
          ],
        },
      }),
      '```',
    ].join('\n');
    const p = parseProposalFromText(text);
    expect(p).not.toBeNull();
    expect(p!.title).toBe('浏览器缓存');
    expect(p!.items).toHaveLength(1);
    expect(p!.items[0]).toMatchObject({ path: 'C:\\Users\\a\\AppData\\Local\\Temp', risk: 'safe' });
  });

  it('accepts the flat propose_cleanup wrapper and bare item JSON (whole-text)', () => {
    // parseFirstJson 只认「整段即 JSON」或围栏内容，纯文本夹裸 JSON 不在兜底范围
    const p = parseProposalFromText(JSON.stringify({ propose_cleanup: { title: 'T', items: [{ path: 'D:\\x' }] } }));
    expect(p).not.toBeNull();
    expect(p!.items[0].path).toBe('D:\\x');

    // 裸 items 也能兜底，但入口守卫要求文本里有 "propose_cleanup" 字面量
    const p2 = parseProposalFromText(JSON.stringify({ note: 'propose_cleanup', items: [{ path: 'E:\\y', risk: 'danger' }] }));
    expect(p2).not.toBeNull();
    expect(p2!.items[0].risk).toBe('danger');
  });

  it('normalizes unknown risk to caution and fills empty fields', () => {
    const text = '```json\n' + JSON.stringify({ note: 'propose_cleanup', items: [{ path: ' C:\\tmp ', risk: 'whatever' }] }) + '\n```';
    const p = parseProposalFromText(text);
    expect(p).not.toBeNull();
    expect(p!.items[0].risk).toBe('caution');
    expect(p!.items[0].path).toBe('C:\\tmp'); // trim 过
    expect(p!.title).toBe('清理清单'); // 默认标题
  });

  it('drops items with empty paths', () => {
    const text = '```json\n' + JSON.stringify({ note: 'propose_cleanup', items: [{ path: '', reason: 'x' }, { path: 'C:\\keep' }] }) + '\n```';
    const p = parseProposalFromText(text);
    expect(p).not.toBeNull();
    expect(p!.items.map((i) => i.path)).toEqual(['C:\\keep']);
  });

  it('caps the list at 10 items', () => {
    const items = Array.from({ length: 15 }, (_, i) => ({ path: `C:\\p${i}` }));
    const p = parseProposalFromText('```json\n' + JSON.stringify({ note: 'propose_cleanup', items }) + '\n```');
    expect(p!.items).toHaveLength(10);
    expect(p!.items[9].path).toBe('C:\\p9');
  });

  it('returns null when the JSON contains no usable items', () => {
    const p = parseProposalFromText('```json\n"propose_cleanup_plan"\n```');
    expect(p).toBeNull();
  });
});

describe('mcpToolRegistry（agent-server 工具集）', () => {
  it('registers exactly 76 tools（65 只读 + 11 写操作，含 AIDA64 复刻 14 个 + 装机验收 3 个 + 信息补全 4 个 + SuperIO 兜底 + 压测停止信号）', () => {
    expect(mcpToolRegistry).toHaveLength(76);
  });

  it('写操作工具 process_kill / service_control / fan_selfheal_fix / fan_control 存在', () => {
    const names = mcpToolRegistry.map((t) => t.name);
    expect(names).toContain('process_kill');
    expect(names).toContain('service_control');
    expect(names).toContain('fan_selfheal_fix');
    expect(names).toContain('fan_selfheal_diag');
    expect(names).toContain('fan_control');
    expect(names).toContain('file_recycle');
    expect(names).toContain('process_start');
    expect(names).toContain('scheduled_task_manage');
    expect(names).toContain('uninstall_app');
    expect(names).toContain('hw_sensors');
    expect(names).toContain('toolbelt_list');
    expect(names).toContain('toolbelt_run');
  });

  it('AIDA64 基准复刻 11 个工具存在（bench_cpu/memory/disk + stress_test + 报告/趋势/历史/告警 + 信息补全）', () => {
    const names = mcpToolRegistry.map((t) => t.name);
    for (const n of [
      'bench_cpu', 'bench_memory', 'bench_disk', 'stress_test',
      'system_report', 'sensor_trend', 'sensor_history', 'sensor_alert',
      'hw_cpu_features', 'hw_dram_timings', 'hw_displays',
    ]) {
      expect(names).toContain(n);
    }
    // stress_test 是写操作（hw.stress 权限门），描述应提示确认
    const stress = mcpToolRegistry.find((t) => t.name === 'stress_test')!;
    expect(stress.description).toContain('写操作');
    expect(stress.description).toContain('熔断');
    // bench_disk：默认 dry_run=true 只出计划不触发确认；dry_run=false 真跑才确认
    const bench = mcpToolRegistry.find((t) => t.name === 'bench_disk')!;
    expect(bench.description).toContain('dry_run');
    expect(bench.description).toContain('用户确认');
  });

  it('AIDA64 长尾 3 工具存在（bench_gpu / hw_ipmi / hw_acpi）', () => {
    const names = mcpToolRegistry.map((t) => t.name);
    for (const n of ['bench_gpu', 'hw_ipmi', 'hw_acpi']) {
      expect(names).toContain(n);
    }
    const gpu = mcpToolRegistry.find((t) => t.name === 'bench_gpu')!;
    expect(gpu.description).toContain('GPU');
    const ipmi = mcpToolRegistry.find((t) => t.name === 'hw_ipmi')!;
    expect(ipmi.description).toContain('IPMI');
  });

  it('装机验收 3 工具存在（mem_test / stress_test_gpu / bsod_analyze）', () => {
    const names = mcpToolRegistry.map((t) => t.name);
    for (const n of ['mem_test', 'stress_test_gpu', 'bsod_analyze']) {
      expect(names).toContain(n);
    }
    // mem_test 只读，描述含 MemTest86 简化版
    const mem = mcpToolRegistry.find((t) => t.name === 'mem_test')!;
    expect(mem.description).toContain('pattern');
    // stress_test_gpu 是写操作（hw.stress 权限门），描述提示确认
    const sg = mcpToolRegistry.find((t) => t.name === 'stress_test_gpu')!;
    expect(sg.description).toContain('写操作');
    expect(sg.description).toContain('熔断');
    // bsod_analyze 只读，描述含 bugcheck 中文映射
    const bsod = mcpToolRegistry.find((t) => t.name === 'bsod_analyze')!;
    expect(bsod.description).toContain('BlueScreenView');
  });

  it('信息补全 4 工具存在（app_licenses / net_adapter_detail / sys_defender_status / sys_user_accounts）', () => {
    const names = mcpToolRegistry.map((t) => t.name);
    for (const n of ['app_licenses', 'net_adapter_detail', 'sys_defender_status', 'sys_user_accounts']) {
      expect(names).toContain(n);
    }
    // app_licenses 含激活；net_adapter_detail 含子网掩码/MAC；defender 含实时保护；user_accounts 含隐私红线
    const lic = mcpToolRegistry.find((t) => t.name === 'app_licenses')!;
    expect(lic.description).toContain('激活');
    const netD = mcpToolRegistry.find((t) => t.name === 'net_adapter_detail')!;
    expect(netD.description).toContain('子网掩码');
    expect(netD.description).toContain('MAC');
    const def = mcpToolRegistry.find((t) => t.name === 'sys_defender_status')!;
    expect(def.description).toContain('实时保护');
    const ua = mcpToolRegistry.find((t) => t.name === 'sys_user_accounts')!;
    expect(ua.description).toContain('绝不输出');
  });

  it('all tools are tauriOnly (桌面端可用) and execute via api.callTool', () => {
    for (const t of mcpToolRegistry) {
      expect(t.tauriOnly, t.name).toBe(true);
      expect(typeof t.execute).toBe('function');
    }
  });

  it('does not collide with built-in tool names', () => {
    const builtin = new Set([
      'web_search', 'path_size', 'inspect_dir', 'propose_cleanup_plan', 'get_disk_health',
      'get_steam_library_summary', 'get_cleanup_suggestions', 'get_system_info',
      'run_system_probe', 'list_top_processes', 'get_tool_manifest', 'get_cli_tool_usage', 'run_cli_tool',
      'get_hardware_info', 'analyze_disk_health', 'generate_hw_report', 'run_hardware_test',
      'adjust_power_plan', 'list_plugins', 'install_plugin', 'uninstall_plugin',
      'market_plugins', 'activate_plugin', 'export_plugin',
    ]);
    for (const t of mcpToolRegistry) {
      expect(builtin.has(t.name), `duplicate tool name: ${t.name}`).toBe(false);
    }
  });
});

describe('dynamicMcpTools（用户 MCP 服务器）', () => {
  it('starts empty in non-Tauri (vitest 环境拉不到 mcp_list_tools)', () => {
    expect(dynamicMcpTools).toBeInstanceOf(Array);
  });

  it('refreshMcpTools 非 Tauri 下不动 mcpToolRegistry 静态表', async () => {
    const before = mcpToolRegistry.length;
    await refreshMcpTools();
    expect(mcpToolRegistry.length).toBe(before);
    expect(mcpToolRegistry).toHaveLength(76);
  });
});

describe('clip / listNote（截断必须自己说清楚）', () => {
  it('没超长度原样返回，不加任何废话', () => {
    expect(clip('abc', 10)).toBe('abc');
    expect(clip('abc', 3)).toBe('abc');
  });

  it('截断时说明还剩多少、显示了多少', () => {
    const text = 'x'.repeat(5000);
    const out = clip(text, 3000);
    expect(out.startsWith('x'.repeat(3000))).toBe(true);
    expect(out).toContain('还有 2000 个字符没显示');
    expect(out).toContain('以上是前 3000 个');
    expect(out).toContain('别把以上当成全部');
  });

  it('带 hint 时给出取回完整内容的路', () => {
    const out = clip('y'.repeat(100), 10, '完整正文已写入保存路径。');
    expect(out).toContain('完整正文已写入保存路径');
    expect(out).toContain('还有 90 个字符没显示');
  });

  it('列表只列前 N 条要说共 M 条；全列出不加注释', () => {
    expect(listNote(8, 8)).toBe('');
    expect(listNote(6, 20)).toContain('共 20 条');
    expect(listNote(6, 20)).toContain('剩下 14 条未列出');
  });
});

// R274：path_size / inspect_dir 只读测量工具在浏览器预览模式放行——
// 底层（estimateSize / inspect）都有 mock 演示数据，隐藏工具让 AI 只能
// 凭记忆猜路径大小/内容，与「预览模式 AI 建议可跑通」的目标脱节。
// 它们是只读无副作用测量，不是写操作，预览开放是诚实的。
describe('toolRegistry 只读测量工具预览模式可用（R274）', () => {
  it('path_size 不带 tauriOnly（预览模式 AI 可核实路径大小）', () => {
    const t = toolRegistry.find((x) => x.name === 'path_size')!;
    expect(t).toBeTruthy();
    expect(t.tauriOnly).toBeUndefined();
    expect(typeof t.execute).toBe('function');
    expect(t.required).toEqual(['path']);
  });

  it('inspect_dir 不带 tauriOnly（预览模式 AI 可核实目录内容）', () => {
    const t = toolRegistry.find((x) => x.name === 'inspect_dir')!;
    expect(t).toBeTruthy();
    expect(t.tauriOnly).toBeUndefined();
    expect(typeof t.execute).toBe('function');
    expect(t.required).toEqual(['path']);
  });
});

// R275：更多只读工具底层有 mock 演示数据（cleanupSuggestions /
// STEAM_INVENTORY / fanStatus），预览模式一并放行——只读枚举无副作用，
// 写操作类（run_cli_tool / adjust_fan_curve 等）保持 tauriOnly 藏着。
describe('toolRegistry 只读汇总工具预览模式可用（R275）', () => {
  const PREVIEW_READONLY = ['get_cleanup_suggestions', 'get_steam_library_summary', 'get_fan_status'];
  const STILL_TAURI_ONLY = ['run_system_probe', 'list_top_processes',
    'run_cli_tool', 'adjust_fan_curve', 'get_cli_tool_usage', 'get_tool_manifest'];

  for (const name of PREVIEW_READONLY) {
    it(`${name} 不带 tauriOnly（预览模式只读放行）`, () => {
      const t = toolRegistry.find((x) => x.name === name)!;
      expect(t, name).toBeTruthy();
      expect(t.tauriOnly, name).toBeUndefined();
      expect(typeof t.execute, name).toBe('function');
    });
  }

  for (const name of STILL_TAURI_ONLY) {
    it(`${name} 仍带 tauriOnly（写操作或底层无 mock，预览保持隐藏）`, () => {
      const t = toolRegistry.find((x) => x.name === name)!;
      expect(t, name).toBeTruthy();
      expect(t.tauriOnly, name).toBe(true);
    });
  }
});

// R276：get_disk_health 预览放行——listDrives 有 mock 演示数据源（与总览
// 磁盘卡同源），只读枚举无副作用，桌面端仍走 agent-server MCP 单源。
describe('toolRegistry get_disk_health 预览模式可用（R276）', () => {
  it('get_disk_health 不带 tauriOnly', () => {
    const t = toolRegistry.find((x) => x.name === 'get_disk_health')!;
    expect(t).toBeTruthy();
    expect(t.tauriOnly).toBeUndefined();
    expect(typeof t.execute).toBe('function');
  });
});

// R277：get_hardware_info / analyze_disk_health 预览放行——hwInfo /
// hwDiskHealth 有 mock 演示数据源（与「系统信息」页同源），只读枚举
// 无副作用；run_hardware_test（压测写系统）与 generate_hw_report
// （savePath 写文件）保持 tauriOnly + 权限门。
describe('toolRegistry 硬件只读工具预览模式可用（R277）', () => {
  const PREVIEW_HW_READONLY = ['get_hardware_info', 'analyze_disk_health'];
  const STILL_HW_TAURI_ONLY = ['run_hardware_test', 'generate_hw_report'];

  for (const name of PREVIEW_HW_READONLY) {
    it(`${name} 不带 tauriOnly（预览模式只读放行）`, () => {
      const t = toolRegistry.find((x) => x.name === name)!;
      expect(t, name).toBeTruthy();
      expect(t.tauriOnly, name).toBeUndefined();
      expect(typeof t.execute, name).toBe('function');
    });
  }

  for (const name of STILL_HW_TAURI_ONLY) {
    it(`${name} 仍带 tauriOnly（写系统/写文件，预览保持隐藏）`, () => {
      const t = toolRegistry.find((x) => x.name === name)!;
      expect(t, name).toBeTruthy();
      expect(t.tauriOnly, name).toBe(true);
    });
  }
});

describe('mcpTool 写工具权限门（R265）', () => {
  beforeEach(() => {
    // node 测试环境无 window：stub localStorage 供 isPermEnabled 读。
    // 默认 perms 全关（写工具 L2 默认不开）——构造与真机默认态一致。
    const store = new Map<string, string>();
    vi.stubGlobal('window', {
      localStorage: {
        getItem: (k: string) => store.get(k) ?? null,
        setItem: (k: string, v: string) => { store.set(k, v); },
        removeItem: (k: string) => { store.delete(k); },
      },
    });
    store.clear();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('写工具在权限未开时返回「未授权」指引，不弹确认、不发请求', async () => {
    const kill = mcpToolRegistry.find((t) => t.name === 'process_kill')!;
    const out = await kill.execute({ pid: 1234 }, { user: 'test' });
    expect(out).toContain('未授权');
    expect(out).toContain('AI 权限中心');
    // sys.control 是默认关的 L2——未开权限时绝不把写请求发给后端。
    expect(out).toContain('控制系统状态');
  });

  it('file_recycle 工具在权限未开时也走未授权指引（不是“用户取消”）', async () => {
    const rec = mcpToolRegistry.find((t) => t.name === 'file_recycle')!;
    const out = await rec.execute({ path: 'C:\\Users\\me\\Downloads\\x.tmp' }, { user: 'test' });
    expect(out).toContain('未授权');
    expect(out).toContain('回收文件到回收站');
  });

  it('写工具在权限已开时走确认门（取消则返回取消说明而非未授权）', async () => {
    // 打开 sys.control（L2）→ isPermEnabled 通过 → promptSessionConfirm 等 UI。
    const win = window as unknown as { localStorage: { getItem: (k: string) => string | null; setItem: (k: string, v: string) => void; removeItem: (k: string) => void } };
    win.localStorage.setItem('diskpilot.perms.v1', JSON.stringify({ enabled: { 'sys.control': true } }));
    const { resolveSessionConfirm } = await import('./tools');
    const kill = mcpToolRegistry.find((t) => t.name === 'process_kill')!;
    const p = kill.execute({ pid: 42 }, { user: 'test' }).then((out) => out);
    // 权限已开 → 挂起在确认卡 → 模拟用户点「取消」。
    resolveSessionConfirm({ run: false, remember: false });
    const out = await p;
    expect(out).toContain('已取消');
    expect(out).not.toContain('未授权');
  });
});
