import { describe, it, expect, afterAll, vi } from 'vitest';
import { formatPluralUnit, htmlLangOf, setLang, t } from './core';

describe('英文单复数（R350）', () => {
  it('formatPluralUnit 只在 1 时用单数', () => {
    expect(formatPluralUnit(1, 'day', 'days')).toBe('1 day');
    expect(formatPluralUnit(0, 'day', 'days')).toBe('0 days');
    expect(formatPluralUnit(3, 'day', 'days')).toBe('3 days');
    expect(formatPluralUnit(1.5, 'day', 'days')).toBe('1.5 days');
  });

  it('单/复数文案留空时只给数字', () => {
    expect(formatPluralUnit(1, '', '')).toBe('1');
    expect(formatPluralUnit(2, '', '')).toBe('2');
  });

  it('t() 认 {n:one|other} 语法，数字与单位一起出', () => {
    setLang('en');
    expect(t('overview.checkup.scanAgeStale', { n: 1, days: 22 })).toBe(
      '1 drive not scanned for over 22 days',
    );
    expect(t('overview.checkup.scanAgeStale', { n: 3, days: 22 })).toBe(
      '3 drives not scanned for over 22 days',
    );
  });

  it('t() 里的复数参数接受数字字符串', () => {
    setLang('en');
    expect(t('overview.clean.peekFiles', { n: '1' })).toBe('1 file');
    expect(t('overview.clean.peekFiles', { n: '2' })).toBe('2 files');
  });

  it('参数缺失时保留占位符原样（开发期一眼看见漏传）', () => {
    setLang('en');
    expect(t('overview.clean.peekFiles', {})).toBe('{n:file|files}');
  });

  // R471：调用方常把已本地化的数字（「1,234」）直接传进来，Number() 得到 NaN。
  // 旧行为是把 {n:file|files} 原样漏到界面上；现在按复数渲染，界面上永远是数字 + 名词。
  it('参数不是数字时按复数渲染（不漏出原始语法）', () => {
    setLang('en');
    expect(t('overview.clean.peekFiles', { n: 'many' })).toBe('many files');
    expect(t('overview.clean.peekFiles', { n: '1,234' })).toBe('1,234 files');
    expect(t('cleanup.done', { n: '1,234', size: '10 GB' })).toBe(
      'Cleaned 1,234 files · about 10 GB (estimate) · moved to the system Recycle Bin',
    );
  });

  it('htmlLangOf 把语言映射成 html lang 值', () => {
    expect(htmlLangOf('zh')).toBe('zh-CN');
    expect(htmlLangOf('en')).toBe('en');
  });

  it('切语言时顺手改 <html lang>（读屏软件靠它选发音）', () => {
    const el = { lang: 'zh-CN' };
    vi.stubGlobal('document', { documentElement: el });
    try {
      setLang('en');
      expect(el.lang).toBe('en');
      setLang('zh');
      expect(el.lang).toBe('zh-CN');
    } finally {
      vi.unstubAllGlobals();
      setLang('zh');
    }
  });

  afterAll(() => {
    setLang('zh');
  });
});