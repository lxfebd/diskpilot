import { describe, expect, it } from 'vitest';
import { entriesScaffold } from './CleanupPage';

describe('entriesScaffold（R241：台账 source → 脚本卡归属）', () => {
  const cards = [
    {
      scaffold: { id: 'crash-dumps' },
      matches: [{ path: 'C:\\Users\\90740\\AppData\\Local\\CrashDumps' }],
    },
    {
      scaffold: { id: 'wechat-cache' },
      matches: [{ path: 'C:\\Users\\90740\\Documents\\WeChat Files' }],
    },
  ];

  it('目录本身命中该卡', () => {
    expect(entriesScaffold('C:\\Users\\90740\\AppData\\Local\\CrashDumps', cards)).toBe('crash-dumps');
  });

  it('目录下的文件命中该卡', () => {
    expect(
      entriesScaffold('C:\\Users\\90740\\AppData\\Local\\CrashDumps\\dmp123.dmp', cards),
    ).toBe('crash-dumps');
  });

  it('子目录下的文件命中该卡（前缀含目录分隔符，不误配同名前缀）', () => {
    expect(
      entriesScaffold('C:\\Users\\90740\\AppData\\Local\\CrashDumps\\sub\\x.dmp', cards),
    ).toBe('crash-dumps');
  });

  it('目录前缀相似但不在命中目录下 → null（不误配 CrashDumpsX）', () => {
    expect(
      entriesScaffold('C:\\Users\\90740\\AppData\\Local\\CrashDumpsX\\x.dmp', cards),
    ).toBeNull();
  });

  it('未命中任何卡 → null', () => {
    expect(entriesScaffold('Z:\\elsewhere\\file.bin', cards)).toBeNull();
  });

  it('大小写/尾斜杠归一（Windows 路径不区分大小写）', () => {
    expect(entriesScaffold('c:\\users\\90740\\appdata\\local\\crashdumps\\', cards)).toBe('crash-dumps');
  });
});
