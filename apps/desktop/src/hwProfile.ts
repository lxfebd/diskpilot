// 本机性能档位检测：决定「本地 Ollama 是否吃得消 / 该建议云端还是本地」。
//
// 本地大模型很吃性能——弱机上跑 7B/14B 模型会把 CPU/内存吃满、整机卡顿。
// 这里读浏览器与后端能拿到的硬件信息（CPU 核心数、内存），算出一个
// 「弱 / 中 / 强」档位，供 AI 配置页给出明确建议，并注入 AI 上下文让它
// 知道自己跑在多强的机器上、该怎么节制。
//
// 文案口径：label / tierLabel 是给用户看的，走文案表**在调用点求值**；
// perfContextLine 是发给模型的 prompt 文本，按约定保持中文不译。
import { t } from './i18n';

export type PerfTier = 'weak' | 'medium' | 'strong';

export interface HwProfile {
  /** 档位：弱(≤4核 且 ≤8GB) / 中(≤8核 且 ≤16GB) / 强(其余) */
  tier: PerfTier;
  /** 逻辑 CPU 核心数；浏览器拿不到时为 null */
  cores: number | null;
  /** 内存（GB）；浏览器拿不到时为 null */
  memGb: number | null;
  /** 是否可信：浏览器近似值 vs 后端真实探针 */
  fromBackend: boolean;
  /** 一句话人话描述，给设置页展示（getter：每次读取按当前语言求值，切语言即时生效） */
  label: string;
}

// 浏览器只能给近似值（hardwareConcurrency / deviceMemory 是 UA 报告的估算）。
// 后端 runSystemProbe 走 Win32 拿真实值，但需要 Tauri 环境。优先用后端真实值。
let cached: HwProfile | null = null;

export async function getHwProfile(force = false): Promise<HwProfile> {
  if (cached && !force) return cached;
  cached = await detect();
  return cached;
}

async function detect(): Promise<HwProfile> {
  // 后端真实探针（Tauri 桌面端）：CPU 逻辑核心 + 真实内存。
  // 复用 hwCache 的共享探针（Win32 真实值 + 5s 缓存），不重复采样。
  let backend: { cores: number; memGb: number } | null = null;
  try {
    const { isTauri } = await import('./env');
    if (isTauri) {
      const { ensureSysProbe } = await import('./hwCache');
      const p = await ensureSysProbe(false);
      backend = { cores: p.cpu_cores, memGb: p.mem_total_bytes / (1024 ** 3) };
    }
  } catch { /* 预览/非 Tauri 环境走浏览器近似值 */ }

  const cores = backend?.cores ?? navigator.hardwareConcurrency ?? null;
  const memGb = backend?.memGb ?? (navigator as { deviceMemory?: number }).deviceMemory ?? null;

  let tier: PerfTier;
  if ((cores !== null && cores <= 4) || (memGb !== null && memGb <= 8)) tier = 'weak';
  else if ((cores !== null && cores <= 8) || (memGb !== null && memGb <= 16)) tier = 'medium';
  else tier = 'strong';

  // label 用 getter 而非拼好的字符串：结果会缓存进 `cached`，若在求值时拼死，
  // 切语言后设置页仍看到旧语言的档位描述。
  return {
    tier,
    cores,
    memGb,
    fromBackend: backend !== null,
    get label() {
      return t('hw.perfLabel', {
        cores: cores ?? '?',
        mem: memGb != null ? memGb.toFixed(0) : '?',
        tier: tierLabel(tier),
      });
    },
  };
}

export function tierLabel(tier: PerfTier): string {
  return tier === 'weak' ? t('hw.tierWeak') : tier === 'medium' ? t('hw.tierMedium') : t('hw.tierStrong');
}

/**
 * 该不该建议本地 Ollama？
 * 弱机本地大模型会明显卡顿 → 明确建议云端 API；
 * 中机可用小参数模型；强机本地无压力。
 */
export function recommendLocal(tier: PerfTier): 'suggest-cloud' | 'allow-local' | 'recommend-local' {
  if (tier === 'weak') return 'suggest-cloud';
  if (tier === 'medium') return 'allow-local';
  return 'recommend-local';
}

/** 供 AI 上下文注入的一句话：让模型知道自己跑在多强的机器上、该怎么节制。
 *  这里是发给模型的 prompt 文本，按 i18n 约定保持中文不译（文案表只服务界面）。 */
export async function perfContextLine(): Promise<string> {
  const p = await getHwProfile();
  const spec = `CPU ${p.cores ?? '?'} 核 · 内存 ${p.memGb != null ? p.memGb.toFixed(0) : '?'} GB`; // @i18n-keep 发给模型的上下文片段，恒中文
  switch (p.tier) {
    case 'weak':
      return `本机为低配（${spec}）。工具执行必须快速：不要跑烤机/压测/全盘深度扫描这类长时间高负载操作；优先轻量、快、少打扰。`; // @i18n-keep prompt 不译
    case 'medium':
      return `本机为中配（${spec}）。可以跑中等负载工具，但注意节制，避免长时间压满 CPU/内存。`; // @i18n-keep prompt 不译
    case 'strong':
      return `本机为高配（${spec}）。性能充裕，可以放心跑烤机/压测/深度分析等重工具。`; // @i18n-keep prompt 不译
  }
}