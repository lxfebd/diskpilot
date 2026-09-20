import { describe, expect, it } from 'vitest';
import { formatBytes, formatBytesTriple, formatCount, ellipsizePath } from './format';

describe('formatBytes', () => {
  it('bytes below 1 KiB stay in B', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1023)).toBe('1023 B');
  });

  it('negative values format with unit too (regression: n<1024 swallowed negatives)', () => {
    expect(formatBytes(-1023)).toBe('-1023 B');
    expect(formatBytes(-2048)).toBe('-2.00 KB');
    expect(formatBytes(-38989467648)).toBe('-36.3 GB');
    expect(formatBytes(-15 * 1024 ** 2)).toBe('-15.0 MB');
  });

  it('picks the largest unit with >=1 value', () => {
    expect(formatBytes(2048)).toBe('2.00 KB');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.00 MB');
    expect(formatBytes(3 * 1024 ** 3)).toBe('3.00 GB');
    expect(formatBytes(2.5 * 1024 ** 4)).toBe('2.50 TB');
  });

  it('reduces precision as the value grows', () => {
    expect(formatBytes(1.2 * 1024 ** 2)).toBe('1.20 MB');
    expect(formatBytes(15 * 1024 ** 2)).toBe('15.0 MB');
    expect(formatBytes(150 * 1024 ** 2)).toBe('150 MB');
  });

  it('never overflows past PB', () => {
    expect(formatBytes(1024 ** 6)).toBe('1024 PB');
  });
});

describe('formatBytesTriple', () => {
  it('below 1 KiB keeps raw B values', () => {
    expect(formatBytesTriple(500, 300, 200)).toEqual(['500 B', '300 B', '200 B']);
  });

  it('total display equals used+free after rounding', () => {
    const [total, used, free] = formatBytesTriple(10 * 1024 ** 3, 9.95 * 1024 ** 3, 0.05 * 1024 ** 3);
    expect(total).toBe('10.0 GB');
    expect(used).toBe('9.9 GB');
    expect(free).toBe('0.1 GB');
    const num = (s: string) => Number(s.replace(/ [A-Z]+$/, ''));
    expect(num(total)).toBe(num(used) + num(free));
  });

  it('uses one decimal for values >= 10', () => {
    const [total, used, free] = formatBytesTriple(1.5 * 1024 ** 4, 1.0 * 1024 ** 4, 0.5 * 1024 ** 4);
    expect(total).toBe('1.50 TB');
    expect(used).toBe('1.00 TB');
    expect(free).toBe('0.50 TB');
  });
});

describe('formatCount', () => {
  it('plain numbers below 1000', () => {
    expect(formatCount(0)).toBe('0');
    expect(formatCount(999)).toBe('999');
  });

  it('k / M suffixes', () => {
    expect(formatCount(1_500)).toBe('1.5k');
    expect(formatCount(999_999)).toBe('1000.0k');
    expect(formatCount(2_500_000)).toBe('2.5M');
  });
});

describe('ellipsizePath', () => {
  it('short paths pass through unchanged', () => {
    expect(ellipsizePath('C:\\Windows')).toBe('C:\\Windows');
  });

  it('long paths fold the middle, keep head and tail', () => {
    const long = 'C:\\Users\\31672\\AppData\\Local\\Microsoft\\Edge\\User Data\\Default\\Cache\\Cache_Data\\f_000001';
    const out = ellipsizePath(long, 48);
    expect(out.length).toBeLessThanOrEqual(48);
    expect(out.startsWith('C:\\Users\\31')).toBe(true);
    expect(out).toContain('…');
    expect(out.endsWith('f_000001')).toBe(true);
  });

  it('at exactly maxLen no fold', () => {
    const p = 'a'.repeat(48);
    expect(ellipsizePath(p, 48)).toBe(p);
  });
});
