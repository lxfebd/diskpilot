import { describe, expect, it } from 'vitest';
import { createRefreshGate } from './refreshGate';

const RAW_TSX = import.meta.glob('./components/history/HistoryPage.tsx', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

describe('刷新闸门（R455 createRefreshGate）', () => {
  it('空闲时可以直接开始，收尾没有补跑', () => {
    const gate = createRefreshGate();
    expect(gate.tryStart()).toBe(true);
    expect(gate.finish()).toBe(false);
  });

  it('在途时拦下第二次刷新并记一次补跑', () => {
    const gate = createRefreshGate();
    expect(gate.tryStart()).toBe(true);
    expect(gate.tryStart()).toBe(false);
    expect(gate.finish()).toBe(true);
  });

  it('补跑标记用过即清，补跑那次收尾不再要求补跑', () => {
    const gate = createRefreshGate();
    expect(gate.tryStart()).toBe(true);
    expect(gate.tryStart()).toBe(false);
    expect(gate.finish()).toBe(true);
    // 补跑：重新占位，收尾时没有新的被拦下的刷新
    expect(gate.tryStart()).toBe(true);
    expect(gate.finish()).toBe(false);
  });

  it('收尾后可以重新开始', () => {
    const gate = createRefreshGate();
    expect(gate.tryStart()).toBe(true);
    expect(gate.finish()).toBe(false);
    expect(gate.tryStart()).toBe(true);
  });

  it('多个实例互不影响', () => {
    const a = createRefreshGate();
    const b = createRefreshGate();
    expect(a.tryStart()).toBe(true);
    expect(b.tryStart()).toBe(true);
    expect(a.finish()).toBe(false);
    expect(b.finish()).toBe(false);
  });

  it('HistoryPage 接上闸门：上限在途放宽时收尾补拉一次（接线）', () => {
    const page = RAW_TSX['./components/history/HistoryPage.tsx'] ?? '';
    expect(page).toContain("import { createRefreshGate } from '../../refreshGate';");
    expect(page).toContain('const gateRef = useRef(createRefreshGate());');
    expect(page).toContain('if (!gateRef.current.tryStart()) return;');
    expect(page).toContain('if (!gateRef.current.finish()) {');
    expect(page).toContain('refreshRef.current?.();');
    expect(page).toContain('refreshRef.current = refresh;');
    expect(page).not.toContain('refreshingRef');
  });
});
