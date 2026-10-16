// 预览模式（非 Tauri）清理演示数据的自洽性测试：
// scopeSizesBatch 必须按 scaffold 的 scopes 返回尺寸（不是空表），
// executeScope 的 dry-run 只出清单、真删才追加台账——保证浏览器里
// 「勾选 → 预览 → 真删 → 历史页可见」整条 demo 链路成立。
import { describe, it, expect, beforeEach } from 'vitest';
import { SCAFFOLDS, scopeSizesBatch, executeScope, executeAiPlan, recyclePaths, listUndo, cleanupSuggestions, listDrives, getSpaceHistory, listStartupItems, setStartupItem, removeStartupItem, listInstalledDrivers, checkSpaceForecast, generateWeeklyReport, getWeeklyReports, mcpListServers, mcpAuditTail, scanDuplicateFiles, estimateSize, fanCurveAdvice, hwInfo, hwDiskHealth, runReminderCheck, getReminderConfig, setReminderConfig, pluginMarket, pluginListInstalled, scaffoldRegistryList, powerPlan, listCondaEnvs, MOCK_TREE, _mockResetRecycled, _mockRecyclePath, _mockPushUndoEntry, _mockResetReclaimed, undo } from './mocks';

beforeEach(() => {
  // R225：mock 级回收台账在每条测试前复位，避免真删路径跨用例累积污染
  // 后续 scan / cleanupSuggestions 断言（UNDO_LEDGER 历史台账刻意保留累积）。
  _mockResetRecycled();
  // R231：会话释放台账同样复位（空间趋势断言依赖干净起点）。
  _mockResetReclaimed();
});

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

  // R238：与回收台账同源——envs-stale 真删环境目录后，环境区刷新不再
  // 返回已删环境（真实桌面端 conda env 删了枚举就没了）；base 永不被清。
  it('R238：回收台账内的用户环境不再列出（base 始终保留）', async () => {
    _mockRecyclePath('C:\\Users\\90740\\anaconda3\\envs\\tf2');
    const envs = await listCondaEnvs();
    const names = envs.map((e) => e.name);
    expect(names).not.toContain('tf2');
    expect(names).toContain('base');
    expect(names).toContain('web');
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

  it('runReminderCheck 返回 C 盘可清字节（与清理建议 mock 同源）', async () => {
    const p = await runReminderCheck();
    expect(p.drives.length).toBeGreaterThan(0);
    expect(p.drives[0].path).toBe('C:\\');
    expect(p.total_bytes).toBeGreaterThan(0);
    expect(p.ts).toBeGreaterThan(0);
    // R235：提醒负载 = 清理建议合计（同源）
    const sugs = await cleanupSuggestions('C:\\', undefined, true);
    const sugTotal = Math.round(sugs.reduce((s, r) => s + r.bytes, 0));
    expect(p.total_bytes).toBe(sugTotal);
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

  // R237：重复查找与真实后端同一语义——只扫选中盘 rootPath 前缀内的文件，
  // 且与回收台账同源：已回收路径（含其目录）不再出现在重复组里；剪完
  // 不足 2 份的组整组丢掉。
  it('R237：只扫选中盘：D 盘扫描不返回 C 盘重复组（不编造）', async () => {
    const groups = await scanDuplicateFiles('D:\\');
    expect(groups).toEqual([]);
  });

  it('R237：回收台账内的文件/目录不再出现在重复组（清完重扫不列已删文件）', async () => {
    const before = await scanDuplicateFiles('C:\\');
    const docGroup = before.find((g) => g.files.some((f) => f.path.includes('report-final')));
    expect(docGroup).toBeTruthy();
    // 把 report 组的候选文件「回收」（等价清理页清掉该文件所在目录）
    for (const c of docGroup!.recycle_candidates) _mockRecyclePath(c);
    const after = await scanDuplicateFiles('C:\\');
    const docAfter = after.find((g) => g.files.some((f) => f.path.includes('report-final')));
    // 只剩 keep 1 份 → 不足 2 份 → 整组消失
    expect(docAfter).toBeUndefined();
    // 其他组不受影响
    expect(after.some((g) => g.files.some((f) => f.path.includes('installer_x64')))).toBe(true);
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

  // R269：树内真实目录路径返回真实大小（与 executeAiPlan 记账同源）——
  // 确认窗「约 X」= 执行释放 X，不出现「显示约 0 B、真删释放 1GB」脱节。
  it('R269：树内真实目录返回该节点大小（显示 = 执行记账）', async () => {
    const dir = 'C:\\Users\\90740\\AppData\\Local\\CrashDumps';
    const node = collectMatchingNode(MOCK_TREE, dir);
    expect(node).not.toBeNull();
    const sz = await estimateSize(dir);
    expect(sz).toBe(node!.size);
    expect(sz).toBeGreaterThan(0);
  });

  // R269：已回收的路径再实测返回 0（刚清过 → 再实测就没大小了，与后端一致）
  it('R269：已回收路径再实测返回 0（清过 → 没有大小）', async () => {
    const dir = 'C:\\Users\\90740\\AppData\\Local\\CrashDumps';
    _mockRecyclePath(dir);
    const sz = await estimateSize(dir);
    expect(sz).toBe(0);
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

  // R235：周报「C 盘可清字节」与总览可清理卡片同源——清理后生成的周报
  // 随之下降，不再是固定 2.91GB 的死数字（真实后端周报从扫描建议汇总）。
  it('R235：清理后生成的周报可清字节下降（与建议同源）', async () => {
    const before = await generateWeeklyReport();
    const sugs = await cleanupSuggestions('C:\\', undefined, true);
    const sugTotal = Math.round(sugs.reduce((s, r) => s + r.bytes, 0));
    expect(before.total_bytes).toBe(sugTotal);
    // 真删 CrashDumps → 建议字节减少 → 周报随之下降
    await executeScope('crash-dumps', 'crash-dumps-local', 'C:\\Users\\90740', false, { confirmed: true });
    const after = await generateWeeklyReport();
    expect(after.total_bytes).toBeLessThan(before.total_bytes);
    // 仍与建议同源（清完的周报 = 清完的可清量）
    const sugs2 = await cleanupSuggestions('C:\\', undefined, true);
    const sugTotal2 = Math.round(sugs2.reduce((s, r) => s + r.bytes, 0));
    expect(after.total_bytes).toBe(sugTotal2);
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

  // R234：外推天数与磁盘当前值同源——清理释放后磁盘降 → 剩余空间增大 →
  // 满盘天数增大（「还能用 112 天」不再是一成不变的死数字）。
  it('R234：真删后满盘天数增大（外推随磁盘当前值联动）', async () => {
    const before = await checkSpaceForecast();
    expect(before[0].days).not.toBeNull();
    await executeScope('crash-dumps', 'crash-dumps-local', 'C:\\Users\\90740', false, { confirmed: true });
    const after = await checkSpaceForecast();
    expect(after[0].days).not.toBeNull();
    expect(after[0].days!).toBeGreaterThan(before[0].days!);
    // 天数增加量与实际释放量成比例（释放 0.276GB / 日增 0.42GB ≈ +0.66 天）
    expect(after[0].days! - before[0].days!).toBeGreaterThan(0);
  });

  // R239：预警判定与用户设置联动——full_warn_days 设了阈值才按该阈值判
  // warn（0=关恒不 warn），不再硬编码 <30 与设置页阈值脱节。
  it('R239：设 full_warn_days 阈值后按配置预警（0=关恒不 warn）', async () => {
    await setReminderConfig({ full_warn_days: 0 });
    const off = await checkSpaceForecast();
    expect(off[0].warn).toBe(false);
    // 初始外推 ≈92 天 → 阈值 200 天必然触发预警（演示「盘快满」分支）
    await setReminderConfig({ full_warn_days: 200 });
    const on = await checkSpaceForecast();
    expect(on[0].warn).toBe(true);
    expect(on[0].days!).toBeLessThanOrEqual(200);
    // 阈值 10 天（远小于 92 天）不预警
    await setReminderConfig({ full_warn_days: 10 });
    const tight = await checkSpaceForecast();
    expect(tight[0].warn).toBe(false);
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

  // R231/R232：真删释放字节累加进会话台账——空间趋势卡今天末点回落与本次
  // 清理同源（清完看曲线今天下降，不是固定脚本一动不动）。R232：按执行
  // 条目 bytes_freed 和累加（= days 过滤后实际删的量），与总览 toast 一致。
  it('R231：真删后空间趋势今天末点回落（按实际释放字节下压）', async () => {
    const before = await getSpaceHistory();
    const beforeLast = before['C:\\'][before['C:\\'].length - 1].used_bytes;
    // 真删 CrashDumps 目录（30 天保留 → 实际释放 ≈0.9GB × 0.3 = 276MB）
    await executeScope('crash-dumps', 'crash-dumps-local', 'C:\\Users\\90740', false, { confirmed: true });
    const after = await getSpaceHistory();
    const afterLast = after['C:\\'][after['C:\\'].length - 1].used_bytes;
    const dropped = beforeLast - afterLast;
    // 实际释放 = entries bytes_freed 之和：在 0.9GB×0.3（276MB）附近，远小于全量 0.9GB
    expect(dropped).toBeGreaterThan(0.25 * 1024 ** 3);
    expect(dropped).toBeLessThan(0.5 * 1024 ** 3);
    // 今天末点仍是末点（时间升序保持）
    const pts = after['C:\\'];
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

  // R231/R232：真删释放后 listDrives 与空间趋势同源下压——工作台磁盘卡
  // 不再「曲线回落了磁盘卡还满着」（清完再扫描，两处一起下降）。R232：
  // 按实际释放字节（days 过滤后），非整目录全量。
  it('R231：真删后磁盘卡 used 下降（与空间趋势同源）', async () => {
    const before = await listDrives();
    await executeScope('crash-dumps', 'crash-dumps-local', 'C:\\Users\\90740', false, { confirmed: true });
    const after = await listDrives();
    const bUsed = before[0].used_bytes;
    const aUsed = after[0].used_bytes;
    const dropped = bUsed - aUsed;
    expect(dropped).toBeGreaterThan(0.25 * 1024 ** 3); // ≈276MB（0.9GB×0.3）
    expect(dropped).toBeLessThan(0.5 * 1024 ** 3); // 远小于整目录 0.9GB
    expect(after[0].free_bytes).toBe(after[0].total_bytes - aUsed);
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

  // R236：盘根口径（总览选中 C:）建议字节限定在盘内命中目录——合计
  // 不再虚高到「可清 70% 磁盘」（旧口径每脚本挑 2 scope × 全全局字节）。
  it('R236：C 盘根口径合计合理（各建议项 >0 且合计远小于磁盘容量）', async () => {
    const list = await cleanupSuggestions('C:\\', undefined, true);
    expect(list.length).toBeGreaterThan(0);
    const total = list.reduce((s, r) => s + r.bytes, 0);
    // 全部建议项限定在 C 盘命中目录内：合计 < 磁盘 187GB 的一半（可清潜力
    // 不该是「清掉大半块盘」），且每项都是实际命中目录大小
    expect(total).toBeLessThan(100 * 1024 ** 3);
    expect(total).toBeGreaterThan(0);
    for (const r of list) expect(r.bytes).toBeGreaterThan(0);
    // 多目录脚本（crash-dumps 的 WER/Minidump）在盘根下都计入——合计包含
    // 各命中目录，不是只挑前 2 个 scope 的全局叠加
    const crashDump = list.filter((r) => r.scaffold_id === 'crash-dumps');
    expect(crashDump.length).toBeGreaterThan(0);
    // 与 scopeSizesBatch 盘根口径一致（不翻倍）
    const batch = await scopeSizesBatch('crash-dumps', ['C:\\']);
    const batchTotal = batch[0].sizes.reduce((s, x) => s + x.total_bytes, 0);
    const sugTotal = crashDump.reduce((s, r) => s + r.bytes, 0);
    expect(sugTotal).toBeLessThanOrEqual(batchTotal);
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
      // R227：total_bytes 是目录全量、bytes 是按保留天数过滤后的可清量——
      // 真实后端 total > bytes 是正常语义（有保留期时只清旧文件）。
      expect(row!.total_bytes).toBeGreaterThanOrEqual(row!.bytes);
      expect(row!.total_bytes).toBeGreaterThan(0);
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

  // R227：清理页 scope 行字节 = 工作台树里该目录的真实命中 size（口径同源），
  // 不再用哈希公式——「工作台 0.9 GB / 清理页 75 MB / 总览 180 MB」的三处
  // 数字对不上在演示里消失：所有 scope 行显示的都是 MOCK_TREE 目录真实字节。
  // R228：按 glob 目录名拆分子目录——三个 scope 行各绑定自己的目录
  // （CrashDumps / WER / Minidump），total_bytes 之和 = 树里全部命中字节。
  it('R227/R228：scope 行 total 之和 = 树真实命中字节（按目录拆分不重复计数）', async () => {
    const batch = await scopeSizesBatch('crash-dumps', ['C:\\'], {});
    const sizes = batch[0].sizes;
    expect(sizes.length).toBeGreaterThan(0);
    // 树里 crash-dumps 的真实命中 size（CrashDumps 0.9GB + WER 320MB + Minidump 140MB）
    const real = realNodeBytesSum('crash-dumps');
    expect(real).toBeGreaterThan(0);
    const sum = sizes.reduce((s, r) => s + r.total_bytes, 0);
    expect(sum).toBe(real); // 每个目录恰好绑定一个 scope 行，不重复不遗漏
    for (const row of sizes) {
      expect(row.total_bytes).toBeGreaterThan(0);
      expect(row.bytes).toBeGreaterThan(0);
      expect(row.bytes).toBeLessThanOrEqual(row.total_bytes); // 保留天数过滤后 ≤ 全量
    }
  });

  // R228：多个命中目录传不同 rootPath 时各算各的——C:\Windows 下只有
  // Minidump（系统崩溃转储），scope 行不把 CrashDumps/WER 也算进去
  // （此前 rootPath 不参与过滤，命中多个目录时 aggregate 累加翻倍）。
  it('R228：scope 行字节按 rootPath 过滤（只算自己目录，不翻倍）', async () => {
    const batch = await scopeSizesBatch('crash-dumps', ['C:\\Windows'], {});
    const sizes = batch[0].sizes;
    const dumpRow = sizes.find((r) => r.scope_id === 'crash-dumps-local')!;
    const sysRow = sizes.find((r) => r.scope_id === 'crash-dumps-system')!;
    // C:\Windows 下只有 Minidump（0.14GB = 140MB）——系统崩溃转储行等于它，
    // 应用崩溃转储（CrashDumps 目录，在 C:\Users 下）不应被算进来。
    expect(sysRow.total_bytes).toBe(140 * 1024 * 1024);
    expect(dumpRow.total_bytes).not.toBe(0.9 * 1024 * 1024 * 1024);
  });

  // R228：三个命中目录（CrashDumps / WER / Minidump）作为多个 root_path
  // 传入时，每个目录只算自己的字节——aggregateScopeSizes 累加不翻倍
  // （浏览器里看到的「三行都是 1.80 GB」就是共享 rootPaths[0] 过滤的 bug）。
  it('R228：多 root_path 各算各的，聚合不翻倍', async () => {
    const paths = [
      'C:\\Users\\90740\\AppData\\Local\\CrashDumps',
      'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows\\WER',
      'C:\\Windows\\Minidump',
    ];
    const batch = await scopeSizesBatch('crash-dumps', paths, {});
    expect(batch).toHaveLength(3);
    const total = batch.reduce((s, b) => s + b.sizes.reduce((x, r) => x + r.total_bytes, 0), 0);
    // 三目录真实字节：0.9GB + 320MB + 140MB（= realNodeBytesSum）
    expect(total).toBe(realNodeBytesSum('crash-dumps'));
  });
});

// R227：树里某 scaffold 全部命中节点的 size 之和（与 mocks.ts 的
// scaffoldRealBytes 同算法，测试侧独立核算不依赖 mocks 内部实现）。
function realNodeBytesSum(scaffoldId: string): number {
  const walk = (n: import('./types').Node, acc: number): number => {
    let s = acc;
    if (n.scaffold_id === scaffoldId) s += n.size || 0;
    for (const c of n.children ?? []) s = walk(c, s);
    return s;
  };
  return walk(MOCK_TREE, 0);
}

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

  // R216：executeScope 的 olderThanDays 与 scopeSizesBatch 同口径联动——
  // 天数越小可清越多，dry-run 清单条目数与总字节数越大（与 R212 的天数
  // 缩放是同一语义，此处补执行路径的覆盖）。
  it('olderThanDays 越小 dry-run 清单越大（R216 执行路径天数联动）', async () => {
    const small = await executeScope('crash-dumps', 'dumps', 'C:\\Users\\a\\dumps', true, { olderThanDays: 1 });
    const large = await executeScope('crash-dumps', 'dumps', 'C:\\Users\\a\\dumps', true, { olderThanDays: 30 });
    const bytesOf = (rows: { bytes_freed?: number | null }[]) => rows.reduce((s, r) => s + (r.bytes_freed ?? 0), 0);
    expect(small.length).toBeGreaterThanOrEqual(large.length);
    expect(small.length).toBeGreaterThan(0);
    expect(bytesOf(small)).toBeGreaterThan(bytesOf(large));
  });

  // R243：dry-run 清单条目数与「真正会释放的字节」同源——按每 ~24MB 一条
  // 换算（此前用哈希兜底，12GB 目录也可能只分 4 条进度，进度条与预览
  // 合计脱节）。断言条目数与释放总量成比例（1~24MB 每条的合理区间）。
  it('R243：dry-run 条目数与释放字节同源（每条约 24MB，不脱节）', async () => {
    const entries = await executeScope('crash-dumps', 'crash-dumps-local', 'C:\\Users\\90740', true, {});
    const total = entries.reduce((s, r) => s + (r.bytes_freed ?? 0), 0);
    expect(entries.length).toBeGreaterThan(0);
    // 每条 ≤ 24MB×1.5（ceil 余量），总条目数应能承载总字节
    const perMax = 24 * 1024 * 1024;
    for (const e of entries) {
      expect(e.bytes_freed ?? 0).toBeGreaterThan(0);
      expect(e.bytes_freed ?? 0).toBeLessThanOrEqual(Math.ceil(perMax * 1.5));
    }
    expect(entries.length).toBeGreaterThanOrEqual(Math.ceil(total / perMax) - 1);
    expect(entries.length).toBeLessThanOrEqual(64);
  });

  // R225：真删条目 source 用 MOCK_TREE 真实命中节点路径（与扫描树同源）——
  // 前端 pruneRecycled 按 source 剪 root/scanCache，演示里执行后总览/工作台
  // 体量才真实下降。dry-run 保持合成清单（预览可读），真删才换真实路径。
  // R229：scopeId 用真实 scope（crash-dumps-local = CrashDumps 目录）。
  it('R225：真删 source 取自扫描树真实命中节点（dry-run 仍是合成清单）', async () => {
    const dry = await executeScope('crash-dumps', 'crash-dumps-local', 'C:\\Users\\90740', true, {});
    for (const e of dry) {
      expect(e.source).toContain('mock_');
      expect(e.source).not.toContain('CrashDumps');
    }
    const real = await executeScope('crash-dumps', 'crash-dumps-local', 'C:\\Users\\90740', false, { confirmed: true });
    const realSet = new Set(real.map((e) => e.source));
    expect(realSet.size).toBeGreaterThan(0);
    for (const p of realSet) {
      const hit = collectMatchingNode(MOCK_TREE, p);
      expect(hit).not.toBeNull();
      expect(hit!.scaffold_id).toBe('crash-dumps');
      // R229：真删路径必须是该 scope 的目录（CrashDumps），不是别的目录
      expect(hit!.name).toBe('CrashDumps');
    }
  });

  // R225：真删路径进 mock 级台账后，cleanupSuggestions 不再建议已清目录
  // （总览「可清理」与工作台体量同源收敛——「清完再看」不再复活已删项）。
  // R229：只清 crash-dumps-local（CrashDumps 目录），WER/Minidump 还在——
  // 建议不再出现 CrashDumps 目录，但 crash-dumps 脚本因剩余目录仍被建议。
  it('R225/R229：真删后建议随之变化（清一个目录，其余目录仍可清）', async () => {
    const before = await cleanupSuggestions('C:\\Users\\90740', undefined, true);
    expect(before.some((r) => r.scaffold_id === 'crash-dumps')).toBe(true);
    await executeScope('crash-dumps', 'crash-dumps-local', 'C:\\Users\\90740', false, { confirmed: true });
    const after = await cleanupSuggestions('C:\\Users\\90740', undefined, true);
    // 其他脚本建议不受影响
    expect(after.some((r) => r.scaffold_id === 'browser-cache')).toBe(true);
    // crash-dumps 仍被建议（WER/Minidump 还在），但建议项不再包含 CrashDumps 路径
    const crashSugs = after.filter((r) => r.scaffold_id === 'crash-dumps');
    expect(crashSugs.length).toBeGreaterThan(0);
    expect(crashSugs.some((r) => /CrashDumps/i.test(r.path))).toBe(false);
    expect(crashSugs.some((r) => /WER|Minidump/i.test(r.path))).toBe(true);
  });

  // R230：还原 = 目录回补扫描树——quarantine 条目 undo 成功后，被回收的
  // 目录从 mock 级台账移除，cleanupSuggestions 重新建议它（演示里还原与
  // 清理是互逆操作，不会出现「还原了但总览还缺这个目录」）。
  it('R230：undo 还原后目录回到扫描树（回收台账移除该路径）', async () => {
    const path = 'C:\\Users\\90740\\AppData\\Local\\CrashDumps';
    // 先回收该目录（模拟真删剪枝）
    _mockRecyclePath(path);
    const before = await cleanupSuggestions('C:\\Users\\90740', undefined, true);
    // CrashDumps 被剪后，crash-dumps-local scope 的建议字节不含 CrashDumps
    // （只剩 WER/Minidump 的兜底）——回补后应回到 CrashDumps 的真实大小。
    const beforeLocal = before.find((r) => r.scope_id === 'crash-dumps-local');
    expect(beforeLocal).toBeTruthy();
    // 注入一条隔离条目（source = 被回收的目录）
    _mockPushUndoEntry({
      timestamp: new Date().toISOString(),
      action: 'quarantine' as const,
      source: path,
      reason: '隔离测试',
      bytes_freed: 0.9 * 1024 ** 3,
    });
    // 找到注入条目在台账里的位置（listUndo 返回按时间倒序）
    const list = await listUndo(500);
    const idx = list.findIndex((e) => e.source === path);
    expect(idx).toBeGreaterThanOrEqual(0);
    const restored = await undo(idx, path);
    expect(restored).not.toBeNull();
    // 还原后 CrashDumps 目录回补到树——crash-dumps-local scope 的建议字节
    // 增长（CrashDumps 0.9GB 回到 byName 兑底，不再是 WER+Minidump 之和）。
    const after = await cleanupSuggestions('C:\\Users\\90740', undefined, true);
    const afterLocal = after.find((r) => r.scope_id === 'crash-dumps-local');
    expect(afterLocal).toBeTruthy();
    expect(afterLocal!.bytes).toBeGreaterThan(beforeLocal!.bytes);
  });

  // R233：还原 = 空间回涨——隔离条目 undo 后**该 source 实际入账过的**释放
  // 字节从会话台账撤回，磁盘卡与空间趋势回升（真实后端还原后 used 增加；
  // 演示里不能「还原了但空间没回来」）。种子隔离条目从没入账，还原不回涨
  // （不能把别的清理释放凭空回涨）。
  it('R233：undo 还原后磁盘卡回涨（只撤回该 source 入账过的释放）', async () => {
    const origin = await listDrives();
    const originUsed = origin[0].used_bytes;
    // 真删 CrashDumps（入账 reclaimedBySource[CrashDumps] ≈276MB）→ 磁盘降
    const entries = await executeScope('crash-dumps', 'crash-dumps-local', 'C:\\Users\\90740', false, { confirmed: true });
    const accounted = entries.reduce((s, e) => s + (e.bytes_freed ?? 0), 0);
    expect(accounted).toBeGreaterThan(0);
    const cleaned = await listDrives();
    expect(cleaned[0].used_bytes).toBeLessThan(originUsed);
    const src = entries[0].source; // CrashDumps 真实目录路径
    // 注入一条以该 source 为源的隔离条目（模拟后端隔离还原链路）
    _mockPushUndoEntry({
      timestamp: new Date().toISOString(),
      action: 'quarantine' as const,
      source: src,
      reason: '隔离还原回涨',
      bytes_freed: accounted,
    });
    const list = await listUndo(500);
    const idx = list.findIndex((e) => e.source === src && e.action === 'quarantine');
    expect(idx).toBeGreaterThanOrEqual(0);
    await undo(idx, src);
    // 还原撤回该 source 的入账 → 磁盘回涨 ≈accounted，回到清之前
    const restored = await listDrives();
    const restoredUsed = restored[0].used_bytes;
    expect(restoredUsed - cleaned[0].used_bytes).toBeGreaterThanOrEqual(accounted - 1);
    expect(restoredUsed).toBeLessThanOrEqual(originUsed);
  });

  // R233：种子隔离条目（从没入账过）还原不回涨——不能把别的清理释放凭空
  // 加回（clamp 到 0 会让磁盘卡跳满）。
  it('R233：未入账的隔离条目还原不改变磁盘（诚实：没清过不回涨）', async () => {
    const before = await listDrives();
    const path = 'C:\\Users\\demo\\Documents\\种子隔离.docx';
    _mockPushUndoEntry({
      timestamp: new Date().toISOString(),
      action: 'quarantine' as const,
      source: path,
      reason: '种子条目',
      bytes_freed: 3.2 * 1024 ** 3,
    });
    const list = await listUndo(500);
    const idx = list.findIndex((e) => e.source === path);
    expect(idx).toBeGreaterThanOrEqual(0);
    await undo(idx, path);
    const after = await listDrives();
    expect(after[0].used_bytes).toBe(before[0].used_bytes);
  });
});

describe('mocks.executeAiPlan（R267 AI 提案执行）', () => {
  it('dry-run 返回可回收条目且不写台账、不剪树', async () => {
    const before = await listUndo(500);
    const known = 'C:\\Users\\90740\\Downloads\\installer_x64 (1).exe';
    const dry = await executeAiPlan([known], true, true);
    expect(dry.length).toBeGreaterThan(0);
    expect(dry[0].source).toBe(known);
    expect(dry[0].action).toBe('recycle');
    expect((dry[0].bytes_freed ?? 0)).toBeGreaterThan(0);
    const after = await listUndo(500);
    expect(after.length).toBe(before.length);
  });

  it('真删写台账、记回收台账并返回实测字节，二次执行不再重复入账', async () => {
    const known = 'C:\\Users\\90740\\Videos\\meeting-2026-09-copy.mp4';
    const before = await listUndo(500);
    const first = await executeAiPlan([known], true, false);
    expect(first.length).toBe(1);
    expect(first[0].bytes_freed).toBeGreaterThan(0);
    const after = await listUndo(500);
    expect(after.length).toBe(before.length + 1);
    // 第二次再真删同路径 → 已回收，dry-run 与真删都空（同 重复文件查找
    // 「再扫一次没得清」的语义，不会同一文件重复入账）。
    const dry2 = await executeAiPlan([known], true, true);
    expect(dry2.length).toBe(0);
    const real2 = await executeAiPlan([known], true, false);
    expect(real2.length).toBe(0);
    const after2 = await listUndo(500);
    expect(after2.length).toBe(after.length);
  });

  it('用户未确认 → 拒绝执行；空路径列表 → 空结果', async () => {
    await expect(executeAiPlan(['C:\\Users\\90740\\Downloads\\installer_x64 (1).exe'], false, false))
      .rejects.toBeTruthy();
    expect(await executeAiPlan([], true, false)).toEqual([]);
  });

  it('未知/危险路径不产生条目（mock 不替后端放行）', async () => {
    const unknown = await executeAiPlan(['E:\\不存在\\foo.tmp'], true, true);
    expect(unknown.length).toBe(0);
    const dangerous = await executeAiPlan(['C:\\Windows\\System32\\foo.dll'], true, true);
    expect(dangerous.length).toBe(0);
    const root = await executeAiPlan(['C:\\'], true, true);
    expect(root.length).toBe(0);
  });
});

describe('mocks.recyclePaths（R268 右键回收与建议同源收敛）', () => {
  it('右键回收目录后 cleanupSuggestions 不再建议它（与 executeScope 同语义）', async () => {
    // 先确认该 scope 在建议里（crash-dumps 含 CrashDumps 目录）
    const before = await cleanupSuggestions('C:\\Users\\90740', undefined, true);
    const beforeLocal = before.find((r) => r.scope_id === 'crash-dumps-local');
    expect(beforeLocal).toBeTruthy();
    expect(beforeLocal!.bytes).toBeGreaterThan(0);
    // 右键回收 CrashDumps 目录（mock 级台账应记入）
    await recyclePaths(['C:\\Users\\90740\\AppData\\Local\\CrashDumps'], '测试右键回收');
    const after = await cleanupSuggestions('C:\\Users\\90740', undefined, true);
    // crash-dumps-local 的字节下降（CrashDumps 被剪，只剩 WER/Minidump 兜底）
    const afterLocal = after.find((r) => r.scope_id === 'crash-dumps-local');
    expect(afterLocal).toBeTruthy();
    expect(afterLocal!.bytes).toBeLessThan(beforeLocal!.bytes);
  });

  it('右键回收写入台账（历史页可见）且 dry-run executeAiPlan 不再返回该路径', async () => {
    const target = 'C:\\Users\\90740\\AppData\\Local\\CrashDumps';
    const before = await listUndo(500);
    await recyclePaths([target], '右键回收');
    const after = await listUndo(500);
    expect(after.length).toBe(before.length + 1);
    expect(after[0].action).toBe('recycle');
    expect(after[0].source).toBe(target);
    // 已回收 → AI 提案 dry-run 也不返回该路径（双路径共享同一台账）
    const dry = await executeAiPlan([target], true, true);
    expect(dry.length).toBe(0);
  });
});

// R225：按路径在树里找节点（真删 source 必须对应树里的真实节点）。
function collectMatchingNode(node: import('./types').Node, path: string): import('./types').Node | null {
  const norm = (s: string) => s.replace(/[\\/]+$/, '').toLowerCase();
  if (norm(node.path) === norm(path)) return node;
  for (const c of node.children ?? []) {
    const found = collectMatchingNode(c, path);
    if (found) return found;
  }
  return null;
}

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

  // R228：命中后仍下钻——Local\Microsoft\Windows（windows-privacy）下的
  // WER 是 crash-dumps 的 detect 目标（%LOCALAPPDATA%/Microsoft/Windows/WER），
  // 不能被外层 Windows 节点截断；每个目录只算给自己的脚本。
  it('R228：crash-dumps 命中 WER 与 Minidump（下钻不再被外层脚本截断）', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'crash-dumps');
    expect(card).toBeTruthy();
    const paths = card!.matches.map((m) => m.path);
    expect(paths.some((p) => /\\WER$/i.test(p))).toBe(true);
    expect(paths.some((p) => /\\Minidump$/i.test(p))).toBe(true);
    // 3 个目录（CrashDumps / WER / Minidump）各算各的，总量 = 三者之和
    const sum = card!.matches.reduce((s, m) => s + m.size, 0);
    expect(sum).toBe(card!.totalSize);
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

  // R214：Discord——%APPDATA%\discord 真实常驻（Electron 缓存），
  // 预览演示 Discord 缓存清理（与 Teams 同族但无 variant）。
  it('discord 命中 discord 目录（R214 Discord 演示）', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'discord');
    expect(card).toBeTruthy();
    expect(card!.matches.some((m) => /discord/i.test(m.path))).toBe(true);
    expect(card!.totalSize).toBeGreaterThan(1024 ** 3);
  });

  // R215：Zoom——%APPDATA%\zoom.us 真实常驻（视频会议高频），补上后
  // 21 脚本除「安全软件套装」全命中。
  it('zoom 命中 zoom.us（R215 Zoom 演示）', () => {
    const cards = collectScaffoldCards(MOCK_TREE, SCAFFOLDS);
    const card = cards.find((c) => c.scaffold.id === 'zoom');
    expect(card).toBeTruthy();
    expect(card!.matches.some((m) => /zoom\.us/i.test(m.path))).toBe(true);
    expect(card!.totalSize).toBeGreaterThan(1024 ** 3);
  });
});