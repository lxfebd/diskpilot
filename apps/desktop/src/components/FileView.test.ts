import { describe, it, expect } from 'vitest';
import { matchesQuery } from './FileView';

describe('FileView 搜索匹配（R93：文件名优先 + 路径片段兜底）', () => {
  it('空查询匹配一切', () => {
    expect(matchesQuery('a.mp4', '')).toBe(true);
    expect(matchesQuery('x', '')).toBe(true);
  });

  it('文件名包含命中（大小写不敏感）', () => {
    expect(matchesQuery('Installer.exe', 'install')).toBe(true);
    expect(matchesQuery('report.pdf', 'PORT')).toBe(true);
  });

  it('文件名不中时匹配相对路径片段', () => {
    expect(matchesQuery('installer.exe', 'downloads', 'downloads/installer.exe')).toBe(true);
    expect(matchesQuery('readme.md', 'project', 'src/project/readme.md')).toBe(true);
  });

  it('文件名与路径都不中 → false', () => {
    expect(matchesQuery('a.mp4', 'video', 'photos/a.mp4')).toBe(false);
  });

  it('通配符只对文件名生效：*.mp4 不因路径里的 .mp4 误命中', () => {
    expect(matchesQuery('clip.mp4', '*.mp4', 'videos/clip.mp4')).toBe(true);
    expect(matchesQuery('clip', '*.mp4', 'videos/clip.mp4')).toBe(false);
    expect(matchesQuery('instal?er', 'instal?er', 'downloads/installer.exe')).toBe(true);
  });
});
