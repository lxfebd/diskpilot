// 硬件/系统信息的前端共享缓存。
//
// 硬件静态信息（CPU/内存/主板/BIOS/显卡/硬盘健康）读一次后基本不变，
// 每次切页都重新跑 PowerShell 既慢又闪「不可读」。这里做模块级缓存：
// 首次读取后永久复用（直到显式 refresh），多组件并发请求合并为一次后端调用。
// info 与 health 独立成功：磁盘健康因权限/缺 CrystalDiskInfo 失败时只标 error，
// 不拖垮已读到的硬件信息。
// 实时指标（CPU 占用、内存用量、开机时长）不属于本缓存，由调用方自行刷新。

import { api } from './api';
import type { HwInfo, HwDiskHealth } from './api';

export interface HwCacheData {
  info: HwInfo | null;
  health: HwDiskHealth | null;
  /** 最近一次成功读取时刻（毫秒时间戳）；null = 尚未读取成功 */
  at: number | null;
  /** 是否正在读取中（首载或手动刷新） */
  loading: boolean;
  /** 最近一次读取失败信息；null = 无错误 */
  error: string | null;
}

let state: HwCacheData = { info: null, health: null, at: null, loading: false, error: null };
const listeners = new Set<() => void>();
let inflight: Promise<void> | null = null;

function emit() {
  for (const l of listeners) l();
}

function setState(patch: Partial<HwCacheData>) {
  state = { ...state, ...patch };
  emit();
}

/** 订阅缓存变化，返回退订函数。 */
export function subscribeHwCache(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function getHwCache(): HwCacheData {
  return state;
}

/**
 * 确保硬件信息已加载：有缓存立即返回；无缓存则发起读取。
 * 并发调用共享同一次后端请求。force=true 时强制重新读取。
 */
export async function ensureHwLoaded(force = false): Promise<HwCacheData> {
  // info 读取成功即视为已加载（health 可能因权限/缺 CrystalDiskInfo 而失败为
  // null，不再拖垮整条硬件信息链路）；force=true 时全部重读。
  if (!force && state.info) return state;
  if (inflight) return inflight.then(() => state);
  inflight = (async () => {
    setState({ loading: true, error: null });
    try {
      const info = await api.hwInfo(force);
      let health: HwDiskHealth | null = null;
      let error: string | null = null;
      try {
        health = await api.hwDiskHealth(force);
      } catch (e) {
        error = String(e instanceof Error ? e.message : e);
      }
      setState({ info, health, at: Date.now(), loading: false, error });
    } catch (e) {
      setState({ loading: false, error: String(e instanceof Error ? e.message : e) });
      throw e;
    } finally {
      inflight = null;
    }
  })();
  try {
    await inflight;
  } finally {
    inflight = null;
  }
  return state;
}

/** 只读系统信息探针（CPU 占用/内存/开机时长）的共享缓存：5 秒内复用。 */
interface SysProbeCache {
  at: number;
  value: import('./types').SystemProbe;
}
let sysProbeCache: SysProbeCache | null = null;
const SYS_PROBE_TTL = 5000;

export async function ensureSysProbe(sample = false): Promise<import('./types').SystemProbe> {
  const now = Date.now();
  if (!sample && sysProbeCache && now - sysProbeCache.at < SYS_PROBE_TTL) {
    return sysProbeCache.value;
  }
  const p = await api.runSystemProbe(sample);
  if (!sample) sysProbeCache = { at: now, value: p };
  return p;
}
