// 预览模式（非 Tauri）清理演示数据的自洽性测试：
// scopeSizesBatch 必须按 scaffold 的 scopes 返回尺寸（不是空表），
// executeScope 的 dry-run 只出清单、真删才追加台账——保证浏览器里
// 「勾选 → 预览 → 真删 → 历史页可见」整条 demo 链路成立。
import { describe, it, expect } from 'vitest';
import { SCAFFOLDS, scopeSizesBatch, executeScope, listUndo, cleanupSuggestions, listDrives } from './mocks';

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