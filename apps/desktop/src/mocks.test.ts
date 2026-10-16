// 预览模式（非 Tauri）清理演示数据的自洽性测试：
// scopeSizesBatch 必须按 scaffold 的 scopes 返回尺寸（不是空表），
// executeScope 的 dry-run 只出清单、真删才追加台账——保证浏览器里
// 「勾选 → 预览 → 真删 → 历史页可见」整条 demo 链路成立。
import { describe, it, expect } from 'vitest';
import { SCAFFOLDS, scopeSizesBatch, executeScope, listUndo, cleanupSuggestions, listDrives, getSpaceHistory, listStartupItems, setStartupItem, removeStartupItem, listInstalledDrivers, checkSpaceForecast, generateWeeklyReport, getWeeklyReports, mcpListServers, mcpAuditTail, scanDuplicateFiles, estimateSize, fanCurveAdvice, hwInfo, hwDiskHealth, runReminderCheck, getReminderConfig, setReminderConfig, pluginMarket, pluginListInstalled, scaffoldRegistryList, powerPlan, listCondaEnvs, MOCK_TREE } from './mocks';

describe('mocks.listCondaEnvs（清理页 Conda 脚本环境区演示数据）', () => {
  it('返回 base + ≥2 个用户环境，base 不可勾选、stale 环境默认勾选', async () => {
    const envs = await listCondaEnvs();
    const base = envs.find((e) => e.is_base);
    expect(base).toBeTruthy();
    expect(base?.default_checked).toBe(false);
    const users = envs.filter((e) => !e.is_base);
    expect(users.length).toBeGreaterThanOrEqual(2);
    for (const e of envs) {
      expect(e.name).toBeTruthy();
      expect(e.path).toBeTruthy();
      expect(e.size_bytes).toBeGreaterThan(0);
    }
    // 至少一个用户环境默认勾选（stale 90d 推荐清理）
    expect(users.some((e) => e.default_checked)).toBe(true);
  });
});

describe('mocks.powerPlan（系统优化页电源计划演示数据）', () => {
  it('返回 3 个标准计划 + 当前活动计划（均衡）', async () => {
    const p = await powerPlan();
    expect(p.schemes.length).toBeGreaterThanOrEqual(3);
    // 标准 GUID：均衡/高性能/节能
    expect(p.schemes).toContain('381b4222-f694-41f0-9685-ff5bb260df2e');
    expect(p.schemes).toContain('8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c');
    expect(p.schemes).toContain('a1841308-3541-4fab-bc81-f71556f20b4a');
    // active 必在 schemes 内
    expect(p.active).toBeTruthy();
    expect(p.schemes).toContain(p.active);
  });
});

describe('mocks.scaffoldRegistryList（清理模板库社区脚本演示数据）', () => {
  it('返回 ≥3 条带签名与下载量的社区脚本', async () => {
    const r = await scaffoldRegistryList();
    expect(r.items.length).toBeGreaterThanOrEqual(3);
    for (const it of r.items) {
      expect(it.id).toBeTruthy();
      expect(it.name).toBeTruthy();
      expect(it.description).toBeTruthy();
      expect(it.risk).toBeTruthy();
      // 签名徽标（signed）可渲染
      expect(it.signature).toBeTruthy();
      expect(it.signer).toBeTruthy();
      expect(it.downloads).toBeGreaterThanOrEqual(0);
    }
    // 有「即将上线」（url 为空 → comingSoon）以外的 ready 条目：url 非空
    expect(r.items.some((it) => it.url.trim().length > 0)).toBe(true);
    expect(r.updated_at).toBeGreaterThan(0);
  });
});

describe('mocks.mcpAuditTail（MCP 页审计面板演示数据）', () => {
  it('返回含 call_tool 成功/拒绝 + 服务器增删的审计记录', async () => {
    const rows = await mcpAuditTail(80);
    expect(rows.length).toBeGreaterThanOrEqual(4);
    const ops = rows.map((r) => r.op);
    expect(ops).toContain('call_tool');
    expect(ops).toContain('add_server');
    // 有 granted:false 的拒绝记录（审计「已拒绝」徽标可演示）
    expect(rows.some((r) => r.granted === false)).toBe(true);
    // 有成功调用（CheckCircle 可演示）
    expect(rows.some((r) => r.ok === true)).toBe(true);
    for (const r of rows) expect(r.ts).toBeGreaterThan(0);
  });
});

describe('mocks.pluginMarket + pluginListInstalled（插件市场 tab 演示数据）', () => {
  it('pluginMarket 返回 3 个内置工具、2 个已插件化', async () => {
    const list = await pluginMarket();
    expect(list.length).toBeGreaterThanOrEqual(3);
    for (const p of list) {
      expect(p.id).toBeTruthy();
      expect(p.name).toBeTruthy();
      expect(p.category).toBeTruthy();
      expect(typeof p.installed).toBe('boolean');
    }
    expect(list.filter((p) => p.installed).length).toBeGreaterThanOrEqual(2);
  });

  it('pluginListInstalled 返回台账 + 历史事件（可演示回滚/更新按钮）', async () => {
    const r = await pluginListInstalled();
    expect(r.installed.length).toBeGreaterThanOrEqual(2);
    for (const p of r.installed) {
      expect(p.id).toBeTruthy();
      expect(p.version).toBeTruthy();
      expect(p.installed_at).toBeGreaterThan(0);
      expect(Array.isArray(p.history)).toBe(true);
    }
    // 至少一条有 ≥2 个历史版本（更新按钮 + 回滚按钮都出现）
    expect(r.installed.some((p) => p.history.length >= 2)).toBe(true);
    expect(r.total_events).toBeGreaterThan(0);
  });
});

describe('mocks.reminder（清理提醒开关/阈值 + 立即检查演示数据）', () => {
  it('开关在会话内可切换（不再点了弹回关闭）', async () => {
    const before = await getReminderConfig();
    expect(before.enabled).toBe(false);
    const on = await setReminderConfig({ enabled: true, min_bytes: 500 * 1024 * 1024 });
    expect(on.enabled).toBe(true);
    expect(on.min_bytes).toBe(500 * 1024 * 1024);
    const back = await getReminderConfig();
    expect(back.enabled).toBe(true);
    await setReminderConfig({ enabled: false });
  });

  it('runReminderCheck 返回 C 盘 2.91GB 提醒负载（与清理建议 mock 同源）', async () => {
    const p = await runReminderCheck();
    expect(p.drives.length).toBeGreaterThan(0);
    expect(p.drives[0].path).toBe('C:\\');
    expect(p.total_bytes).toBeGreaterThan(0);
    expect(p.ts).toBeGreaterThan(0);
  });
});

describe('mocks.hwInfo + hwDiskHealth（体检卡硬盘健康格 + 工具页硬件面板演示数据）', () => {
  it('hwInfo 返回 CPU/内存/主板/显卡齐全的静态信息', async () => {
    const info = await hwInfo();
    expect(info.cpu).toBeTruthy();
    expect((info.cpu as Array<Record<string, unknown>>)[0].Name).toContain('Intel');
    expect(info.memory).toBeTruthy();
    expect(info.board).toBeTruthy();
    expect(info.os).toBeTruthy();
  });

  it('hwDiskHealth 返回 C 盘 SSD Healthy（体检卡可显示全部健康）', async () => {
    const h = await hwDiskHealth();
    expect(h.reliability).toBe(true);
    const disks = h.disks as Array<Record<string, unknown>>;
    expect(disks.length).toBeGreaterThan(0);
    expect(String(disks[0].health).toLowerCase()).toContain('healthy');
    expect(disks[0].drive_letters).toBe('C');
  });
});

describe('mocks.fanCurveAdvice（总览温度格 + 系统工具风扇建议演示数据）', () => {
  it('返回均衡档建议 + 温度 52°C（与周报 mock 同源）+ 曲线锚点', async () => {
    const a = await fanCurveAdvice();
    expect(a.temp_max_c).toBe(52);
    expect(a.cpu_load_pct).not.toBeNull();
    expect(a.level).toBeTruthy();
    expect(a.level_label).toBeTruthy();
    expect(a.curve.length).toBeGreaterThanOrEqual(4);
    for (const p of a.curve) {
      expect(p.load_pct).toBeGreaterThanOrEqual(0);
      expect(p.fan_pct).toBeGreaterThanOrEqual(0);
    }
  });
});

describe('mocks.scanDuplicateFiles（总览重复文件查找演示数据）', () => {
  it('返回 ≥2 组重复：每组保留 1 份、其余进回收候选', async () => {
    const groups = await scanDuplicateFiles('C:\\');
    expect(groups.length).toBeGreaterThanOrEqual(2);
    for (const g of groups) {
      expect(g.size).toBeGreaterThan(0);
      expect(g.files.length).toBeGreaterThanOrEqual(2);
      expect(g.keep).toBeTruthy();
      expect(g.files.some((f) => f.path === g.keep)).toBe(true);
      expect(g.recycle_candidates.length).toBeGreaterThanOrEqual(1);
      for (const c of g.recycle_candidates) expect(c).not.toBe(g.keep);
      expect(g.running).toEqual([]);
    }
  });
});

describe('mocks.estimateSize（确认窗逐项实测大小演示数据）', () => {
  it('重复文件的回收候选路径命中已知表并返回组大小（不再是恒 0 误标「路径不存在」）', async () => {
    const groups = await scanDuplicateFiles('C:\\');
    for (const g of groups) {
      for (const c of g.recycle_candidates) {
        const sz = await estimateSize(c);
        expect(sz).toBe(g.size);
      }
    }
  });

  it('不在已知表的路径返回 0（与后端「路径不存在/空」同语义）', async () => {
    await expect(estimateSize('C:\\不存在\\路径\\x.bin')).resolves.toBe(0);
  });
});

describe('mocks.mcpListServers（设置页 MCP tab 演示数据）', () => {
  it('返回 stdio/http 混合样例服务器（列表与权限摘要可演示）', async () => {
    const servers = await mcpListServers();
    expect(servers.length).toBeGreaterThan(0);
    const transports = new Set(servers.map((s) => s.transport_type));
    expect(transports.has('stdio')).toBe(true);
    expect(transports.has('http')).toBe(true);
    for (const s of servers) {
      expect(s.id).toBeTruthy();
      expect(s.name).toBeTruthy();
      expect(s.perm_scope).toBeTruthy();
    }
  });

  it('enabled 服务器有工具数，停用的不编造（tool_count=null）', async () => {
    const servers = await mcpListServers();
    const enabledOnes = servers.filter((s) => s.enabled);
    const disabledOnes = servers.filter((s) => !s.enabled);
    expect(enabledOnes.length).toBeGreaterThan(0);
    for (const s of enabledOnes) expect(s.tool_count).toBeGreaterThan(0);
    for (const s of disabledOnes) expect(s.tool_count).toBeNull();
  });
});

describe('mocks 周报（历史页周体检卡演示数据）', () => {
  it('未生成过时 getWeeklyReports 出一份样例（环比一开始可看）', async () => {
    const list = await getWeeklyReports(2);
    expect(list.length).toBe(1);
    const r = list[0];
    expect(r.total_bytes).toBeGreaterThan(0);
    expect(r.drives.length).toBeGreaterThan(0);
    expect(r.ts).toBeGreaterThan(0);
  });

  it('generate 写入归档，随后 getWeeklyReports 可读到（最新在前）', async () => {
    const a = await getWeeklyReports(10);
    const gen = await generateWeeklyReport();
    const b = await getWeeklyReports(10);
    expect(b.length).toBe(a.length + 1);
    expect(b[0].ts).toBe(gen.ts);
  });
});

describe('mocks.checkSpaceForecast（清理提醒/体检空间外推演示数据）', () => {
  it('返回 C 盘预计满盘天数（与趋势卡约 112 天同源，reason=ok 不告警）', async () => {
    const rows = await checkSpaceForecast();
    expect(rows.length).toBe(1);
    const c = rows[0];
    expect(c.path).toBe('C:\\');
    expect(c.days).toBeGreaterThan(0);
    expect(c.reason).toBe('ok');
    expect(c.warn).toBe(false);
  });
});

describe('mocks.listInstalledDrivers（系统工具驱动检查演示数据）', () => {
  it('返回覆盖常见类别的样例驱动（类别筛选可演示）', async () => {
    const drivers = await listInstalledDrivers();
    expect(drivers.length).toBeGreaterThan(0);
    const classes = new Set(drivers.map((d) => d.class));
    expect(classes.has('display')).toBe(true);
    expect(classes.has('network')).toBe(true);
    expect(classes.has('sound')).toBe(true);
    expect(classes.has('storage')).toBe(true);
    for (const d of drivers) {
      expect(d.name).toBeTruthy();
      expect(d.provider).toBeTruthy();
      expect(d.version).toBeTruthy();
      expect(d.date).toBeTruthy();
    }
  });

  it('两次调用返回独立副本（调用方改动不污染模块级数据）', async () => {
    const a = await listInstalledDrivers();
    const b = await listInstalledDrivers();
    a[0].version = '9.9.9.9';
    expect(b[0].version).not.toBe('9.9.9.9');
  });
});

describe('mocks.listStartupItems（设置页系统工具演示数据）', () => {
  it('返回覆盖四个来源分组的样例启动项（搜索/分组可演示）', async () => {
    const items = await listStartupItems();
    expect(items.length).toBeGreaterThan(0);
    const locations = new Set(items.map((i) => i.location));
    expect(locations.has('registry_hkcu')).toBe(true);
    expect(locations.has('folder_user')).toBe(true);
    expect(locations.has('registry_hklm')).toBe(true);
    expect(locations.has('folder_machine')).toBe(true);
    for (const it of items) {
      expect(it.id).toBeTruthy();
      expect(it.name).toBeTruthy();
      expect(it.target).toBeTruthy();
    }
  });

  it('两次调用返回独立副本（调用方改动不污染模块级数据）', async () => {
    const a = await listStartupItems();
    const b = await listStartupItems();
    a[0].enabled = !a[0].enabled;
    expect(b[0].enabled).not.toBe(a[0].enabled);
  });

  it('启停/删除在会话内真实生效（不再假成功）', async () => {
    // hkcu-obs 初始禁用 → 启用后列表反映（内存态持久跨 list 调用）
    await setStartupItem('hkcu-obs', true);
    let items = await listStartupItems();
    expect(items.find((i) => i.id === 'hkcu-obs')?.enabled).toBe(true);
    // 删除 machine-hp（文件夹项）→ 返回空 = 回收站接住，且列表不再含它
    const backup = await removeStartupItem('machine-hp');
    expect(backup).toBe('');
    items = await listStartupItems();
    expect(items.find((i) => i.id === 'machine-hp')).toBeUndefined();
    // 注册表项删除返回 .reg 备份路径（演示）
    const backupReg = await removeStartupItem('hklm-nv');
    expect(backupReg.length).toBeGreaterThan(0);
    expect(backupReg.includes('.reg')).toBe(true);
    // 不存在 id：删除返回空、不报错
    await expect(removeStartupItem('no-such-id')).resolves.toBe('');
    // 自愈：把改过的状态恢复，避免影响同文件其他测试
    await setStartupItem('hkcu-obs', false);
    // machine-hp / hklm-nv 已删，无法恢复——放在最后执行且不依赖它们的测试在后
  });
});

describe('mocks.getSpaceHistory（总览空间趋势演示数据）', () => {
  it('返回 C 盘 ≥5 个采样点（趋势卡判定 rich）且总量与 listDrives 一致', async () => {
    const hist = await getSpaceHistory();
    const pts = hist['C:\\'];
    expect(pts).toBeDefined();
    expect(pts.length).toBeGreaterThanOrEqual(5);
    for (const p of pts) {
      expect(p.root).toBe('C:\\');
      expect(p.total_bytes).toBeGreaterThan(0);
      expect(p.used_bytes).toBeGreaterThan(0);
      expect(p.used_bytes).toBeLessThanOrEqual(p.total_bytes);
      expect(p.at).toBeGreaterThan(0);
    }
    // 时间升序（趋势按序画线）
    for (let i = 1; i < pts.length; i++) expect(pts[i].at).toBeGreaterThan(pts[i - 1].at);
  });
});

describe('mocks.listDrives（总览磁盘详情演示数据）', () => {
  it('返回与 MOCK_TREE 同源的一块 C 盘（非空，总览不再显示未检测到磁盘）', async () => {
    const drives = await listDrives();
    expect(drives.length).toBe(1);
    const c = drives[0];
    expect(c.path).toBe('C:\\');
    expect(c.total_bytes).toBeGreaterThan(0);
    expect(c.used_bytes).toBeGreaterThan(0);
    expect(c.free_bytes).toBe(c.total_bytes - c.used_bytes);
  });
});

describe('mocks.cleanupSuggestions（总览「可清理」卡片演示数据）', () => {
  it('对命中目录返回建议项：脚本命中与 MOCK_TREE 的 scaffold_id 一致', async () => {
    const list = await cleanupSuggestions('C:\\Users\\90740\\AppData\\Local', undefined, true);
    // MOCK_TREE 的 AppData\Local 下命中 browser-cache / dev-caches / ide-caches / docker-buildx
    const ids = new Set(list.map((r) => r.scaffold_id));
    expect(ids.has('browser-cache')).toBe(true);
    expect(ids.has('dev-caches')).toBe(true);
    expect(ids.has('wechat-pc')).toBe(false); // 微信在 Documents，不在 Local
  });

  it('建议项字节 > 0 且 scope_id 存在于对应脚本', async () => {
    const list = await cleanupSuggestions('C:\\Users\\90740\\AppData\\Local', undefined, true);
    for (const r of list) {
      expect(r.bytes).toBeGreaterThan(0);
      const sc = SCAFFOLDS.find((s) => s.id === r.scaffold_id);
      expect(sc).toBeTruthy();
      expect(sc!.scopes.some((s) => s.id === r.scope_id)).toBe(true);
    }
  });

  it('未命中目录返回空列表（不编造）', async () => {
    const list = await cleanupSuggestions('Z:\\nonexistent', undefined, true);
    expect(list).toEqual([]);
  });
});

describe('mocks.scopeSizesBatch（预览模式 scope 尺寸）', () => {
  it('对命中脚本返回与 scopes 一一对应的尺寸表，而非空表', async () => {
    const scaffold = SCAFFOLDS.find((s) => s.id === 'browser-cache')!;
    const roots = ['C:\\Users\\90740\\AppData\\Local\\Microsoft\\Edge'];
    const batch = await scopeSizesBatch('browser-cache', roots);
    expect(batch).toHaveLength(roots.length);
    const sizes = batch[0].sizes;
    expect(sizes.length).toBe(scaffold.scopes.length);
    for (const sc of scaffold.scopes) {
      const row = sizes.find((r) => r.scope_id === sc.id);
      expect(row).toBeTruthy();
      expect(row!.bytes).toBeGreaterThan(0);
      expect(row!.file_count).toBeGreaterThan(0);
      expect(row!.total_bytes).toBe(row!.bytes);
    }
  });

  it('同 scope 两次查询返回相同尺寸（确定性，刷新不跳变）', async () => {
    const a = await scopeSizesBatch('dev-caches', ['C:\\']);
    const b = await scopeSizesBatch('dev-caches', ['C:\\']);
    expect(a[0].sizes).toEqual(b[0].sizes);
  });

  it('未知脚本返回空表（不编造尺寸）', async () => {
    const batch = await scopeSizesBatch('no-such-scaffold', ['C:\\']);
    expect(batch[0].sizes).toEqual([]);
  });

  // R212：天数联动——带 prompt.days 的 scope 传 scopeDays 后字节应随天数
  // 缩小（真实后端按 mtime 过滤，天数越大可清越少）。此前 mock 忽略
  // scopeDays，改天数「待清」纹丝不动，预览演示上天数选择形同虚设。
  it('scopeDays 使可清字节随天数缩小（R212 天数联动）', async () => {
    const scaffold = SCAFFOLDS.find((s) => s.id === 'crash-dumps')!;
    const daysScope = scaffold.scopes.find((sc) => sc.prompt?.kind === 'days')!;
    const noDays = await scopeSizesBatch('crash-dumps', ['C:\\']);
    const days1 = await scopeSizesBatch('crash-dumps', ['C:\\'], { [daysScope.id]: 1 });
    const days30 = await scopeSizesBatch('crash-dumps', ['C:\\'], { [daysScope.id]: 30 });
    const baseRow = noDays[0].sizes.find((r) => r.scope_id === daysScope.id)!;
    const d1Row = days1[0].sizes.find((r) => r.scope_id === daysScope.id)!;
    const d30Row = days30[0].sizes.find((r) => r.scope_id === daysScope.id)!;
    // 1 天几乎全清（保留期最短）→ 比 30 天多；默认无 days 走 prompt.default（30 天）≈ 30 天
    expect(d1Row.bytes).toBeGreaterThan(d30Row.bytes);
    expect(baseRow.bytes).toBe(d30Row.bytes);
    expect(d30Row.bytes).toBeGreaterThan(0);
  });

  it('无 prompt.days 的 scope 不受 scopeDays 影响（R212 不误伤）', async () => {
    const scaffold = SCAFFOLDS.find((s) => s.id === 'browser-cache')!;
    const noPrompt = scaffold.scopes.find((sc) => sc.prompt?.kind !== 'days')!;
    const a = await scopeSizesBatch('browser-cache', ['C:\\'], { [noPrompt.id]: 7 });
    const b = await scopeSizesBatch('browser-cache', ['C:\\']);
    const ra = a[0].sizes.find((r) => r.scope_id === noPrompt.id)!;
    const rb = b[0].sizes.find((r) => r.scope_id === noPrompt.id)!;
    expect(ra.bytes).toBe(rb.bytes);
  });
});

describe('mocks.executeScope（预览模式清理执行）', () => {
  it('dry-run 返回样例清单且不写入台账', async () => {
    const before = await listUndo(500);
    const entries = await executeScope('browser-cache', 'chrome-cache', 'C:\\Users\\a\\Chrome', true, {});
    expect(entries.length).toBeGreaterThan(0);
    for (const e of entries) {
      expect(e.source).toContain('C:\\Users\\a\\Chrome');
      expect(e.action).toBe('recycle');
    }
    const after = await listUndo(500);
    expect(after.length).toBe(before.length);
  });

  it('真删返回回收条目并追加到台账（历史页可见）', async () => {
    const before = await listUndo(500);
    const entries = await executeScope('dev-caches', 'npm-cache', 'C:\\Users\\a\\npm', false, { confirmed: true });
    expect(entries.length).toBeGreaterThan(0);
    const after = await listUndo(500);
    expect(after.length).toBe(before.length + entries.length);
    // 台账新条目是这条真删的回收项
    const added = after.slice(0, entries.length);
    for (const e of added) {
      expect(e.action).toBe('recycle');
      expect(e.source).toContain('C:\\Users\\a\\npm');
    }
  });

  it('dry-run 条目数有限（demo 列表不无限膨胀）', async () => {
    const entries = await executeScope('wechat-pc', 'received-files', 'C:\\Users\\a\\xwechat', true, {});
    expect(entries.length).toBeLessThanOrEqual(64);
  });
});

// R205：隐私清理页（tagFilter="privacy" 的 CleanupPage）命中 MOCK_TREE——
// windows-privacy（Recent / thumbcache / INetCache）与 browser-privacy
// （Firefox 配置）不再是恒「未检测到」，预览演示能看到真实痕迹体量。
import { collectScaffoldCards } from './cleanup/matches';

describe('mocks 隐私脚本命中（R205 隐私清理页演示数据）', () => {
  it('browser-privacy（浏览器使用痕迹）在 MOCK_TREE 命中 Firefox 配置', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'browser-privacy');
    expect(card).toBeTruthy();
    expect(card!.matches.length).toBeGreaterThan(0);
    // 命中路径含 Firefox 配置（真实浏览痕迹目录）
    expect(card!.matches.some((m) => /Firefox/i.test(m.path))).toBe(true);
    expect(card!.totalSize).toBeGreaterThan(0);
  });

  it('windows-privacy（Windows 活动与使用痕迹）命中 Recent / thumbcache', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'windows-privacy');
    expect(card).toBeTruthy();
    expect(card!.matches.length).toBeGreaterThan(0);
    // visit 命中 scaffold_id 即收集不下钻：拿到的是命中目录本身
    // （Recent / Explorer / INetCache 任一挂 scaffold_id 的顶层目录）。
    expect(card!.matches.length).toBeGreaterThanOrEqual(2);
    expect(card!.totalSize).toBeGreaterThan(0);
  });

  it('browser-cache（浏览器缓存）命中不受新增隐私节点影响', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'browser-cache');
    expect(card).toBeTruthy();
    expect(card!.matches.length).toBeGreaterThanOrEqual(2); // Edge User Data + Chrome
    expect(card!.totalSize).toBeGreaterThan(10 * 1024 ** 3);
  });

  // R206：CleanupBody 按 category 分四组（media/cache/backup/privacy）渲染
  // scope 勾选行。隐私脚本的 scope 必须全部带 category='privacy'，否则落不进
  // 任何组导致勾选行不渲染（R206 修复前的缺陷：隐私页只有覆盖统计无可勾项）。
  it('隐私脚本全部 scope 带 category=privacy（R206 分组渲染不变量）', () => {
    const privacyScaffolds = SCAFFOLDS.filter((s) => s.tags?.includes('privacy'));
    expect(privacyScaffolds.length).toBeGreaterThan(0);
    for (const sc of privacyScaffolds) {
      expect((sc.scopes ?? []).length).toBeGreaterThan(0);
      for (const scope of sc.scopes ?? []) {
        expect(scope.category).toBe('privacy');
      }
    }
  });

  // R207：补齐真实 Windows 必有目录的演示命中——%TEMP%（system-temp）、
  // CrashDumps（crash-dumps）、Firefox cache2/shader-cache（firefox-cache）。
  // 同一 Firefox 树：Profiles 归 browser-privacy，cache2/shader-cache 归
  // firefox-cache（visit 命中即不下钻，互不抢占）。
  it('system-temp 命中 %TEMP%（R207 系统临时文件演示）', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'system-temp');
    expect(card).toBeTruthy();
    expect(card!.matches.some((m) => /\\Temp$/i.test(m.path))).toBe(true);
    expect(card!.totalSize).toBeGreaterThan(0);
  });

  it('crash-dumps 命中 CrashDumps（R207 崩溃转储演示）', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'crash-dumps');
    expect(card).toBeTruthy();
    expect(card!.matches.some((m) => /CrashDumps/i.test(m.path))).toBe(true);
    expect(card!.totalSize).toBeGreaterThan(0);
  });

  it('firefox-cache 命中 cache2 且不与 browser-privacy 抢 Profiles（R207）', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const fc = cards.find((c) => c.scaffold.id === 'firefox-cache');
    const bp = cards.find((c) => c.scaffold.id === 'browser-privacy');
    expect(fc).toBeTruthy();
    expect(fc!.matches.some((m) => /cache2|shader-cache/i.test(m.path))).toBe(true);
    // firefox-cache 不得吞掉 Profiles（那是 browser-privacy 的命中点）
    expect(fc!.matches.some((m) => /Profiles/i.test(m.path))).toBe(false);
    expect(bp).toBeTruthy();
    expect(bp!.matches.some((m) => /Profiles/i.test(m.path))).toBe(true);
  });

  // R208：Windows 更新缓存（windows-update）——C:\Windows\
  // SoftwareDistribution\Download 每台 Windows 必有，预览演示更新缓存清理。
  it('windows-update 命中 SoftwareDistribution（R208 Windows 更新缓存演示）', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'windows-update');
    expect(card).toBeTruthy();
    expect(card!.matches.some((m) => /SoftwareDistribution/i.test(m.path))).toBe(true);
    expect(card!.totalSize).toBeGreaterThan(0);
    // 命中目录下存在更新包文件（.cab / .msu）——只有目录名没有真实更新包
    // 会让「清理」失去意义，这里校验命中体量来自真实结构。
    expect(card!.totalSize).toBeGreaterThan(1024 ** 3);
  });

  // R209：腾讯 QQ（经典版）——%APPDATA%\Tencent\QQ 真实常驻，预览演示
  // QQ 接收文件/图片清理（media 组可勾选）。
  it('qq-pc 命中 Tencent\\QQ 接收文件（R209 QQ 演示）', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'qq-pc');
    expect(card).toBeTruthy();
    expect(card!.matches.some((m) => /Tencent\\QQ/i.test(m.path))).toBe(true);
    expect(card!.totalSize).toBeGreaterThan(0);
    // 命中目录含 FileRecv（接收的文件）——QQ 演示的核心可清内容
    expect(card!.totalSize).toBeGreaterThan(1024 ** 3);
  });

  // R210：Unreal 派生数据缓存（engine-caches）——%LOCALAPPDATA%\
  // UnrealEngine\Common\DerivedDataCache 真实常驻大头，预览演示引擎缓存清理。
  it('engine-caches 命中 UnrealEngine 目录（R210 游戏引擎缓存演示）', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'engine-caches');
    expect(card).toBeTruthy();
    // visit 命中 scaffold_id 即不下钻：matches 是顶层 UnrealEngine 节点，
    // 不是内部的 DerivedDataCache（与 R205 同一机制）。
    expect(card!.matches.some((m) => /UnrealEngine/i.test(m.path))).toBe(true);
    expect(card!.totalSize).toBeGreaterThan(1024 ** 3);
  });

  // R211：OBS Studio——%APPDATA%\obs-studio 真实常驻（直播/录屏用户高频），
  // 预览演示 OBS 崩溃转储/日志清理。
  it('obs-cache 命中 obs-studio（R211 OBS 演示）', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'obs-cache');
    expect(card).toBeTruthy();
    expect(card!.matches.some((m) => /obs-studio/i.test(m.path))).toBe(true);
    expect(card!.totalSize).toBeGreaterThan(1024 ** 3);
  });

  // R213：Microsoft Teams 双变体——1.x 经典版（%APPDATA%\Microsoft\Teams）
  // 与 2.x 新版（%LOCALAPPDATA%\Packages\MSTeams_8wekyb3d8bbwe）各一个命中
  // 目录，R210 的 detectVariants 修复让两组 scope 同屏渲染。
  it('teams 双变体命中：1.x 经典 + 2.x 新版（R213 变体同屏演示）', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'teams');
    expect(card).toBeTruthy();
    expect(card!.matches.some((m) => /Microsoft\\Teams/i.test(m.path))).toBe(true);
    expect(card!.matches.some((m) => /MSTeams_8wekyb3d8bbwe/i.test(m.path))).toBe(true);
    expect(card!.totalSize).toBeGreaterThan(1024 ** 3);
  });
});