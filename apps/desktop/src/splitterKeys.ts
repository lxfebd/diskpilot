export const SPLITTER_STEP = 12;
export const SPLITTER_STEP_LARGE = 48;

export interface SplitterKeyInput {
  key: string;
  shiftKey?: boolean;
}

/** 键盘调整分隔条：返回水平像素增量（正 = 向右），不属于左右方向键的键返回 null。 */
export function splitterDelta(e: SplitterKeyInput): number | null {
  const step = e.shiftKey === true ? SPLITTER_STEP_LARGE : SPLITTER_STEP;
  if (e.key === 'ArrowLeft') return -step;
  if (e.key === 'ArrowRight') return step;
  return null;
}
