import { describe, expect, it } from 'vitest';
import { sortByPivot } from './SteamInspector';
import type { SteamGame } from '../types';

// Steam 库排序透视（R97）：四条 pivot 各管一种「先看谁」的心智模型：
// sleep（休眠分=大小×未玩月数，幽灵置顶）/ size / library / lastplayed。
// 纯逻辑抽出来单测，避免排序回归无人管。

const game = (over: Partial<SteamGame>): SteamGame => ({
  appid: 0,
  name_en: 'x',
  name_cn: null,
  install_dir_name: 'x',
  install_path: 'C:\\Steam\\steamapps\\common\\x',
  appmanifest_path: 'C:\\Steam\\steamapps\\appmanifest_0.acf',
  size_bytes: 1_000_000_000,
  last_played_ts: null,
  last_updated_ts: null,
  bytes_to_download: 0,
  bytes_downloaded: 0,
  library_root: 'C:\\Steam',
  state_flags: 4,
  is_fully_installed: true,
  is_ghost: false,
  default_recommended: false,
  recommendation_reason: null,
  workshop_item_count: 0,
  ...over,
});

describe('sortByPivot sleep（休眠分）', () => {
  it('大体积 × 久未玩排前面（休眠分高优先清）', () => {
    const now = Date.now() / 1000;
    const bigStale = game({ appid: 1, size_bytes: 100_000_000_000, last_played_ts: now - 400 * 86400 });
    const smallFresh = game({ appid: 2, size_bytes: 1_000_000_000, last_played_ts: now - 3 * 86400 });
    const sorted = sortByPivot([smallFresh, bigStale], 'sleep');
    expect(sorted.map((g) => g.appid)).toEqual([1, 2]);
  });

  it('未玩过按 12 个月计（不设 0 免分）', () => {
    const never = game({ appid: 1, size_bytes: 10_000_000_000, last_played_ts: null });
    const playedLong = game({ appid: 2, size_bytes: 10_000_000_000, last_played_ts: Date.now() / 1000 - 365 * 86400 });
    const sorted = sortByPivot([playedLong, never], 'sleep');
    // 未玩=12 个月 vs 已玩=12 个月 → 前者分数略高（12 > 11.99...），1 在前
    expect(sorted[0].appid).toBe(1);
  });

  it('幽灵永远置顶（无论多大）', () => {
    const ghost = game({ appid: 1, is_ghost: true, size_bytes: 0 });
    const huge = game({ appid: 2, size_bytes: 500_000_000_000, last_played_ts: null });
    const sorted = sortByPivot([huge, ghost], 'sleep');
    expect(sorted.map((g) => g.appid)).toEqual([1, 2]);
  });

  it('不改原数组', () => {
    const a = game({ appid: 1, size_bytes: 10_000_000_000 });
    const b = game({ appid: 2, size_bytes: 1_000_000_000 });
    const src = [a, b];
    sortByPivot(src, 'sleep');
    expect(src.map((g) => g.appid)).toEqual([1, 2]);
  });
});

describe('sortByPivot size / library / lastplayed', () => {
  it('size：大体积在前', () => {
    const big = game({ appid: 1, size_bytes: 200_000_000_000 });
    const small = game({ appid: 2, size_bytes: 5_000_000_000 });
    expect(sortByPivot([small, big], 'size').map((g) => g.appid)).toEqual([1, 2]);
  });

  it('library：先按库路径，再按体积', () => {
    const libB = game({ appid: 1, library_root: 'D:\\Steam', size_bytes: 9_000_000_000 });
    const libA1 = game({ appid: 2, library_root: 'C:\\Steam', size_bytes: 1_000_000_000 });
    const libA2 = game({ appid: 3, library_root: 'C:\\Steam', size_bytes: 5_000_000_000 });
    const sorted = sortByPivot([libB, libA2, libA1], 'library');
    // C:\Steam 两块在前（按体积 5G 再 1G），D:\ 最后
    expect(sorted.map((g) => g.appid)).toEqual([3, 2, 1]);
  });

  it('lastplayed：最近游玩在前，未玩过沉底', () => {
    const now = Date.now() / 1000;
    const fresh = game({ appid: 1, last_played_ts: now - 1000 });
    const old = game({ appid: 2, last_played_ts: now - 200 * 86400 });
    const never = game({ appid: 3, last_played_ts: null });
    const sorted = sortByPivot([never, old, fresh], 'lastplayed');
    expect(sorted.map((g) => g.appid)).toEqual([1, 2, 3]);
  });
});