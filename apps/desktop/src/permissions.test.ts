import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import {
  PERMS,
  defaultPermConfig,
  loadPermConfig,
  savePermConfig,
  resetPermConfig,
  isPermEnabled,
  grantSessionAuth,
  hasSessionAuth,
  clearSessionAuths,
} from './permissions';

// 权限表是四级权限模型的唯一真源：isPermEnabled 的短路（L0 恒开 / L3 恒关）与
// loadPermConfig 的逐键合并都依赖「PERMS 的 id 集 == DEFAULT_ENABLED 的键集」。
// 加一条权限只改一张表、漏改另一张不会编译报错，只会静默按 undefined 处理——
// 这里把两张表的对偶关系与读盘时的布尔转换都钉住。

function stubStorage(map: Map<string, string>): void {
  vi.stubGlobal('window', {
    localStorage: {
      getItem: (k: string) => map.get(k) ?? null,
      setItem: (k: string, v: string) => { map.set(k, v); },
      removeItem: (k: string) => { map.delete(k); },
    },
  });
}

const STORAGE_KEY = 'diskpilot.perms.v1';

afterEach(() => vi.unstubAllGlobals());
beforeEach(() => clearSessionAuths());

describe('权限表一致性（R422）', () => {
  it('PERMS 与 DEFAULT_ENABLED 一一对应，无遗漏、无多余、无重复', () => {
    const ids = PERMS.map((p) => p.id);
    const defaults = Object.keys(defaultPermConfig().enabled);
    expect(new Set(ids)).toEqual(new Set(defaults));
    expect(ids.length).toBe(defaults.length);
    expect(new Set(ids).size).toBe(ids.length);
  });
  it('每项文案键都按 perm.<id>.label / perm.<id>.desc 生成', () => {
    for (const p of PERMS) {
      expect(p.labelKey).toBe('perm.' + p.id + '.label');
      expect(p.descKey).toBe('perm.' + p.id + '.desc');
    }
  });
  it('分级口径：L0 三项只读、L3 四项永久禁止（门没有锁孔）', () => {
    const byLevel = (lv: string) => PERMS.filter((p) => p.level === lv).map((p) => p.id).sort();
    expect(byLevel('L0')).toEqual(['hw.disk_health', 'hw.read', 'hw.report']);
    expect(byLevel('L3')).toEqual(['bios.flash', 'disk.format', 'hw.overclock', 'system.files']);
    expect(byLevel('L1').length).toBeGreaterThan(0);
    expect(byLevel('L2').length).toBeGreaterThan(0);
  });
});

describe('isPermEnabled 短路与默认（R422）', () => {
  it('L0 恒开：配置里显式写 false 也开', () => {
    const cfg = defaultPermConfig();
    cfg.enabled['hw.read'] = false;
    expect(isPermEnabled('hw.read', cfg)).toBe(true);
    expect(isPermEnabled('hw.report', cfg)).toBe(true);
  });
  it('L3 恒关：配置里显式写 true 也关', () => {
    const cfg = defaultPermConfig();
    cfg.enabled['disk.format'] = true;
    cfg.enabled['bios.flash'] = true;
    expect(isPermEnabled('disk.format', cfg)).toBe(false);
    expect(isPermEnabled('bios.flash', cfg)).toBe(false);
  });
  it('L1/L2 按默认：L1 开、L2 关，配置可逐项覆盖', () => {
    const cfg = defaultPermConfig();
    expect(isPermEnabled('hw.stress', cfg)).toBe(true);
    expect(isPermEnabled('cli.run', cfg)).toBe(true);
    expect(isPermEnabled('cleanup.execute', cfg)).toBe(false);
    expect(isPermEnabled('fan.adjust', cfg)).toBe(false);
    cfg.enabled['cleanup.execute'] = true;
    expect(isPermEnabled('cleanup.execute', cfg)).toBe(true);
    cfg.enabled['hw.stress'] = false;
    expect(isPermEnabled('hw.stress', cfg)).toBe(false);
  });
});

describe('loadPermConfig 读盘卫生（R422）', () => {
  it('无存值 / 坏 JSON 都回落默认，不抛', () => {
    stubStorage(new Map());
    expect(loadPermConfig()).toEqual(defaultPermConfig());
    stubStorage(new Map([[STORAGE_KEY, '{not json']]));
    expect(loadPermConfig()).toEqual(defaultPermConfig());
  });
  it('只合并已知键：未知键与异构形状被丢弃', () => {
    stubStorage(new Map([[STORAGE_KEY, JSON.stringify({ enabled: { 'hw.stress': false, 'nope.id': true }, extra: 1 })]]));
    const cfg = loadPermConfig();
    expect(cfg.enabled['hw.stress']).toBe(false);
    expect(Object.keys(cfg.enabled)).toEqual(Object.keys(defaultPermConfig().enabled));
    expect((cfg.enabled as Record<string, boolean>)['nope.id']).toBeUndefined();
  });
  it('只认字面布尔 true："false" 字符串与 0/1 一律按关', () => {
    stubStorage(new Map([[STORAGE_KEY, JSON.stringify({ enabled: { 'hw.stress': 'false', 'hw.export': 1, 'cli.run': 0, 'hw.export2': 'true' } })]]));
    const cfg = loadPermConfig();
    expect(cfg.enabled['hw.stress']).toBe(false);
    expect(cfg.enabled['hw.export']).toBe(false);
    expect(cfg.enabled['cli.run']).toBe(false);
  });
  it('L3 键即使被存成 true，isPermEnabled 仍拒绝', () => {
    stubStorage(new Map([[STORAGE_KEY, JSON.stringify({ enabled: { 'disk.format': true } })]]));
    expect(isPermEnabled('disk.format')).toBe(false);
  });
});

describe('存盘与会话授权（R422）', () => {
  it('savePermConfig 往返，resetPermConfig 写回默认', () => {
    const map = new Map<string, string>();
    stubStorage(map);
    const cfg = defaultPermConfig();
    cfg.enabled['fan.adjust'] = true;
    savePermConfig(cfg);
    expect(loadPermConfig().enabled['fan.adjust']).toBe(true);
    const reset = resetPermConfig();
    expect(reset).toEqual(defaultPermConfig());
    expect(loadPermConfig()).toEqual(defaultPermConfig());
  });
  it('写盘失败静默（配额 / 隐私模式）', () => {
    vi.stubGlobal('window', { localStorage: { getItem: () => null, setItem: () => { throw new Error('quota'); }, removeItem: () => {} } });
    expect(() => savePermConfig(defaultPermConfig())).not.toThrow();
    expect(() => resetPermConfig()).not.toThrow();
  });
  it('会话授权：授予即命中，清空后不再命中', () => {
    expect(hasSessionAuth('hw.stress')).toBe(false);
    grantSessionAuth('hw.stress');
    expect(hasSessionAuth('hw.stress')).toBe(true);
    clearSessionAuths();
    expect(hasSessionAuth('hw.stress')).toBe(false);
  });
});
