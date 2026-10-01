// 低配机器降载参数（README「低配机器优化专项」）：弱机上把高频/高开销的
// 前端行为调轻，避免扫描/工具墙/甜甜圈把整机拖卡。档位识别复用 hwProfile.ts
// （4 核/8GB 为弱机），getHwProfile 内部有缓存，多组件共享不重复采样。

import { useEffect, useState } from 'react';
import { getHwProfile, type PerfTier } from './hwProfile';

export interface PerfScale {
  /** weak=1（降载） / medium=0.5 / strong=0：0 表示不用降载。 */
  scale: number;
  /** 轮询间隔倍率：弱机拉长（5s→10s、1s→2s），强机保持。 */
  pollFactor: number;
  /** 虚拟滚动 overscan：弱机裁小离屏渲染量。 */
  overscan: number;
}

/** 降载档位 → 具体参数。weak 全部降载；medium 微降；strong 不动。 */
export function perfScaleFor(tier: PerfTier): PerfScale {
  if (tier === 'weak') return { scale: 1, pollFactor: 2, overscan: 6 };
  if (tier === 'medium') return { scale: 0.5, pollFactor: 1.5, overscan: 10 };
  return { scale: 0, pollFactor: 1, overscan: 14 };
}

/** 同步默认（强机参数，避免首帧闪烁）；异步探测到档位后更新。 */
const DEFAULT_SCALE: PerfScale = { scale: 0, pollFactor: 1, overscan: 14 };

let cachedScale: PerfScale | null = null;

/** 读取当前档位的降载参数（同步返回缓存；无缓存时异步探测并更新）。 */
export function usePerfScale(): PerfScale {
  const [scale, setScale] = useState<PerfScale>(cachedScale ?? DEFAULT_SCALE);
  useEffect(() => {
    let alive = true;
    getHwProfile()
      .then((p) => {
        const s = perfScaleFor(p.tier);
        cachedScale = s;
        if (alive) setScale(s);
      })
      .catch(() => {});
    return () => { alive = false; };
  }, []);
  return scale;
}

/** 弱机上轮询类接口的间隔（毫秒）：base 是强机间隔，weak 翻倍。 */
export function pollIntervalMs(scale: PerfScale, baseMs: number): number {
  return Math.round(baseMs * scale.pollFactor);
}
