import { describe, expect, it } from 'vitest';

/**
 * R382 回归守卫：总览的三条异步加载都必须带「迟到的旧响应作废」逻辑。
 * 快速切盘 / 扫描完成重拉时，先发的请求可能后返回——没有守卫就会把旧盘的
 * 建议清理项、跨次环比、dry-run 预览写进当前盘（同 key 跨盘串数据）。
 *
 * 组件本身依赖 React 运行时，这里读源码做静态断言：删掉守卫就红。
 */
const RAW = import.meta.glob('./AssetOverview.tsx', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const SRC = Object.values(RAW)[0] ?? '';

function sliceFn(name: string): string {
  const start = SRC.indexOf('const ' + name + ' = ');
  if (start < 0) return '';
  const end = SRC.indexOf('\n  };', start);
  return SRC.slice(start, end < 0 ? undefined : end);
}

describe('跨盘异步加载的迟到响应守卫（R382）', () => {
  it('建议清理项 / 跨次环比各自持有请求序号，且写入前比对', () => {
    const clean = sliceFn('loadClean');
    expect(clean).toContain('cleanReq.current');
    expect(clean).toMatch(/req !== cleanReq\.current/);
    expect(clean).toMatch(/req === cleanReq\.current/);
    const diff = sliceFn('loadSuggDiff');
    expect(diff).toContain('suggReq.current');
    expect(diff).toMatch(/req !== suggReq\.current/);
  });

  it('dry-run 预览写入前核对当前盘镜像，切盘后不落旧结果', () => {
    const prev = sliceFn('togglePreview');
    expect(prev).toContain('reqPath');
    expect(prev).toMatch(/selDriveRef\.current !== reqPath/);
  });
});
