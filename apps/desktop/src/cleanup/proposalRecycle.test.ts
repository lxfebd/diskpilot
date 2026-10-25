// R448：AI 清单「回收结果」的窗内反馈——纯函数 + 组件接线守卫。
// 缺口：doRecycle 此前不看返回值、无论成败都 close()；权限未开或整批失败时窗口照关，
// 用户只在一闪而过的 toast 里看到原因，回到总览查不到「刚才那批到底成没成」。
import { describe, expect, it } from 'vitest';
import { recycleOutcome } from './model';

const RAW = import.meta.glob('../components/CleanupProposalDialog.tsx', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;
const src = Object.values(RAW)[0] ?? '';

describe('回收结果的窗内反馈（R448 recycleOutcome）', () => {
  it('有成功项就关窗，部分失败交给 store 的 toast', () => {
    expect(recycleOutcome({ done: 3, failed: 2 })).toEqual({ close: true, done: 3, failed: 2 });
    expect(recycleOutcome({ done: 1, failed: 0 })).toEqual({ close: true, done: 1, failed: 0 });
  });

  it('一项都没成时不关窗，并把失败条数带出来', () => {
    expect(recycleOutcome({ done: 0, failed: 4 })).toEqual({ close: false, done: 0, failed: 4 });
  });

  it('空批次与缺失返回不把窗钉住', () => {
    expect(recycleOutcome({ done: 0, failed: 0 })).toEqual({ close: true, done: 0, failed: 0 });
    expect(recycleOutcome(null)).toEqual({ close: true, done: 0, failed: 0 });
    expect(recycleOutcome(undefined)).toEqual({ close: true, done: 0, failed: 0 });
  });

  it('负数、小数与非数值一律归一成非负整数', () => {
    expect(recycleOutcome({ done: -2, failed: 1.7 })).toEqual({ close: false, done: 0, failed: 1 });
    expect(recycleOutcome({ done: Number.NaN, failed: Number.POSITIVE_INFINITY })).toEqual({
      close: true,
      done: 0,
      failed: 0,
    });
  });
});

describe('清单窗的回收结果接线（R448）', () => {
  const doRecycle = (() => {
    const i = src.indexOf('const doRecycle = async () => {');
    const j = src.indexOf('// 按风险分组', i);
    return i >= 0 && j > i ? src.slice(i, j) : '';
  })();

  it('doRecycle 先拿到返回值再决定关窗', () => {
    expect(doRecycle).toContain('const outcome = recycleOutcome(res);');
    expect(doRecycle).toContain('if (outcome.close) close();');
    expect(doRecycle).toContain("else setRecycleErr(t('cleanup.recycleAllFailed', { n: outcome.failed }));");
    expect(doRecycle).not.toContain('setBusy(false);' + String.fromCharCode(10) + '    close();');
  });

  it('busy 一定在 finally 里收——异常也不会把执行按钮锁死', () => {
    const fin = doRecycle.indexOf('} finally {');
    expect(fin).toBeGreaterThan(-1);
    expect(doRecycle.indexOf('setBusy(false);')).toBeGreaterThan(fin);
  });

  it('窗内给错误位（role=alert），proposal 变化时清掉', () => {
    expect(src).toContain('{recycleErr && (');
    expect(src).toContain('role="alert"');
    expect(src).toContain('setRecycleErr(null);');
  });
});
