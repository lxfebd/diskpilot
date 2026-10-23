import { describe, expect, it } from 'vitest';
import { formatBytes, formatBytesTriple, formatCount, ellipsizePath, extOf, driveRootOf, formatDateKey, formatDateTime, formatTime, formatMonthDay } from './format';

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

describe('extOf', () => {
  it('extracts lowercase extension without dot', () => {
    expect(extOf('photo.JPG')).toBe('jpg');
    expect(extOf('archive.tar.gz')).toBe('gz');
    expect(extOf('README.md')).toBe('md');
  });

  it('returns empty string when no extension', () => {
    expect(extOf('README')).toBe('');
    expect(extOf('folder.')).toBe('');
    expect(extOf('.gitignore')).toBe(''); // 点开头视为无扩展名（lastIndexOf 在 0）
    expect(extOf('.')).toBe('');
  });
});

// R248：盘根提取——空间大头 Top 10 行点击定位到工作台时，从子目录路径
// 还原盘根去扫描，扫完再 selectPath 回子目录。
describe('driveRootOf（路径 → 盘根）', () => {
  it('子目录取第一个分隔符后的盘根', () => {
    expect(driveRootOf('C:\\Users\\AppData')).toBe('C:\\');
    expect(driveRootOf('C:\\Users\\90740\\AppData\\Local')).toBe('C:\\');
  });

  it('盘根本身不变', () => {
    expect(driveRootOf('C:\\')).toBe('C:\\');
  });

  it('UNC 路径取服务器根（近似）', () => {
    expect(driveRootOf('\\\\server\\share\\dir')).toBe('\\\\server\\');
  });

  it('无分隔符路径原样返回', () => {
    expect(driveRootOf('plainpath')).toBe('plainpath');
  });
});

// R324：报表/审计等要落成文本的时间显示——此前 Steam 报表写死 toLocaleString('zh-CN')，
// 英文界面也按中文地区格式渲染。收口成地区中立的 `YYYY-MM-DD HH:MM`。
describe('formatDateTime（地区中立的完整时间）', () => {
  it('日期 + 时分，补零', () => {
    expect(formatDateTime(new Date(2026, 0, 5, 9, 7))).toBe('2026-01-05 09:07');
    expect(formatDateTime(new Date(2026, 11, 31, 23, 59, 8))).toBe('2026-12-31 23:59');
  });

  it('withSeconds 时带上秒', () => {
    expect(formatDateTime(new Date(2026, 9, 24, 15, 4, 3), true)).toBe('2026-10-24 15:04:03');
  });

  it('无效日期返回空串，不打印 NaN', () => {
    expect(formatDateTime(new Date('nope'))).toBe('');
  });

  it('前半个日期与 formatDateKey 同口径', () => {
    const d = new Date(2026, 9, 24, 15, 4);
    expect(formatDateTime(d).split(' ')[0]).toBe(formatDateKey(d));
  });
});

// 守卫：日期格式化不许写死地区（否则界面语言切换后时间格式不跟着变）。
describe('日期格式化不许写死地区（R324）', () => {
  const RAW = import.meta.glob('./**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

  it('源码里不出现 toLocale*(\'xx-XX\') 这类硬编码地区', () => {
    const bad: string[] = [];
    for (const [rel, src] of Object.entries(RAW)) {
      if (rel.includes('.test.')) continue;
      src.split('\n').forEach((line, i) => {
        if (/toLocale\w*\(\s*['"]/.test(line)) bad.push(`${rel}:${i + 1}: ${line.trim()}`);
      });
    }
    expect(bad).toEqual([]);
  });
});

// R329：此前 9 处时间显示走 toLocaleTimeString/toLocaleDateString，输出跟随系统地区
// （英文界面在中文系统上渲染成中文时段），与 formatDateTime 的「应用语言决定格式」口径不一致。
describe('formatTime（地区中立的时分）', () => {
  it('补零到 HH:MM', () => {
    expect(formatTime(new Date(2026, 0, 5, 9, 7))).toBe('09:07');
    expect(formatTime(new Date(2026, 11, 31, 23, 59, 8))).toBe('23:59');
  });

  it('withSeconds 时带秒', () => {
    expect(formatTime(new Date(2026, 9, 24, 15, 4, 3), true)).toBe('15:04:03');
  });

  it('无效日期返回空串', () => {
    expect(formatTime(new Date('nope'))).toBe('');
  });
});

describe('formatMonthDay（地区中立的月-日）', () => {
  it('补零到 MM-DD', () => {
    expect(formatMonthDay(new Date(2026, 0, 5))).toBe('01-05');
    expect(formatMonthDay(new Date(2026, 11, 31))).toBe('12-31');
  });

  it('无效日期返回空串', () => {
    expect(formatMonthDay(new Date('nope'))).toBe('');
  });
});

// 守卫：日期/时间一律走 format.ts，禁止再出现跟随系统地区的 toLocale 时间 API。
// 数字的 toLocaleString（千分位）仍允许——那是数量级展示，不是日期。
describe('日期时间不许跟随系统地区（R329）', () => {
  it('源码里不出现 toLocaleTimeString / toLocaleDateString', () => {
    const files = import.meta.glob('./**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
    const bad: string[] = [];
    for (const [rel, text] of Object.entries(files)) {
      if (rel.includes('.test.')) continue;
      if (rel.endsWith('/format.ts') || rel.endsWith('/mocks.ts')) continue;
      text.split('\n').forEach((line, i) => {
        if (/\.toLocale(Time|Date)String\(/.test(line)) bad.push(`${rel}:${i + 1}: ${line.trim()}`);
      });
    }
    expect(bad).toEqual([]);
  });
});
