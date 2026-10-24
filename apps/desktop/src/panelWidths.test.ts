import { describe, it, expect } from 'vitest';
import { AI_WIDTH_DEFAULT, AI_WIDTH_MIN, clampAiWidth } from './panelWidths';

// R408：AI 侧栏宽度夹取——记住的宽值在窄窗上不许挤掉中栏保底。
describe('clampAiWidth（AI 侧栏宽度夹取）', () => {
  it('非法值回默认 300', () => {
    expect(clampAiWidth(Number.NaN, 1920, 209)).toBe(AI_WIDTH_DEFAULT);
    expect(clampAiWidth(Infinity, 1920, 209)).toBe(AI_WIDTH_DEFAULT);
    expect(clampAiWidth(-Infinity, 1920, 209)).toBe(AI_WIDTH_DEFAULT);
  });
  it('区间内的值原样返回', () => {
    expect(clampAiWidth(800, 1920, 209)).toBe(800);
    expect(clampAiWidth(300, 1000, 209)).toBe(300);
  });
  it('窄窗按上界夹取（窗口 - 侧栏占位 - 480）', () => {
    // 1000 - 209 - 480 = 311
    expect(clampAiWidth(800, 1000, 209)).toBe(311);
    // 700 - 57 - 480 = 163 < 260 → 保底 AI_WIDTH_MIN
    expect(clampAiWidth(800, 700, 57)).toBe(AI_WIDTH_MIN);
  });
  it('低于下界夹到 AI_WIDTH_MIN', () => {
    expect(clampAiWidth(100, 1920, 209)).toBe(AI_WIDTH_MIN);
    expect(clampAiWidth(0, 1920, 209)).toBe(AI_WIDTH_MIN);
    expect(clampAiWidth(-50, 1920, 209)).toBe(AI_WIDTH_MIN);
  });
});

const RAW = import.meta.glob('./**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

describe('R408 守卫：AI 侧栏宽度初始化与兜底必须走 clampAiWidth', () => {
  const app = RAW['./App.tsx'] ?? '';
  it('App.tsx 存在', () => {
    expect(app.length).toBeGreaterThan(100);
  });
  const region = (src: string, startMarker: string, endMarker: string): string => {
    const i = src.indexOf(startMarker);
    if (i < 0) return '';
    const j = src.indexOf(endMarker, i + startMarker.length);
    return j < 0 ? src.slice(i) : src.slice(i, j);
  };
  const initRegion = region(app, 'const [aiWidth, setAiWidth] = useState<number>(() =>', '  );');
  const fitRegion = region(app, 'const fit = () => {', 'window.addEventListener(');
  it('aiWidth 初始化走 clampAiWidth（不是裸 isFinite 判定）', () => {
    expect(initRegion.length).toBeGreaterThan(80);
    expect(initRegion).toContain('clampAiWidth(');
    expect(initRegion).toContain('sidebarFootprintAt(');
    expect(initRegion).not.toContain('Number.isFinite(v)');
  });
  it('fit 兜底在左/右收到底后会收 AI 侧栏', () => {
    expect(fitRegion.length).toBeGreaterThan(150);
    expect(fitRegion).toContain('setAiWidth((w) => clampAiWidth(w, winW, sidebarFootprint))');
  });
  it('nav.ts 导出 sidebarFootprintAt 供初始化算占位', () => {
    expect((RAW['./nav.ts'] ?? '')).toContain('export function sidebarFootprintAt');
  });
});
