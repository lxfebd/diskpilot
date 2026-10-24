import { describe, it, expect, afterAll } from 'vitest';
import { formatPluralUnit, setLang, t } from './core';

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

  it('参数缺失或不是数字时保留占位符原样（不显示 NaN）', () => {
    setLang('en');
    expect(t('overview.clean.peekFiles', {})).toBe('{n:file|files}');
    expect(t('overview.clean.peekFiles', { n: 'many' })).toBe('{n:file|files}');
  });

  afterAll(() => {
    setLang('zh');
  });
});