// ── 轻量 SVG 图表（无第三方图表库）：传感器趋势折线 + 稳定性压测温度折线 ──
import { useT } from '../../i18n';
import { TEMP_RAMP } from '../../colors';

// ── G15 传感器趋势折线图（轻量 SVG，无第三方图表库）──
export interface TrendPoint {
  ts: number;
  cpu_temp_c?: number | null;
  mem_percent?: number | null;
}

/// 解析 sensor_trend format=json 的结构化输出 → 温度点序列。
/// 与 parseStressCurve（文本行）互补：这个是压测期内高频轮询的实时通道。
/// 宽松解析：非法/字段缺失的点跳过；不足 2 点返回 null（调用方不画线）。
/// 导出以便单测锁定与后端 rep_report.rs 的 JSON 契约（ts + cpu_temp_c 可空）。
export function parseTrendJson(text: string): TrendPoint[] | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== 'object') return null;
  const arr = (parsed as { points?: unknown }).points;
  if (!Array.isArray(arr)) return null;
  const points: TrendPoint[] = [];
  for (const item of arr) {
    if (!item || typeof item !== 'object') continue;
    const ts = (item as { ts?: unknown }).ts;
    if (typeof ts !== 'number') continue;
    const cpu = (item as { cpu_temp_c?: unknown }).cpu_temp_c;
    const mem = (item as { mem_percent?: unknown }).mem_percent;
    const p: TrendPoint = { ts };
    if (typeof cpu === 'number') p.cpu_temp_c = cpu;
    if (typeof mem === 'number') p.mem_percent = mem;
    points.push(p);
  }
  return points.length >= 2 ? points : null;
}

export function TrendChart({ points }: { points: TrendPoint[] }) {
  const t = useT();
  const W = 720;
  const H = 200;
  const PAD = 26;

  const cpuVals = points.map((p) => p.cpu_temp_c).filter((v): v is number => typeof v === 'number');
  const memVals = points.map((p) => p.mem_percent).filter((v): v is number => typeof v === 'number');
  const all = [...cpuVals, ...memVals];
  if (all.length === 0) return <p className="muted small">{t('toolbelt.trend.noData')}</p>;

  const lo = Math.min(...all);
  const hi = Math.max(...all);
  const span = hi - lo || 1;
  const xAt = (i: number, n: number) => PAD + (i / Math.max(n - 1, 1)) * (W - PAD * 2);

  const line = (vals: number[], color: string) => {
    if (vals.length < 2) return null;
    const pts = vals.map((v, i) => `${xAt(i, vals.length).toFixed(1)},${(H - PAD - ((v - lo) / span) * (H - PAD * 2)).toFixed(1)}`).join(' ');
    return <polyline key={color} points={pts} fill="none" stroke={color} strokeWidth={1.8} strokeLinejoin="round" strokeLinecap="round" />;
  };

  // 时间轴刻度：取首/中/尾 ts
  const ticks = points.length >= 3
    ? [points[0], points[Math.floor(points.length / 2)], points[points.length - 1]]
    : points;

  return (
    <div className="bench-trend">
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={t('toolbelt.trend.aria')} className="bench-trend-svg">
        {[0.25, 0.5, 0.75].map((f) => (
          <line key={f} x1={PAD} x2={W - PAD} y1={PAD + (H - PAD * 2) * f} y2={PAD + (H - PAD * 2) * f} stroke="var(--line)" strokeDasharray="4 4" strokeWidth={1} />
        ))}
        {ticks.map((p, i) => (
          <text key={i} x={xAt(i, ticks.length)} y={H - 8} textAnchor="middle" className="bench-trend-ticks">
            {new Date(p.ts * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
          </text>
        ))}
        <text x={PAD} y={12} fill="#888" fontSize={10}>max {hi.toFixed(0)}</text>
        <text x={PAD} y={H - PAD + 12} fill="#888" fontSize={10}>min {lo.toFixed(0)}</text>
        {line(cpuVals, 'var(--accent-strong)')}
        {line(memVals, 'var(--warn)')}
      </svg>
      <div className="bench-trend-legend">
        {cpuVals.length > 0 && <span><i style={{ background: 'var(--accent-strong)' }} /> {t('toolbelt.trend.legendCpu')}</span>}
        {memVals.length > 0 && <span><i style={{ background: 'var(--warn)' }} /> {t('toolbelt.trend.legendMem')}</span>}
        <span className="muted small">{t('toolbelt.trend.samples', { n: points.length })}</span>
      </div>
    </div>
  );
}

// ── 稳定性压测实时温度折线（SST 复刻）：stress_test 输出解析出的每秒 CPU 温度样本 ──
// 轻量 SVG，无第三方图表库；X=压测秒数，Y=温度℃，辅助线展示 压测前/峰值/熔断阈值。
interface StressCurvePoint {
  sec: number;
  temp: number;
}

export function StressTempChart({ points }: { points: StressCurvePoint[] }) {
  const t = useT();
  const W = 720;
  const H = 160;
  const PAD = 30;

  const vals = points.map((p) => p.temp);
  if (vals.length < 2) return null;
  const lo = Math.min(...vals, 0);
  const hi = Math.max(...vals, 100);
  const span = hi - lo || 10;
  const xAt = (i: number) => PAD + (i / Math.max(points.length - 1, 1)) * (W - PAD * 2);
  const yAt = (v: number) => H - PAD - ((v - lo) / span) * (H - PAD * 2);
  const pts = points.map((p, i) => `${xAt(i).toFixed(1)},${yAt(p.temp).toFixed(1)}`).join(' ');

  return (
    <div className="bench-trend" style={{ marginTop: 10 }}>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={t('toolbelt.stressChart.aria')} className="bench-trend-svg">
        {[0.25, 0.5, 0.75].map((f) => (
          <line key={f} x1={PAD} x2={W - PAD} y1={PAD + (H - PAD * 2) * f} y2={PAD + (H - PAD * 2) * f} stroke="var(--line)" strokeDasharray="4 4" strokeWidth={1} />
        ))}
        <text x={PAD} y={12} fill="#888" fontSize={10}>max {hi.toFixed(0)}°C</text>
        <text x={PAD} y={H - 8} fill="#888" fontSize={10}>min {lo.toFixed(0)}°C</text>
        <text x={PAD} y={H - PAD + 12} fill="#888" fontSize={10}>0s</text>
        <text x={W - PAD} y={H - 8} textAnchor="end" fill="#888" fontSize={10}>
          {(points[points.length - 1].sec).toFixed(0)}s
        </text>
        <polyline points={pts} fill="none" stroke="var(--accent-strong)" strokeWidth={1.8} strokeLinejoin="round" strokeLinecap="round" />
      </svg>
      <div className="bench-trend-legend">
        <span><i style={{ background: 'var(--accent-strong)' }} /> {t('toolbelt.stressChart.legendCpu')}</span>
        <span className="muted small">{t('toolbelt.stressChart.peak', { peak: hi.toFixed(1), n: points.length })}</span>
      </div>
    </div>
  );
}

// ── R115 基准跑分历史对比 ──
// 把同一基准历次跑分的分数画成时间序条形图：睿频/散热/后台负载对分数的影响、
// 分数随时间的漂移一眼可见（跑分文本已存 localStorage，无需后端改动）。
// 每类基准定义自己解析一条「主分数行」的正则（label + 数值 + 单位），
// 数值缺失的跑分记录如实跳过（如 bench_disk 失败/被拒），不猜 0。

export interface ScoreHistorySeries {
  label: string;
  value: number;
  unit: string;
  at: number;
}

/// 通用解析：在历史结果文本里匹配「行首 label：数值 单位」，
/// 数值容忍千分位逗号与科学计数法（12,345.6 / 9.9e7 等文本仍能解析）。
/// 返回 {label, value, unit}。导出于单测：锁住 bench_cpu / bench_memory /
/// bench_disk 的输出格式契约。
export function parseScoreLine(text: string, label: string): { value: number; unit: string } | null {
  for (const line of text.split('\n')) {
    const t = line.trim();
    if (!t.startsWith(label)) continue;
    // 行首 label（全角冒号「：」是后端输出格式，数据非文案）——@i18n-keep
    const m = t.match(/^([^:：]+)[:：]\s*([\d.,]+(?:[eE][+-]?\d+)?)\s*([^\s]*)\s*$/); // @i18n-keep
    if (!m) continue;
    const value = parseFloat(m[2].replace(/,/g, ''));
    if (!Number.isFinite(value) || value <= 0) continue;
    return { value, unit: m[3] };
  }
  return null;
}

/// 从历次跑分结果里抽一组主分数（bench_cpu=整数 ops/s，bench_memory=读带宽 MB/s，
/// bench_disk=顺序读 MB/s）。旧→新排序（最近在右）；无有效分数的返回空数组。
export function buildScoreSeries(
  history: { at: number; label: string; result: string }[],
  label: string,
): ScoreHistorySeries[] {
  const out: ScoreHistorySeries[] = [];
  for (const h of history) {
    const parsed = parseScoreLine(h.result, label);
    if (parsed) out.push({ label: h.label, value: parsed.value, unit: parsed.unit, at: h.at });
  }
  return out;
}

/// 分数历史条形图：每根条 = 一次跑分（旧→新 → 左→右），Y 值即分数；
/// 最高分那根用强调色，其余用主题色——峰值一眼可见。
export function ScoreHistoryChart({ series }: { series: ScoreHistorySeries[] }) {
  const t = useT();
  const W = 720;
  const H = 160;
  const PAD = 30;

  if (series.length === 0) return null;
  const vals = series.map((s) => s.value);
  const max = Math.max(...vals);
  const min = Math.min(...vals);
  const span = max - min || 1;
  const barW = Math.min(46, (W - PAD * 2) / series.length - 4);
  const yAt = (v: number) => H - PAD - ((v - min) / span) * (H - PAD * 2 - 24);

  return (
    <div className="bench-trend" style={{ marginTop: 10 }}>
      {series.length >= 2 && (
        <div className="bench-stress-hist-title muted small">{t('toolbelt.scoreChart.title')}</div>
      )}
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={t('toolbelt.scoreChart.title')} className="bench-trend-svg">
        <text x={PAD} y={12} fill="#888" fontSize={10}>{t('toolbelt.scoreChart.maxLabel', { max: max.toLocaleString() })}</text>
        <text x={PAD} y={H - PAD + 14} fill="#888" fontSize={10}>{t('toolbelt.scoreChart.minLabel', { min: min.toLocaleString() })}</text>
        {series.map((s, i) => {
          const x = PAD + i * ((W - PAD * 2) / series.length) + ((W - PAD * 2) / series.length - barW) / 2;
          const h = Math.max(2, yAt(min) - yAt(s.value));
          const isBest = s.value === max;
          return (
            <g key={i}>
              <rect x={x} y={yAt(s.value)} width={barW} height={h} rx={2} fill={isBest ? 'var(--accent-strong)' : 'var(--line)'} />
              <text x={x + barW / 2} y={yAt(s.value) - 4} textAnchor="middle" fontSize={9} fill="#888">
                {s.value >= 1000 ? `${(s.value / 1000).toFixed(s.value >= 100000 ? 0 : 1)}k` : s.value.toFixed(0)}
              </text>
              <text x={x + barW / 2} y={H - 4} textAnchor="middle" fontSize={8} fill="#888">
                {new Date(s.at).toLocaleDateString([], { month: '2-digit', day: '2-digit' })}
              </text>
            </g>
          );
        })}
      </svg>
      <div className="bench-trend-legend">
        {series.length > 0 && (
          <span>
            <i style={{ background: 'var(--accent-strong)' }} /> {t('toolbelt.scoreChart.bestLabel')}
          </span>
        )}
        <span className="muted small">
          {t('toolbelt.scoreChart.countLabel', { n: series.length, unit: series[series.length - 1].unit })}
        </span>
      </div>
    </div>
  );
}

// ── R113 压测历史温度叠画对比 ──
// 把历次压测的温度曲线叠画在一张图上：散热状态/峰值随时间的演变一眼可见
// （最近一次最亮、较旧的渐暗）。X 轴是压测秒数（各次时长可能不同，按各自
// 实际秒数对齐起点），Y 轴统一 0–100℃（与 StressTempChart 同口径，跨次可比）。
export interface StressHistorySeries {
  label: string;
  points: StressCurvePoint[];
  color: string;
}

export function StressHistoryChart({ series }: { series: StressHistorySeries[] }) {
  const t = useT();
  const W = 720;
  const H = 160;
  const PAD = 30;

  const usable = series
    .map((s) => ({ ...s, vals: s.points.map((p) => p.temp) }))
    .filter((s) => s.vals.length >= 2);
  if (usable.length === 0) return null;

  const yAt = (v: number) => H - PAD - (v / 100) * (H - PAD * 2);

  return (
    <div className="bench-trend" style={{ marginTop: 10 }}>
      <div className="bench-stress-hist-title muted small">{t('toolbelt.stressChart.historyTitle')}</div>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={t('toolbelt.stressChart.historyTitle')} className="bench-trend-svg">
        {[0.25, 0.5, 0.75].map((f) => (
          <line key={f} x1={PAD} x2={W - PAD} y1={PAD + (H - PAD * 2) * f} y2={PAD + (H - PAD * 2) * f} stroke="var(--line)" strokeDasharray="4 4" strokeWidth={1} />
        ))}
        <text x={PAD} y={12} fill="#888" fontSize={10}>100°C</text>
        <text x={PAD} y={H - 8} fill="#888" fontSize={10}>0°C</text>
        <text x={PAD} y={H - PAD + 12} fill="#888" fontSize={10}>0s</text>
        {usable.map((s) => {
          const xAt = (i: number) => PAD + (i / Math.max(s.points.length - 1, 1)) * (W - PAD * 2);
          const pts = s.points.map((p, i) => `${xAt(i).toFixed(1)},${yAt(p.temp).toFixed(1)}`).join(' ');
          return (
            <polyline key={s.label} points={pts} fill="none" stroke={s.color} strokeWidth={1.4} strokeLinejoin="round" strokeLinecap="round" opacity={0.85} />
          );
        })}
      </svg>
      <div className="bench-trend-legend">
        {usable.map((s) => (
          <span key={s.label} className="bench-stress-hist-item">
            <i style={{ background: s.color }} /> {s.label}
            <b className="bench-stress-hist-peak">{Math.max(...s.vals).toFixed(0)}°C</b>
          </span>
        ))}
        <span className="muted small">{t('toolbelt.stressChart.historyNote')}</span>
      </div>
    </div>
  );
}

// ── R116 温度分布色阶条 ──
// 把长期历史里的 CPU 温度样本按区间（TEMP_RAMP 色阶）统计，画成一条分段
// 色带 + 峰值/均值/样本摘要——「这台机器日常散热表现如何」一眼可见。
// 只统计有温度的样本；无温度（null / 采集失败）的样本如实跳过。

export interface TempBucket {
  label: string;
  count: number;
  color: string;
}

export type Ramp = ReadonlyArray<{ label: string; to: number; color: string }>;

export type HealthRating = 'good' | 'fair' | 'watch';

/// 高值占比 → 健康度评级（纯函数，可单测）：<5% 优秀 / <20% 良好 /
/// 否则关注。温度与内存共用同一档位（阈值语义已由调用方注入 highThreshold）。
export function rateHealth(highRatio: number): HealthRating {
  if (highRatio < 0.05) return 'good';
  if (highRatio < 0.2) return 'fair';
  return 'watch';
}

/// 数值样本 → 分布桶（纯函数，可单测）。分层口径与传入 ramp 一致；
/// 只统计数字值，非数字 / 非有限值跳过（不猜 0）。
export function buildBuckets(values: number[], ramp: Ramp): TempBucket[] {
  const buckets = ramp.map((r) => ({ label: r.label, count: 0, color: r.color }));
  for (const v of values) {
    if (!Number.isFinite(v)) continue;
    const idx = ramp.findIndex((r) => v < r.to);
    if (idx >= 0) buckets[idx].count += 1;
    else buckets[buckets.length - 1].count += 1;
  }
  return buckets;
}

/// 分布摘要（纯函数，可单测）：峰值 / 均值 / 有效样本数 / 高值占比
/// （≥ highThreshold，阈值由调用方按维度界定：温度 70℃ / 内存 85%）。
/// 有效样本为 0 时返回 null（调用方不渲染摘要）。
export function distSummary(values: number[], highThreshold: number): {
  peak: number;
  mean: number;
  n: number;
  highRatio: number;
} | null {
  const valid = values.filter((v) => Number.isFinite(v));
  if (valid.length === 0) return null;
  return {
    peak: Math.max(...valid),
    mean: valid.reduce((a, b) => a + b, 0) / valid.length,
    n: valid.length,
    highRatio: valid.filter((v) => v >= highThreshold).length / valid.length,
  };
}

/// 通用分布色阶条（R117 泛化）：把数值样本按 ramp 区间统计成分段色带 +
/// 峰值/均值/摘要 + 高值占比解读。i18n 文案按 ns 取（heatDist / memDist
/// 两组键同构）。温度与内存占用两路复用。
export function DistributionBar({
  values,
  ramp,
  highThreshold,
  ns,
}: {
  values: number[];
  ramp: Ramp;
  highThreshold: number;
  ns: 'toolbelt.heatDist' | 'toolbelt.memDist';
}) {
  const t = useT();
  if (values.length === 0) return null;
  const buckets = buildBuckets(values, ramp);
  const sum = buckets.reduce((a, b) => a + b.count, 0) || 1;
  const summary = distSummary(values, highThreshold);

  return (
    <div className="bench-trend" style={{ marginTop: 10 }}>
      <div className="bench-stress-hist-title muted small">{t(`${ns}.title`)}</div>
      <div className="heat-dist-bar" role="img" aria-label={t(`${ns}.aria`)}>
        {buckets.map((b) =>
          b.count > 0 ? (
            <div
              key={b.label}
              className="heat-dist-seg"
              style={{ width: `${(b.count / sum) * 100}%`, background: b.color }}
              title={`${b.label}: ${b.count}`}
            />
          ) : null,
        )}
      </div>
      <div className="heat-dist-legend">
        {buckets.map((b) => (
          <span key={b.label} className="heat-dist-item muted small">
            <i style={{ background: b.color }} /> {b.label}
            <b>{b.count}</b>
          </span>
        ))}
      </div>
      {summary && (
        <p className="muted small heat-dist-summary">
          {t(`${ns}.summary`, { n: summary.n, peak: summary.peak.toFixed(1), mean: summary.mean.toFixed(1) })}
          {' · '}
          {summary.highRatio > 0.1
            ? t(`${ns}.hot`, { pct: (summary.highRatio * 100).toFixed(0) })
            : t(`${ns}.hotNone`)}
        </p>
      )}
      <div className="bench-trend-legend">
        <span className="muted small">{t(`${ns}.note`)}</span>
      </div>
    </div>
  );
}

// ── R116 温度分布（兼容薄封装：R116 单测与 bench.tsx 接线不变）──
export function buildTempBuckets(temps: number[]): TempBucket[] {
  return buildBuckets(temps, TEMP_RAMP);
}
export function tempSummary(temps: number[]): ReturnType<typeof distSummary> {
  return distSummary(temps, 70);
}
export function HeatDistributionBar({ temps }: { temps: number[] }) {
  return <DistributionBar values={temps} ramp={TEMP_RAMP} highThreshold={70} ns="toolbelt.heatDist" />;
}

// ── R118 长期健康度评级卡 ──
// 把温度/内存两条分布的高值占比汇总成一句人话结论：<5% 优秀、<20% 良好、
// 否则关注。复用 distSummary（已是纯函数）；任一维度无有效样本则该维度不
// 评级（如实缺省）。放长期历史视图顶部——先出结论，再看下方分布明细。

export function HealthRatingCard({
  cpuTemps,
  memPercents,
}: {
  cpuTemps: number[];
  memPercents: number[];
}) {
  const t = useT();
  const tS = distSummary(cpuTemps, 70);
  const mS = distSummary(memPercents, 85);
  if (!tS && !mS) return null;
  const tRating = tS ? rateHealth(tS.highRatio) : null;
  const mRating = mS ? rateHealth(mS.highRatio) : null;

  return (
    <div className="health-rating-card" role="status">
      <div className="health-rating-row">
        <span className="muted small">{t('toolbelt.healthRating.cpu')}</span>
        {tRating ? (
          <span className={`health-rating-badge health-rating-${tRating}`}>
            {t(`toolbelt.healthRating.${tRating}`)}
            <small>{t('toolbelt.healthRating.ratio', { pct: (tS!.highRatio * 100).toFixed(0) })}</small>
          </span>
        ) : (
          <span className="muted small">{t('toolbelt.healthRating.na')}</span>
        )}
      </div>
      <div className="health-rating-row">
        <span className="muted small">{t('toolbelt.healthRating.mem')}</span>
        {mRating ? (
          <span className={`health-rating-badge health-rating-${mRating}`}>
            {t(`toolbelt.healthRating.${mRating}`)}
            <small>{t('toolbelt.healthRating.ratio', { pct: (mS!.highRatio * 100).toFixed(0) })}</small>
          </span>
        ) : (
          <span className="muted small">{t('toolbelt.healthRating.na')}</span>
        )}
      </div>
      <div className="bench-trend-legend">
        <span className="muted small">{t('toolbelt.healthRating.note')}</span>
      </div>
    </div>
  );
}


