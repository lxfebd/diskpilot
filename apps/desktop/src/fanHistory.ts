// R173：风扇建议的历史对比——用户反复看建议时想知道「这次和上次比变了没」
// （档位抬升 = 负载/温度在涨；回落 = 变凉快了）。只存「上一次」一个快照，
// 不做时间序列（风扇建议是随手看一眼的，不需要趋势图）。纯函数 + localStorage。

export const FAN_SNAP_KEY = 'system.fanLastSnapshot';

export interface FanSnapshot {
  /** 建议档位标识（quiet / balanced / balanced_plus / high / urgent）。 */
  level: string;
  /** 档位展示名（后端给的 level_label，可能随语言变，仅作展示兜底）。 */
  levelLabel: string;
  /** 本次读到的最高温（℃）；读不到为 null。 */
  tempMaxC: number | null;
  /** 记录时刻（ms）。 */
  atMs: number;
}

/** 解析存储的风扇快照：形状非法（缺 level / 非对象 / 坏 JSON）→ null。 */
export function parseFanSnapshot(raw: string | null): FanSnapshot | null {
  if (!raw) return null;
  let obj: unknown;
  try {
    obj = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof obj !== 'object' || obj === null) return null;
  const o = obj as Record<string, unknown>;
  const level = typeof o.level === 'string' ? o.level : '';
  if (!level) return null;
  const t = typeof o.tempMaxC === 'number' && Number.isFinite(o.tempMaxC) ? o.tempMaxC : null;
  const at = typeof o.atMs === 'number' && Number.isFinite(o.atMs) ? o.atMs : 0;
  return {
    level,
    levelLabel: typeof o.levelLabel === 'string' ? o.levelLabel : '',
    tempMaxC: t,
    atMs: at,
  };
}

/** 序列化快照（parse 的逆；字段名与 parse 一致）。 */
export function formatFanSnapshot(s: FanSnapshot): string {
  return JSON.stringify({
    level: s.level,
    levelLabel: s.levelLabel,
    tempMaxC: s.tempMaxC,
    atMs: s.atMs,
  });
}

export interface FanDelta {
  /** 档位是否变了（变了才值得提示）。 */
  levelChanged: boolean;
  prevLevel: string;
  prevLevelLabel: string;
  /** 温度差（本次 − 上次，℃）；任一方读不到 → null（不编造）。 */
  tempDeltaC: number | null;
}

/** 与上次快照对比：无非空历史 → null（不比较）。档位相同且温度读不到/无变化时
 *  调用方按「与上次一致」处理。纯函数。 */
export function fanAdviceDelta(
  cur: { level: string; levelLabel: string; tempMaxC: number | null },
  prev: FanSnapshot | null,
): FanDelta | null {
  if (!prev) return null;
  if (!cur.level) return null;
  const tempDeltaC =
    cur.tempMaxC != null && prev.tempMaxC != null ? cur.tempMaxC - prev.tempMaxC : null;
  return {
    levelChanged: cur.level !== prev.level,
    prevLevel: prev.level,
    prevLevelLabel: prev.levelLabel || prev.level,
    tempDeltaC,
  };
}