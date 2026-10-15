// 预览模式（非 Tauri）清理演示数据的自洽性测试：
// scopeSizesBatch 必须按 scaffold 的 scopes 返回尺寸（不是空表），
// executeScope 的 dry-run 只出清单、真删才追加台账——保证浏览器里
// 「勾选 → 预览 → 真删 → 历史页可见」整条 demo 链路成立。
import { describe, it, expect } from 'vitest';
import { SCAFFOLDS, scopeSizesBatch, executeScope, listUndo, cleanupSuggestions, listDrives, getSpaceHistory, listStartupItems, listInstalledDrivers, checkSpaceForecast, generateWeeklyReport, getWeeklyReports } from './mocks';

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