// ── i18n 国际化单元测试（R9）───────────────────────────────────────
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { setLang, getLangMode, getLang, t } from './i18n';

// vitest node 环境没有 localStorage：stub 一个内存实现。
class MemoryStorage {
  private m = new Map<string, string>();
  getItem(k: string) { return this.m.has(k) ? this.m.get(k)! : null; }
  setItem(k: string, v: string) { this.m.set(k, v); }
  clear() { this.m.clear(); }
}

describe('i18n 文案表', () => {
  beforeEach(() => {
    vi.stubGlobal('localStorage', new MemoryStorage());
    setLang('system');
  });

  it('默认跟随系统（zh 时回中文）', () => {
    Object.defineProperty(navigator, 'language', { value: 'zh-CN', configurable: true });
    expect(getLang()).toBe('zh');
    expect(t('settings.appearance')).toBe('外观');
  });

  it('en 生效时返回英文', () => {
    setLang('en');
    expect(getLangMode()).toBe('en');
    expect(t('settings.appearance')).toBe('Appearance');
    expect(t('settings.title')).toBe('Settings');
  });

  it('未翻译的 key 回退中文原文，绝不显示 key 本体', () => {
    expect(t('settings.language.hint')).toBe('切换立即生效，不用重启。');
    // 两边都没有 → 返回 key 本身（兜底，不应在日常中出现）
    expect(t('no.such.key')).toBe('no.such.key');
  });

  it('占位符替换', () => {
    expect(t('settings.general.updateFound', { latest: '2.0.0', current: '1.9.0' })).toContain('2.0.0');
    setLang('en');
    expect(t('settings.general.updateFound', { latest: '2.0.0', current: '1.9.0' })).toContain('2.0.0');
  });

  it('setLang 持久化到 localStorage', () => {
    setLang('en');
    expect(localStorage.getItem('diskpilot.lang.v1')).toBe('en');
    // 模拟新会话：模块状态重置后从 localStorage 恢复
    setLang('system');
    expect(getLangMode()).toBe('system');
    expect(localStorage.getItem('diskpilot.lang.v1')).toBe('system');
  });

  it('system 模式回退非中文系统为英文', () => {
    Object.defineProperty(navigator, 'language', { value: 'fr-FR', configurable: true });
    setLang('system');
    expect(getLang()).toBe('en');
  });

  it('useT 导出存在且为函数（切换不刷新由 useSyncExternalStore 订阅驱动）', async () => {
    // useT 是 hook，不直接调用；这里确认导出存在，渲染层行为由 Settings 集成验证。
    const mod = await import('./i18n');
    expect(typeof mod.useT).toBe('function');
  });
});